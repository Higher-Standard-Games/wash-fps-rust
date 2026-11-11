//! The update check: washoi.net publishes the newest released game under
//! `/client/` with a `latest.json` index (wash-fps
//! `scripts/deployment/client-release.sh`). The menu reads it once per run,
//! and when a newer version is out it offers the package for this machine.
//!
//! A game that can replace itself does so before the data sync instead
//! (`self_update`, `apply`): it downloads the newer package, checks its size
//! and SHA-256 against the index, and puts it in place. On Windows the
//! installer it ran silently does that; an AppImage replaces its own file;
//! a `Washoi.app` swaps its bundle. Then the new game starts.
//!
//! No TypeScript source: the browser game is always current.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use shagger::network::client::http_fetch::{FetchEvent, HttpFetch, fetch_at_most};

/// Where the site publishes the index.
pub const LATEST_URL: &str = "https://washoi.net/client/latest.json";
/// Downloads are offered only from here.
const DOWNLOAD_PREFIX: &str = "https://washoi.net/client/";
/// The index is a few hundred bytes; anything larger is not it.
const MAX_INDEX: usize = 64 * 1024;
/// The largest installer the game will download.
const MAX_INSTALLER: u64 = 512 * 1024 * 1024;

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
    pub files: Vec<File>,
}

/// One package in the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// As `linux-x86_64`.
    pub platform: String,
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
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
                    let text = |k: &str| f.get(k).and_then(Value::as_str).map(str::to_owned);
                    Some(File {
                        platform: text("platform")?,
                        name: text("name").unwrap_or_default(),
                        url: text("url")?,
                        size: f.get("size").and_then(Value::as_u64),
                        sha256: text("sha256"),
                    })
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
        .find(|f| f.platform == platform && safe_url(&f.url))
        .map(|f| f.url.clone());
    Some(Update {
        version: latest.version.clone(),
        url,
    })
}

/// How this copy of the game replaces itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Installed by `tools/windows/washoi.iss`: a newer installer is run.
    WindowsInstaller,
    /// Run as this AppImage (the runtime's `APPIMAGE`): the file is replaced.
    AppImage(PathBuf),
    /// Run from this `Washoi.app`: the bundle is replaced.
    MacApp(PathBuf),
}

impl Target {
    /// The end of the package's name in the index (`tools/package.sh`).
    #[must_use]
    pub fn suffix(&self) -> &'static str {
        match self {
            Self::WindowsInstaller => "-setup.exe",
            Self::AppImage(_) => ".AppImage",
            Self::MacApp(_) => ".zip",
        }
    }
}

/// A newer package to download: the one [`Target::suffix`] names, with the
/// size and SHA-256 the site recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installer {
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// The package for this machine ending in `suffix` when `latest` is newer;
/// `None` without one, or without the size and hash to check it by.
#[must_use]
pub fn installer(latest: &Latest, current: &str, os: &str, arch: &str, suffix: &str) -> Option<Installer> {
    if !newer(&latest.version, current) {
        return None;
    }
    let platform = format!("{os}-{arch}");
    let file = latest
        .files
        .iter()
        .find(|f| f.platform == platform && f.name.ends_with(suffix) && f.url.ends_with(suffix) && safe_url(&f.url))?;
    let size = file.size.filter(|s| *s > 0 && *s <= MAX_INSTALLER)?;
    let sha256 = file
        .sha256
        .as_deref()
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))?
        .to_ascii_lowercase();
    Some(Installer {
        version: latest.version.clone(),
        url: file.url.clone(),
        size,
        sha256,
    })
}

/// How this game updates itself, if it does: a release build that the
/// Windows installer put in place (its uninstaller sits beside the exe), that
/// runs as an AppImage, or that runs from a `.app` bundle macOS has not
/// moved to a read-only place (App Translocation).
#[must_use]
pub fn self_update_target() -> Option<Target> {
    if cfg!(debug_assertions) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    if cfg!(windows) {
        return exe
            .parent()
            .is_some_and(|d| d.join("unins000.exe").is_file())
            .then_some(Target::WindowsInstaller);
    }
    if cfg!(target_os = "macos") {
        // `Washoi.app/Contents/MacOS/washoi`.
        let bundle = exe.parent()?.parent()?.parent()?;
        let is_app = bundle.extension().is_some_and(|e| e == "app");
        let translocated = bundle.to_string_lossy().contains("/AppTranslocation/");
        return (is_app && !translocated).then(|| Target::MacApp(bundle.to_path_buf()));
    }
    let image = PathBuf::from(std::env::var_os("APPIMAGE")?);
    (image.is_absolute() && image.is_file()).then_some(Target::AppImage(image))
}

