//! wash-fps `client/DemoDownload.ts`: a recording over HTTP(S) with an idle
//! and a total deadline, and the words the player sees when it fails; with
//! `DemoService.demoBaseUrl` and the file it lands in.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use shagger::network::client::http_fetch::{FetchError, fetch_at_most};
use shagger::shared::demo::demo_reader::DemoReader;

/// `DEMO_DOWNLOAD_IDLE_MS`: file transfers need a longer deadline than the
/// archive's metadata.
pub const DEMO_DOWNLOAD_IDLE: Duration = Duration::from_secs(60);
/// `DEMO_DOWNLOAD_TOTAL_MS`.
pub const DEMO_DOWNLOAD_TOTAL: Duration = Duration::from_secs(15 * 60);
/// The largest recording fetched: a match's demo is a few megabytes
/// gzipped, so a body past this is not one and must not fill the memory.
const MAX_DEMO_BYTES: u64 = 256 * 1024 * 1024;

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
        FetchError::TooLarge => "The recording is larger than a demo can be.".to_owned(),
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

/// `downloadDemo` for one recording, saved into the cache. Checking a saved
/// copy, the transfer, the check of what arrived and the save all run on a
/// thread of their own: a recording is megabytes, and reading it on the game
/// thread held up the menu's frame.
#[derive(Debug)]
pub struct DemoDownload {
    pub filename: String,
    worker: Option<Worker>,
    pub state: DemoDownloadState,
}

#[derive(Debug)]
struct Worker {
    states: mpsc::Receiver<DemoDownloadState>,
    cancel: Arc<AtomicBool>,
}

/// Whether `bytes` open as a demo: its header and index read.
fn reads_as_demo(bytes: &[u8]) -> bool {
    DemoReader::new(bytes.to_vec()).is_ok_and(|mut r| r.read_header().is_ok() && r.read_index().is_ok())
}

impl DemoDownload {
    /// Starts fetching `filename` from `base`, or finds it already saved.
    #[must_use]
    pub fn start(base: &str, filename: &str) -> Self {
        let failed = |why: &str| Self {
            filename: filename.to_owned(),
            worker: None,
            state: DemoDownloadState::Failed(why.to_owned()),
        };
        let Some(name) = safe_name(filename) else {
            return failed("That recording's name is not a demo file.");
        };
        let (send, states) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let url = format!("{base}/{name}");
        let (name, stop) = (name.to_owned(), cancel.clone());
        let spawned = std::thread::Builder::new()
            .name("demo download".to_owned())
            .spawn(move || {
                let state = download(&url, &name, &stop, &mut |received, total| {
                    let _ = send.send(DemoDownloadState::Loading { received, total });
                });
                let _ = send.send(state);
            });
        if spawned.is_err() {
            return failed("Could not start the download.");
        }
        Self {
            filename: filename.to_owned(),
            worker: Some(Worker { states, cancel }),
            state: DemoDownloadState::Loading {
                received: 0,
                total: None,
            },
        }
    }

    /// Takes what the transfer reported since the last frame.
    pub fn poll(&mut self) {
        let Some(worker) = &self.worker else { return };
        loop {
            match worker.states.try_recv() {
                Ok(state) => {
                    let done = !matches!(state, DemoDownloadState::Loading { .. });
                    self.state = state;
                    if done {
                        self.worker = None;
                        return;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.state = DemoDownloadState::Failed("The download stopped.".to_owned());
                    return;
                }
            }
        }
    }

    /// `demoLoadCancelled`.
    pub fn cancel(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel.store(true, Ordering::Relaxed);
        }
        self.state = DemoDownloadState::Failed(download_message(&FetchError::Cancelled));
    }
}

/// The download thread: a saved copy that reads as a demo, or the recording
/// fetched, checked and saved.
fn download(
    url: &str,
    name: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> DemoDownloadState {
    // A saved copy is used only if it reads as a demo; a damaged one goes,
    // and the recording is fetched again.
    let saved = demo_cache_dir().map(|d| d.join(name));
    if let Some(path) = &saved
        && path.is_file()
    {
        if std::fs::read(path).is_ok_and(|b| reads_as_demo(&b)) {
            return DemoDownloadState::Ready(path.clone());
        }
        let _ = std::fs::remove_file(path);
    }
    let bytes = match fetch_at_most(
        url,
        DEMO_DOWNLOAD_IDLE,
        DEMO_DOWNLOAD_TOTAL,
        Some(MAX_DEMO_BYTES),
        cancel,
        progress,
    ) {
        Ok(bytes) => bytes,
        Err(error) => return DemoDownloadState::Failed(download_message(&error)),
    };
    // An error page sent as 200, or a cut-short body, is not kept: it would
    // fail every later Play.
    if !reads_as_demo(&bytes) {
        return DemoDownloadState::Failed("The recording arrived damaged. Try again.".to_owned());
    }
    let Some(path) = saved else {
        return DemoDownloadState::Failed("Could not save the recording: no config folder".to_owned());
    };
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| shagger::core::atomic_file::write_atomic(&path, &bytes));
    match saved {
        Ok(()) => DemoDownloadState::Ready(path),
        Err(e) => DemoDownloadState::Failed(format!("Could not save the recording: {e}")),
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
