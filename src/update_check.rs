//! The update check: washoi.net publishes the newest released game under
//! `/client/` with a `latest.json` index (wash-fps
//! `scripts/deployment/client-release.sh`). The menu reads it once per run,
//! and when a newer version is out it offers the package for this machine.
//!
//! No TypeScript source: the browser game is always current.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;
use shagger::network::client::http_fetch::{FetchEvent, HttpFetch};

/// Where the site publishes the index.
pub const LATEST_URL: &str = "https://washoi.net/client/latest.json";
/// Downloads are offered only from here.
const DOWNLOAD_PREFIX: &str = "https://washoi.net/client/";
/// The index is a few hundred bytes; anything larger is not it.
const MAX_INDEX: usize = 64 * 1024;

/// A newer game, and its package for this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    /// The package's address; `None` when no package fits this machine.
    pub url: Option<String>,
}

enum State {
    Idle,
    Fetching(HttpFetch),
    /// Checked: the update, unless there is none or the player dismissed it.
    Done(Option<Update>),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);

/// Starts the check, once per run.
pub fn start() {
    let Ok(mut state) = STATE.lock() else { return };
    if matches!(*state, State::Idle) {
        *state = State::Fetching(HttpFetch::start(
            LATEST_URL,
            Duration::from_secs(10),
            Duration::from_secs(20),
        ));
    }
}

/// The update to offer, once the check has answered.
pub fn available() -> Option<Update> {
    let mut state = STATE.lock().ok()?;
    if let State::Fetching(fetch) = &*state {
        let mut answer = None;
        while let Some(event) = fetch.poll() {
            match event {
                FetchEvent::Progress { received, .. } if received as usize > MAX_INDEX => {
                    fetch.cancel();
                }
                FetchEvent::Progress { .. } => {}
                FetchEvent::Done(body) => {
                    answer = Some(parse_latest(&body).and_then(|latest| {
                        offer(&latest, crate::VERSION, std::env::consts::OS, std::env::consts::ARCH)
                    }));
                    break;
                }
                FetchEvent::Failed(why) => {
                    eprintln!("update check: {why:?}");
                    answer = Some(None);
                    break;
                }
            }
        }
        if let Some(update) = answer {
            if let Some(u) = &update {
                println!("update check: Washoi {} is available", u.version);
            }
            *state = State::Done(update);
        }
    }
    match &*state {
        State::Done(update) => update.clone(),
        _ => None,
    }
}

/// Hides the offer for the rest of this run.
pub fn dismiss() {
    if let Ok(mut state) = STATE.lock() {
        *state = State::Done(None);
    }
}

/// `latest.json` as the site writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub version: String,
    /// `(platform, url)`, platform as `linux-x86_64`.
    pub files: Vec<(String, String)>,
}

/// Reads the index; `None` when it is not one.
#[must_use]
pub fn parse_latest(body: &[u8]) -> Option<Latest> {
    if body.len() > MAX_INDEX {
        return None;
    }
    let json: Value = serde_json::from_slice(body).ok()?;
    let version = json.get("version")?.as_str()?.to_owned();
    parse_version(&version)?;
    let files = json
        .get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(|f| {
                    let platform = f.get("platform")?.as_str()?;
                    let url = f.get("url")?.as_str()?;
                    Some((platform.to_owned(), url.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Latest { version, files })
}

/// The offer for this build on this machine: `None` unless `latest` is newer.
#[must_use]
pub fn offer(latest: &Latest, current: &str, os: &str, arch: &str) -> Option<Update> {
    if !newer(&latest.version, current) {
        return None;
    }
    let platform = format!("{os}-{arch}");
    let url = latest
        .files
        .iter()
        .find(|(p, url)| *p == platform && safe_url(url))
        .map(|(_, url)| url.clone());
    Some(Update {
        version: latest.version.clone(),
        url,
    })
}

/// `1.2.3` or `1.2.3-rc.1`: the numbers, and whether it is a pre-release.
fn parse_version(v: &str) -> Option<([u64; 3], bool)> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let (core, pre) = match v.find(['-', '+']) {
        Some(at) => (&v[..at], v[at..].starts_with('-')),
        None => (v, false),
    };
    let mut parts = core.split('.');
    let mut out = [0; 3];
    for n in &mut out {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *n = part.parse().ok()?;
    }
    parts.next().is_none().then_some((out, pre))
}

/// Whether `latest` is a later version than `current`; a pre-release comes
/// before its release, and an unreadable version is never newer.
#[must_use]
pub fn newer(latest: &str, current: &str) -> bool {
    let (Some((l, l_pre)), Some((c, c_pre))) = (parse_version(latest), parse_version(current)) else {
        return false;
    };
    l > c || (l == c && c_pre && !l_pre)
}

/// Only the site's own packages, and nothing a shell or the browser could
/// read as more than one address.
fn safe_url(url: &str) -> bool {
    url.strip_prefix(DOWNLOAD_PREFIX).is_some_and(|rest| {
        !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b))
            && !rest.contains("..")
    })
}