/// What [`self_update`] reports while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateProgress {
    Checking,
    Downloading { version: String, received: u64, total: u64 },
}

/// Fetches the index and, when a newer package for `target` is out,
/// downloads and checks it: its path (in `dir`, or beside the AppImage it
/// replaces), or `None` when this game is current.
///
/// # Errors
/// Why the index or the package could not be had; the caller plays on.
pub fn self_update(
    target: &Target,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(UpdateProgress),
) -> Result<Option<PathBuf>, String> {
    progress(UpdateProgress::Checking);
    let index = fetch_at_most(
        LATEST_URL,
        Duration::from_secs(5),
        Duration::from_secs(10),
        Some(MAX_INDEX as u64),
        cancel,
        &mut |_, _| {},
    )
    .map_err(|e| format!("update check: {e:?}"))?;
    let latest = parse_latest(&index).ok_or("update check: not an index")?;
    // Old downloads are not kept.
    let _ = std::fs::remove_dir_all(dir);
    let Some(update) = installer(
        &latest,
        crate::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        target.suffix(),
    ) else {
        return Ok(None);
    };
    println!("update: downloading Washoi {}", update.version);
    let version = update.version.clone();
    let body = fetch_at_most(
        &update.url,
        Duration::from_secs(20),
        Duration::from_secs(30 * 60),
        Some(update.size),
        cancel,
        &mut |received, _| {
            progress(UpdateProgress::Downloading {
                version: version.clone(),
                received,
                total: update.size,
            });
        },
    )
    .map_err(|e| format!("update download: {e:?}"))?;
    verify(&body, &update)?;
    let path = match target {
        // Beside the file it replaces, so the swap is one rename.
        Target::AppImage(image) => {
            let mut name = image.file_name().ok_or("update: the AppImage has no name")?.to_owned();
            name.push(".update");
            image.with_file_name(name)
        }
        Target::WindowsInstaller | Target::MacApp(_) => {
            std::fs::create_dir_all(dir).map_err(|e| format!("update: {e}"))?;
            dir.join(format!("washoi-{}{}", update.version, target.suffix()))
        }
    };
    shagger::core::atomic_file::write_atomic(&path, &body).map_err(|e| format!("update: {e}"))?;
    Ok(Some(path))
}

/// The download is the file the site recorded.
fn verify(body: &[u8], update: &Installer) -> Result<(), String> {
    if body.len() as u64 != update.size {
        return Err(format!("update: {} bytes, not {}", body.len(), update.size));
    }
    let hash: String = Sha256::digest(body).iter().map(|b| format!("{b:02x}")).collect();
    if hash != update.sha256 {
        return Err("update: the download does not match its SHA-256".into());
    }
    Ok(())
}

