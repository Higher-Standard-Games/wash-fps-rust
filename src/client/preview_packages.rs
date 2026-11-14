//! The packages the UI's model previews read from (`WeaponPreview`,
//! `PlayerTeamPreview`): the browser fetches a model from the published
//! data on demand; here the data folder's packages are read the first time
//! one of their files is asked for, and only those holding it
//! (shagger's `ingest_pk3_dir_holding`), so the lobby reads the player
//! models' packages and not every map. No TypeScript source of its own.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use shagger::loader::pk3::node_pk3::ingest_pk3_dir_holding;
use shagger::loader::pk3::pk3_loader::{Pk3Loader, normalize_path};

static DATA_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

struct Packages {
    loader: Pk3Loader,
    /// Paths already looked for, found or not.
    looked: HashSet<String>,
}

static PACKAGES: Mutex<Option<Packages>> = Mutex::new(None);

/// Where the packages are (the game's data folder). A new folder starts
/// the reading again.
pub fn set_data_dir(dir: PathBuf) {
    let mut current = DATA_DIR.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if current.as_ref() != Some(&dir) {
        *current = Some(dir);
        *PACKAGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

/// A file from the packages, by its package path (`models/...`), reading
/// the packages that hold it the first time it is asked for.
#[must_use]
pub fn read(path: &str) -> Option<Vec<u8>> {
    let dir = DATA_DIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()?;
    let mut guard = PACKAGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let packages = guard.get_or_insert_with(|| Packages {
        loader: Pk3Loader::new(),
        looked: HashSet::new(),
    });
    let key = normalize_path(path);
    if !packages.loader.has_file(&key) && packages.looked.insert(key.clone()) {
        ingest_pk3_dir_holding(&mut packages.loader, &dir, std::slice::from_ref(&key));
    }
    packages.loader.get_file(&key)
}
