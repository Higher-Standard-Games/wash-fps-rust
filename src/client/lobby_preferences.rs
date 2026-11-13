//! wash-fps's `writePreference` and the `localStorage` reads beside it
//! (`engine/core/PreferenceStorage.ts`), for the lobby's own choices: the
//! terms agreed to and whether the welcome guide opens by itself.
//!
//! The browser keeps these in local storage; here they live in
//! `preferences.json` beside the bindings, one string per key. Not
//! mirrored with the web lobby's storage: the web view asks again.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Where the file lives: beside the bindings (which `--cvars` can move),
/// else the config dir.
fn file() -> Option<PathBuf> {
    shagger::input::input_system::with_input(|i| i.storage().map(std::path::Path::to_path_buf))
        .or_else(crate::config_dir)
        .map(|d| d.join("preferences.json"))
}

fn load() -> BTreeMap<String, String> {
    file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| match v {
            serde_json::Value::Object(map) => Some(
                map.into_iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// `localStorage.getItem`.
#[must_use]
pub fn read_preference(key: &str) -> Option<String> {
    load().remove(key)
}

/// `writePreference`: set or (with `None`) remove one; false when it could
/// not be saved.
pub fn write_preference(key: &str, value: Option<&str>) -> bool {
    let Some(file) = file() else { return false };
    let mut all = load();
    match value {
        Some(v) => all.insert(key.to_owned(), v.to_owned()),
        None => all.remove(key),
    };
    let map: serde_json::Map<String, serde_json::Value> = all
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    if let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_err()
    {
        return false;
    }
    let text = serde_json::Value::Object(map).to_string();
    shagger::core::atomic_file::write_atomic(&file, text.as_bytes()).is_ok()
}