/// Puts the downloaded package in place and starts the new game; the caller
/// then ends this one.
///
/// # Errors
/// The new game could not be put in place or started; the old one is kept.
pub fn apply(target: &Target, download: &Path) -> Result<(), String> {
    match target {
        // The installer waits for this game to close, replaces it and starts
        // the new one (`tools/windows/washoi.iss`, `/RELAUNCH=1`).
        Target::WindowsInstaller => std::process::Command::new(download)
            .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/RELAUNCH=1"])
            .spawn()
            .map(drop)
            .map_err(|e| format!("update: could not start the installer: {e}")),
        Target::AppImage(image) => {
            make_executable(download)?;
            // A running AppImage keeps its open file; the name gets the new one.
            std::fs::rename(download, image).map_err(|e| format!("update: could not replace the AppImage: {e}"))?;
            std::process::Command::new(image)
                .spawn()
                .map(drop)
                .map_err(|e| format!("update: could not start the new AppImage: {e}"))
        }
        Target::MacApp(bundle) => replace_bundle(bundle, download),
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("update: {e}"))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Unpacks the zip (`ditto`, as `tools/package.sh` packed it) beside the
/// bundle, swaps the two by rename, and opens the new one.
fn replace_bundle(bundle: &Path, zip: &Path) -> Result<(), String> {
    let parent = bundle.parent().ok_or("update: the app has no folder")?;
    let id = std::process::id();
    let unpacked = parent.join(format!(".washoi-update-{id}"));
    let _ = std::fs::remove_dir_all(&unpacked);
    let ok = std::process::Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(&unpacked)
        .status()
        .map_err(|e| format!("update: could not unpack: {e}"))?;
    let new_app = unpacked.join("Washoi.app");
    if !ok.success() || !new_app.join("Contents/MacOS/washoi").is_file() {
        let _ = std::fs::remove_dir_all(&unpacked);
        return Err("update: the download holds no Washoi.app".into());
    }
    let old = parent.join(format!(".washoi-old-{id}.app"));
    if let Err(e) = std::fs::rename(bundle, &old) {
        let _ = std::fs::remove_dir_all(&unpacked);
        return Err(format!("update: could not move the app aside: {e}"));
    }
    if let Err(e) = std::fs::rename(&new_app, bundle) {
        let _ = std::fs::rename(&old, bundle);
        let _ = std::fs::remove_dir_all(&unpacked);
        return Err(format!("update: could not put the new app in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&unpacked);
    let _ = std::fs::remove_dir_all(&old);
    std::process::Command::new("open")
        .arg("-n")
        .arg(bundle)
        .spawn()
        .map(drop)
        .map_err(|e| format!("update: could not open the new app: {e}"))
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

    const SHA: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn with_installer(file: &str) -> Latest {
        parse_latest(format!(r#"{{"version":"0.3.0","files":[{file}]}}"#).as_bytes()).unwrap()
    }

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
    fn the_installer_needs_its_name_size_and_hash() {
        let good = format!(
            r#"{{"platform":"windows-x86_64","name":"washoi-0.3.0-windows-x86_64-setup.exe",
               "url":"https://washoi.net/client/0.3.0/washoi-0.3.0-windows-x86_64-setup.exe","size":4,"sha256":"{}"}}"#,
            SHA.to_uppercase()
        );
        let latest = with_installer(&format!(
            r#"{{"platform":"windows-x86_64","name":"washoi-0.3.0-windows-x86_64.zip",
               "url":"https://washoi.net/client/0.3.0/washoi-0.3.0-windows-x86_64.zip","size":4,"sha256":"{SHA}"}},{good}"#
        ));
        let found = installer(&latest, "0.2.0", "windows", "x86_64", "-setup.exe").unwrap();
        assert_eq!(
            found.url,
            "https://washoi.net/client/0.3.0/washoi-0.3.0-windows-x86_64-setup.exe"
        );
        assert_eq!((found.size, found.sha256.as_str()), (4, SHA));
        assert_eq!(installer(&latest, "0.3.0", "windows", "x86_64", "-setup.exe"), None);
        assert_eq!(installer(&latest, "0.2.0", "windows", "aarch64", "-setup.exe"), None);
        // Without a size or a hash, or off the site, nothing is run.
        for broken in [
            good.replace(r#""size":4,"#, ""),
            good.replace(r#""size":4"#, r#""size":0"#),
            good.replace(&SHA.to_uppercase(), "00"),
            good.replace("https://washoi.net/", "https://evil.example/"),
        ] {
            assert_eq!(
                installer(&with_installer(&broken), "0.2.0", "windows", "x86_64", "-setup.exe"),
                None,
                "{broken}"
            );
        }
    }

    #[test]
    fn each_target_takes_its_own_package() {
        let file = |platform: &str, name: &str| {
            format!(
                r#"{{"platform":"{platform}","name":"{name}","url":"https://washoi.net/client/0.3.0/{name}","size":4,"sha256":"{SHA}"}}"#
            )
        };
        let latest = with_installer(
            &[
                file("linux-x86_64", "washoi-0.3.0-linux-x86_64.tar.gz"),
                file("linux-x86_64", "washoi-0.3.0-linux-x86_64.AppImage"),
                file("macos-aarch64", "washoi-0.3.0-macos-aarch64.zip"),
            ]
            .join(","),
        );
        let pick = |os: &str, arch: &str, target: Target| {
            installer(&latest, "0.2.0", os, arch, target.suffix()).map(|i| i.url)
        };
        assert_eq!(
            pick("linux", "x86_64", Target::AppImage(PathBuf::from("/x"))).as_deref(),
            Some("https://washoi.net/client/0.3.0/washoi-0.3.0-linux-x86_64.AppImage")
        );
        assert_eq!(
            pick("macos", "aarch64", Target::MacApp(PathBuf::from("/A/Washoi.app"))).as_deref(),
            Some("https://washoi.net/client/0.3.0/washoi-0.3.0-macos-aarch64.zip")
        );
        assert_eq!(pick("linux", "x86_64", Target::WindowsInstaller), None);
    }

    #[test]
    fn the_download_must_match_the_index() {
        let update = Installer {
            version: "0.3.0".into(),
            url: String::new(),
            size: 4,
            sha256: SHA.into(),
        };
        assert!(verify(b"test", &update).is_ok());
        assert!(verify(b"tess", &update).is_err());
        assert!(verify(b"tests", &update).is_err());
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
