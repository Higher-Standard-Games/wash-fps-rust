//! The update check: washoi.net publishes the newest released game under
//! `/client/` with a `latest.json` index (wash-fps
//! `scripts/deployment/client-release.sh`). The menu reads it once per run,
//! and when a newer version is out it offers the package for this machine.
//!
//! A game that can replace itself does so before the data sync instead
//! (`self_update`, `apply`): it downloads the newer package, checks its size
//! and SHA-256 against the index and the release workflow's Ed25519
//! signature on both ([`UPDATE_KEYS`]), and puts it in place. The signature
//! is what proves the package is ours: the index and the files come from the
//! same server, so a hash alone only proves they agree. On Windows the
//! installer it ran silently does that; an AppImage replaces its own file;
//! a `Washoi.app` swaps its bundle. Then the new game starts.
//!
//! No TypeScript source: the browser game is always current.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use base64::Engine as _;
use ring::signature::{ED25519, UnparsedPublicKey};
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
/// The release workflow's update-signing public keys (raw Ed25519, 32
/// bytes): the secret `UPDATE_SIGNING_KEY` signs each package's
/// [`signed_message`]. More than one while a key is being replaced.
pub const UPDATE_KEYS: &[[u8; 32]] = &[UPDATE_KEY];
/// `myfRIxIWo37fdl4SZ9Qt8FPaP+2J3pOXwUp+lvR7sjs=`, the 2026-11 release key
/// (`openssl pkey -pubout -outform DER | tail -c 32 | base64`).
const UPDATE_KEY: [u8; 32] = [
    0x9b, 0x27, 0xd1, 0x23, 0x12, 0x16, 0xa3, 0x7e, 0xdf, 0x76, 0x5e, 0x12, 0x67, 0xd4, 0x2d, 0xf0, 0x53, 0xda, 0x3f,
    0xed, 0x89, 0xde, 0x93, 0x97, 0xc1, 0x4a, 0x7e, 0x96, 0xf4, 0x7b, 0xb2, 0x3b,
];

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
        *state = State::Fetching(HttpFetch::start_at_most(
            LATEST_URL,
            Duration::from_secs(10),
            Duration::from_secs(20),
            Some(MAX_INDEX as u64),
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
    /// The Ed25519 signature on [`signed_message`], base64.
    pub sig: Option<String>,
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
                        sig: text("sig"),
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
    /// As the package's file name, which carries its version.
    pub name: String,
    pub signature: [u8; 64],
}

/// What the release workflow signs for each package: its name (and so its
/// version), size and SHA-256, so a signed older package cannot stand in for
/// a newer one.
#[must_use]
pub fn signed_message(name: &str, size: u64, sha256: &str) -> String {
    format!("washoi-update-v1\n{name}\n{size}\n{sha256}\n")
}

/// The package for this machine ending in `suffix` when `latest` is newer;
/// `None` without one, or without the size, hash and signature to check it
/// by, or when its name is not the version's package at its address.
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
    if !file.name.starts_with(&format!("washoi-{}-", latest.version)) || !file.url.ends_with(&format!("/{}", file.name))
    {
        return None;
    }
    let size = file.size.filter(|s| *s > 0 && *s <= MAX_INSTALLER)?;
    let sha256 = file
        .sha256
        .as_deref()
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))?
        .to_ascii_lowercase();
    let signature = base64::engine::general_purpose::STANDARD
        .decode(file.sig.as_deref()?)
        .ok()
        .and_then(|b| <[u8; 64]>::try_from(b).ok())?;
    Some(Installer {
        version: latest.version.clone(),
        url: file.url.clone(),
        size,
        sha256,
        name: file.name.clone(),
        signature,
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
    verify(&body, &update, UPDATE_KEYS)?;
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

/// The download is the file the site recorded, and one of `keys` signed it.
fn verify(body: &[u8], update: &Installer, keys: &[[u8; 32]]) -> Result<(), String> {
    if body.len() as u64 != update.size {
        return Err(format!("update: {} bytes, not {}", body.len(), update.size));
    }
    let hash: String = Sha256::digest(body).iter().map(|b| format!("{b:02x}")).collect();
    if hash != update.sha256 {
        return Err("update: the download does not match its SHA-256".into());
    }
    let message = signed_message(&update.name, update.size, &update.sha256);
    if !keys.iter().any(|key| {
        UnparsedPublicKey::new(&ED25519, key)
            .verify(message.as_bytes(), &update.signature)
            .is_ok()
    }) {
        return Err("update: the download is not signed by the release key".into());
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

/// `1.2.3` or `1.2.3-rc.1`: the numbers, and the pre-release identifiers
/// (`["rc", "1"]`; none for a release). Build metadata (`+...`) is ignored.
fn parse_version(v: &str) -> Option<([u64; 3], Vec<&str>)> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let v = v.split_once('+').map_or(v, |(v, _)| v);
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) => (core, pre.split('.').collect::<Vec<_>>()),
        None => (v, Vec::new()),
    };
    if pre
        .iter()
        .any(|id| id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    {
        return None;
    }
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

/// Semantic Versioning's order of two pre-releases of one version: by
/// identifier, numbers by value and below words, and more identifiers later
/// (`rc.1` < `rc.2` < `rc.10`, `beta.2` < `rc.1`).
fn pre_release_order(a: &[&str], b: &[&str]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let number = |id: &str| {
        id.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| id.parse::<u64>().ok())
            .flatten()
    };
    for (x, y) in a.iter().zip(b) {
        let order = match (number(x), number(y)) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

/// Whether `latest` is a later version than `current` (Semantic
/// Versioning's order): a pre-release comes before its release, `rc.2`
/// after `rc.1`, and an unreadable version is never newer.
#[must_use]
pub fn newer(latest: &str, current: &str) -> bool {
    let (Some((l, l_pre)), Some((c, c_pre))) = (parse_version(latest), parse_version(current)) else {
        return false;
    };
    if l != c {
        return l > c;
    }
    match (l_pre.is_empty(), c_pre.is_empty()) {
        (true, true) | (false, true) => false,
        (true, false) => true,
        (false, false) => pre_release_order(&l_pre, &c_pre).is_gt(),
    }
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
    /// Any 64 bytes, base64: what the index needs to carry a signature.
    const SIG: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

    /// A throwaway signing key and its public half.
    fn test_key() -> (ring::signature::Ed25519KeyPair, [u8; 32]) {
        use ring::signature::KeyPair as _;
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = <[u8; 32]>::try_from(pair.public_key().as_ref()).unwrap();
        (pair, public)
    }

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
    fn release_candidates_follow_each_other() {
        assert!(newer("1.0.0-rc.2", "1.0.0-rc.1"));
        assert!(newer("1.0.0-rc.10", "1.0.0-rc.2"));
        assert!(newer("1.0.0-rc.1", "1.0.0-beta.13"));
        assert!(newer("1.0.0-rc.1", "0.6.1"));
        assert!(newer("1.0.0-rc.1.1", "1.0.0-rc.1"));
        assert!(newer("1.0.0-rc", "1.0.0-1"));
        assert!(newer("1.0.0", "1.0.0-rc.2"));
        assert!(newer("1.0.0+build.5", "1.0.0-rc.2"));
        assert!(!newer("1.0.0-rc.1", "1.0.0-rc.1"));
        assert!(!newer("1.0.0-rc.1", "1.0.0-rc.2"));
        assert!(!newer("1.0.0-rc.2", "1.0.0"));
        assert!(!newer("1.0.0+build.6", "1.0.0+build.5"));
        assert!(!newer("1.0.0-rc..1", "0.1.0"));
        assert!(!newer("1.0.0-", "0.1.0"));
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
               "url":"https://washoi.net/client/0.3.0/washoi-0.3.0-windows-x86_64-setup.exe","size":4,"sha256":"{}","sig":"{SIG}"}}"#,
            SHA.to_uppercase()
        );
        let latest = with_installer(&format!(
            r#"{{"platform":"windows-x86_64","name":"washoi-0.3.0-windows-x86_64.zip",
               "url":"https://washoi.net/client/0.3.0/washoi-0.3.0-windows-x86_64.zip","size":4,"sha256":"{SHA}","sig":"{SIG}"}},{good}"#
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
            good.replace(&format!(r#","sig":"{SIG}""#), ""),
            good.replace(SIG, "AAAA"),
            // An older version's package, or another file at its address.
            good.replace(
                "washoi-0.3.0-windows-x86_64-setup.exe\",",
                "washoi-0.2.5-windows-x86_64-setup.exe\",",
            ),
            good.replace("0.3.0/washoi-0.3.0-windows", "0.3.0/old-washoi-0.3.0-windows"),
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
                r#"{{"platform":"{platform}","name":"{name}","url":"https://washoi.net/client/0.3.0/{name}","size":4,"sha256":"{SHA}","sig":"{SIG}"}}"#
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
    fn the_download_must_match_the_index_and_its_signature() {
        let (pair, public) = test_key();
        let (_, other) = test_key();
        let name = "washoi-0.3.0-windows-x86_64-setup.exe";
        let sign = |message: &str| <[u8; 64]>::try_from(pair.sign(message.as_bytes()).as_ref()).unwrap();
        let mut update = Installer {
            version: "0.3.0".into(),
            url: String::new(),
            size: 4,
            sha256: SHA.into(),
            name: name.into(),
            signature: sign(&signed_message(name, 4, SHA)),
        };
        assert!(verify(b"test", &update, &[public]).is_ok());
        assert!(verify(b"test", &update, &[other, public]).is_ok());
        assert!(verify(b"tess", &update, &[public]).is_err());
        assert!(verify(b"tests", &update, &[public]).is_err());
        assert!(verify(b"test", &update, &[other]).is_err());
        assert!(verify(b"test", &update, &[]).is_err());
        // A signature on an older package's name does not carry over.
        update.signature = sign(&signed_message("washoi-0.2.5-windows-x86_64-setup.exe", 4, SHA));
        assert!(verify(b"test", &update, &[public]).is_err());
    }

    #[test]
    fn a_release_key_is_built_in() {
        assert!(!UPDATE_KEYS.is_empty());
        assert!(
            UPDATE_KEYS.iter().all(|key| key != &[0; 32]),
            "UPDATE_KEY is still the placeholder"
        );
        let written = base64::engine::general_purpose::STANDARD
            .decode("myfRIxIWo37fdl4SZ9Qt8FPaP+2J3pOXwUp+lvR7sjs=")
            .unwrap();
        assert_eq!(written, UPDATE_KEY);
    }

    #[test]
    fn the_signed_message_is_what_the_release_workflow_signs() {
        // .github/workflows/release.yml: printf 'washoi-update-v1\n%s\n%s\n%s\n'
        assert_eq!(
            signed_message("washoi-1.0.0-linux-x86_64.AppImage", 12, "ab"),
            "washoi-update-v1\nwashoi-1.0.0-linux-x86_64.AppImage\n12\nab\n"
        );
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