/// Opens the package's address in the player's browser.
///
/// # Errors
/// The browser could not be started.
pub fn open_in_browser(url: &str) -> Result<(), String> {
    if !safe_url(url) {
        return Err(format!("not a washoi.net download: {url}"));
    }
    let mut command = if cfg!(target_os = "windows") {
        std::process::Command::new("explorer")
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    command
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open the browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"{"version":"0.3.0","tag":"v0.3.0","publishedAt":"2026-11-03T10:00:00Z",
        "files":[
          {"platform":"linux-x86_64","name":"washoi-0.3.0-linux-x86_64.tar.gz",
           "url":"https://washoi.net/client/0.3.0/washoi-0.3.0-linux-x86_64.tar.gz","size":1,"sha256":"00"},
          {"platform":"macos-aarch64","name":"washoi-0.3.0-macos-aarch64.zip",
           "url":"https://washoi.net/client/0.3.0/washoi-0.3.0-macos-aarch64.zip","size":1,"sha256":"00"},
          {"platform":"windows-x86_64","name":"x","url":"https://evil.example/x.zip"}
        ]}"#;

    #[test]
    fn versions_compare_by_number_and_a_release_beats_its_pre_release() {
        assert!(newer("0.3.0", "0.2.9"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(newer("1.0.0", "1.0.0-rc.1"));
        assert!(newer("v1.0.1", "1.0.0"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("1.0.0-rc.1", "1.0.0"));
        assert!(!newer("0.2.0", "0.3.0"));
        assert!(!newer("1.0", "0.1.0"));
        assert!(!newer("1.0.0.0", "0.1.0"));
        assert!(!newer("x", "0.1.0"));
        assert!(!newer("2.0.0", "garbage"));
    }

    #[test]
    fn the_index_offers_this_machines_package() {
        let latest = parse_latest(INDEX.as_bytes()).unwrap();
        assert_eq!(latest.version, "0.3.0");
        assert_eq!(
            offer(&latest, "0.2.0", "linux", "x86_64"),
            Some(Update {
                version: "0.3.0".into(),
                url: Some("https://washoi.net/client/0.3.0/washoi-0.3.0-linux-x86_64.tar.gz".into()),
            })
        );
        assert_eq!(
            offer(&latest, "0.2.0", "macos", "aarch64").and_then(|u| u.url),
            Some("https://washoi.net/client/0.3.0/washoi-0.3.0-macos-aarch64.zip".into())
        );
        // No package for it: the version is still worth saying.
        assert_eq!(offer(&latest, "0.2.0", "linux", "aarch64").unwrap().url, None);
        // Never an address off the site.
        assert_eq!(offer(&latest, "0.2.0", "windows", "x86_64").unwrap().url, None);
        assert_eq!(offer(&latest, "0.3.0", "linux", "x86_64"), None);
        assert_eq!(offer(&latest, "0.4.0", "linux", "x86_64"), None);
    }

    #[test]
    fn a_broken_index_offers_nothing() {
        assert_eq!(parse_latest(b""), None);
        assert_eq!(parse_latest(b"<html>"), None);
        assert_eq!(parse_latest(br#"{"version":"soon"}"#), None);
        assert_eq!(parse_latest(br#"{"version":3}"#), None);
        let bare = parse_latest(br#"{"version":"9.0.0","files":"no"}"#).unwrap();
        assert!(bare.files.is_empty());
        assert_eq!(parse_latest(&vec![b' '; MAX_INDEX + 1]), None);
    }

    #[test]
    fn only_plain_site_addresses_open() {
        assert!(safe_url(
            "https://washoi.net/client/0.3.0/washoi-0.3.0-linux-x86_64.tar.gz"
        ));
        assert!(!safe_url("https://washoi.net/client/"));
        assert!(!safe_url("https://washoi.net/client/a b"));
        assert!(!safe_url("https://washoi.net/client/x&calc"));
        assert!(!safe_url("https://washoi.net/client/../x"));
        assert!(!safe_url("http://washoi.net/client/x.zip"));
        assert!(!safe_url("https://washoi.net.evil/client/x.zip"));
        assert!(open_in_browser("file:///etc/passwd").is_err());
    }
}
