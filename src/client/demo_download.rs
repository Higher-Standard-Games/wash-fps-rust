//! wash-fps `client/DemoDownload.ts`: a recording over HTTP(S) with an idle
//! and a total deadline, and the words the player sees when it fails; with
//! `DemoService.demoBaseUrl` and the file it lands in.

use std::path::PathBuf;
use std::time::Duration;

use shagger::network::client::http_fetch::{FetchError, FetchEvent, HttpFetch};
use shagger::shared::demo::demo_reader::DemoReader;

/// `DEMO_DOWNLOAD_IDLE_MS`: file transfers need a longer deadline than the
/// archive's metadata.
pub const DEMO_DOWNLOAD_IDLE: Duration = Duration::from_secs(60);
/// `DEMO_DOWNLOAD_TOTAL_MS`.
pub const DEMO_DOWNLOAD_TOTAL: Duration = Duration::from_secs(15 * 60);

/// `DemoService.demoBaseUrl`: `WASHOI_DEMO_BASE_URL` (the browser's
/// `VITE_DEMO_BASE_URL`), else `/data/demos` on the lobby's host, as the
/// browser fetches it from the page's own.
#[must_use]
pub fn demo_base_url(lobby_url: &str) -> String {
    if let Ok(base) = std::env::var("WASHOI_DEMO_BASE_URL")
        && !base.trim().is_empty()
    {
        return base.trim().trim_end_matches('/').to_owned();
    }
    let (scheme, rest) = match lobby_url.split_once("://") {
        Some(("wss", rest)) => ("https", rest),
        Some((_, rest)) => ("http", rest),
        None => ("http", lobby_url),
    };
    let host = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{host}/data/demos")
}

/// The error a failed download shows.
#[must_use]
pub fn download_message(error: &FetchError) -> String {
    match error {
        FetchError::Status(status) => {
            format!("Could not download the recording (HTTP {status}). Please try again.")
        }
        FetchError::Stalled => "The demo download stalled. Please try again.".to_owned(),
        FetchError::TooLong => "The demo download took too long. Please try again.".to_owned(),
        FetchError::Cancelled => "Demo loading cancelled".to_owned(),
        FetchError::Network(e) => format!("Could not download the recording: {e}"),
    }
}

/// Where downloaded recordings are kept: `<config>/demos`.
#[must_use]
pub fn demo_cache_dir() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("demos"))
}

/// A file name the archive gave, if it is one plain name.
fn safe_name(filename: &str) -> Option<&str> {
    let ok = !filename.is_empty()
        && filename.ends_with(".shagdemo")
        && !filename.starts_with('.')
        && filename
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    ok.then_some(filename)
}

/// What a download is doing.
#[derive(Debug)]
pub enum DemoDownloadState {
    Loading {
        received: u64,
        total: Option<u64>,
    },
    /// The recording, saved.
    Ready(PathBuf),
    Failed(String),
}

/// `downloadDemo` for one recording, saved into the cache.
#[derive(Debug)]
pub struct DemoDownload {
    pub filename: String,
    fetch: Option<HttpFetch>,
    pub state: DemoDownloadState,
}

/// Whether `bytes` open as a demo: its header and index read.
fn reads_as_demo(bytes: &[u8]) -> bool {
    DemoReader::new(bytes.to_vec()).is_ok_and(|mut r| r.read_header().is_ok() && r.read_index().is_ok())
}

impl DemoDownload {
    /// Starts fetching `filename` from `base`, or finds it already saved.
    #[must_use]
    pub fn start(base: &str, filename: &str) -> Self {
        let Some(name) = safe_name(filename) else {
            return Self {
                filename: filename.to_owned(),
                fetch: None,
                state: DemoDownloadState::Failed("That recording's name is not a demo file.".to_owned()),
            };
        };
        // A saved copy is used only if it reads as a demo; a damaged one
        // goes, and the recording is fetched again.
        if let Some(path) = demo_cache_dir().map(|d| d.join(name))
            && path.is_file()
            && (std::fs::read(&path).is_ok_and(|b| reads_as_demo(&b)) || {
                let _ = std::fs::remove_file(&path);
                false
            })
        {
            return Self {
                filename: filename.to_owned(),
                fetch: None,
                state: DemoDownloadState::Ready(path),
            };
        }
        Self {
            filename: filename.to_owned(),
            fetch: Some(HttpFetch::start(
                &format!("{base}/{name}"),
                DEMO_DOWNLOAD_IDLE,
                DEMO_DOWNLOAD_TOTAL,
            )),
            state: DemoDownloadState::Loading {
                received: 0,
                total: None,
            },
        }
    }

    /// Takes what the transfer reported since the last frame.
    pub fn poll(&mut self) {
        let Some(fetch) = &self.fetch else { return };
        while let Some(event) = fetch.poll() {
            match event {
                FetchEvent::Progress { received, total } => {
                    self.state = DemoDownloadState::Loading { received, total };
                }
                FetchEvent::Done(bytes) => {
                    self.fetch = None;
                    // An error page sent as 200, or a cut-short body, is not
                    // kept: it would fail every later Play.
                    if !reads_as_demo(&bytes) {
                        self.state = DemoDownloadState::Failed("The recording arrived damaged. Try again.".to_owned());
                        return;
                    }
                    self.state = match self.save(&bytes) {
                        Ok(path) => DemoDownloadState::Ready(path),
                        Err(e) => DemoDownloadState::Failed(format!("Could not save the recording: {e}")),
                    };
                    return;
                }
                FetchEvent::Failed(error) => {
                    self.fetch = None;
                    self.state = DemoDownloadState::Failed(download_message(&error));
                    return;
                }
            }
        }
    }

    fn save(&self, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let dir = demo_cache_dir().ok_or_else(|| std::io::Error::other("no config folder"))?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&self.filename);
        let partial = dir.join(format!("{}.part", self.filename));
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, &path)?;
        Ok(path)
    }

    /// `demoLoadCancelled`.
    pub fn cancel(&mut self) {
        if let Some(fetch) = self.fetch.take() {
            fetch.cancel();
        }
        self.state = DemoDownloadState::Failed(download_message(&FetchError::Cancelled));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_follows_the_lobby_and_names_stay_plain() {
        assert_eq!(demo_base_url("wss://washoi.net/lobby"), "https://washoi.net/data/demos");
        assert_eq!(demo_base_url("ws://localhost:9000"), "http://localhost:9000/data/demos");
        assert_eq!(
            safe_name("washoi_dm2_duel_20261101_143946.shagdemo"),
            Some("washoi_dm2_duel_20261101_143946.shagdemo")
        );
        assert_eq!(safe_name("../x.shagdemo"), None);
        assert_eq!(safe_name("x.zip"), None);
        assert_eq!(
            download_message(&FetchError::Status(404)),
            "Could not download the recording (HTTP 404). Please try again."
        );
    }

    #[test]
    fn only_a_demo_reads_as_one() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/demos");
        let file = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "shagdemo"))
            .unwrap();
        let bytes = std::fs::read(file).unwrap();
        assert!(reads_as_demo(&bytes));
        assert!(!reads_as_demo(b"<html>502 Bad Gateway</html>"));
        assert!(!reads_as_demo(&bytes[..bytes.len().min(40)]));
    }
}
