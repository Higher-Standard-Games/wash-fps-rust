//! The maps' levelshots (`/data/levelshots/<map>.jpg`, as the browser's),
//! fetched from the region's site and kept in the cache: the loading
//! screen's backdrop and the room's map cards. No TypeScript source: the
//! browser loads them as CSS backgrounds.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use shagger::network::client::http_fetch::fetch_at_most;
use shagger::ui::egui_layer::egui;

const LEVELSHOT_IDLE: Duration = Duration::from_secs(5);
const LEVELSHOT_TOTAL: Duration = Duration::from_secs(10);

/// `https://eu.washoi.net/data/levelshots` for `wss://eu.washoi.net/lobby`.
#[must_use]
pub fn levelshot_base(lobby_url: &str) -> String {
    format!("{}/data/levelshots", site_base(lobby_url))
}

/// `https://eu.washoi.net` for `wss://eu.washoi.net/lobby`: the site the
/// browser's game is served from, which invites open and levelshots come from.
#[must_use]
pub fn site_base(lobby_url: &str) -> String {
    let (scheme, rest) = match lobby_url.split_once("://") {
        Some(("wss" | "https", rest)) => ("https", rest),
        Some((_, rest)) => ("http", rest),
        None => ("http", lobby_url),
    };
    let host = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{host}")
}

/// A map name that is safe in a URL and a file name.
pub fn safe_map(map: &str) -> bool {
    !map.is_empty() && map.len() <= 64 && map.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn levelshot_cache(map: &str) -> Option<PathBuf> {
    crate::cache_dir().map(|d| d.join("levelshots").join(format!("{map}.jpg")))
}

/// A levelshot's bytes as an image.
#[must_use]
pub fn decode_levelshot(bytes: &[u8]) -> Option<egui::ColorImage> {
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
}

/// The levelshot: from the cache, or fetched and then kept. Reading,
/// fetching and decoding run on a thread of their own, so a menu that asks
/// for many maps at once does not hold up a frame.
pub enum Levelshot {
    None,
    Loading(mpsc::Receiver<Option<egui::ColorImage>>),
    Ready(Arc<egui::ColorImage>),
}

/// A levelshot is a JPEG of a few hundred kilobytes; a body past this is not one.
const MAX_LEVELSHOT: u64 = 8 * 1024 * 1024;

impl Levelshot {
    pub fn start(base: Option<&str>, map: &str) -> Self {
        if !safe_map(map) {
            return Self::None;
        }
        let map = map.to_owned();
        let url = base.map(|base| format!("{base}/{map}.jpg"));
        let (send, receive) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name(format!("levelshot {map}"))
            .spawn(move || {
                let _ = send.send(load(&map, url.as_deref()));
            });
        match spawned {
            Ok(_) => Self::Loading(receive),
            Err(_) => Self::None,
        }
    }

    pub fn poll(&mut self) {
        let Self::Loading(receive) = self else { return };
        *self = match receive.try_recv() {
            Ok(Some(image)) => Self::Ready(Arc::new(image)),
            // A map with no levelshot keeps the stage's own gradient.
            Ok(None) | Err(mpsc::TryRecvError::Disconnected) => Self::None,
            Err(mpsc::TryRecvError::Empty) => return,
        };
    }
}

/// The levelshot thread: the cached file, or the site's, kept once it decodes.
fn load(map: &str, url: Option<&str>) -> Option<egui::ColorImage> {
    let cache = levelshot_cache(map);
    if let Some(image) = cache
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| decode_levelshot(&b))
    {
        return Some(image);
    }
    let never = AtomicBool::new(false);
    let bytes = fetch_at_most(
        url?,
        LEVELSHOT_IDLE,
        LEVELSHOT_TOTAL,
        Some(MAX_LEVELSHOT),
        &never,
        &mut |_, _| {},
    )
    .ok()?;
    let image = decode_levelshot(&bytes)?;
    if let Some(path) = cache {
        let _ = path.parent().map(std::fs::create_dir_all);
        let _ = shagger::core::atomic_file::write_atomic(&path, &bytes);
    }
    Some(image)
}

/// Every map's levelshot the lobby has asked for, as textures.
pub struct Levelshots {
    base: String,
    shots: HashMap<String, (Levelshot, Option<egui::TextureHandle>)>,
}

impl Levelshots {
    #[must_use]
    pub fn new(lobby_url: &str) -> Self {
        Self {
            base: levelshot_base(lobby_url),
            shots: HashMap::new(),
        }
    }

    /// Starts fetching the maps not asked for yet.
    pub fn want<'a>(&mut self, maps: impl IntoIterator<Item = &'a str>) {
        for map in maps {
            if !self.shots.contains_key(map) {
                let shot = Levelshot::start(Some(&self.base), map);
                self.shots.insert(map.to_owned(), (shot, None));
            }
        }
    }

    /// Moves the fetches on and uploads what arrived.
    pub fn poll(&mut self, ctx: &egui::Context) {
        for (map, (shot, texture)) in &mut self.shots {
            shot.poll();
            if texture.is_none()
                && let Levelshot::Ready(image) = shot
            {
                *texture = Some(ctx.load_texture(
                    format!("levelshot-{map}"),
                    (**image).clone(),
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
    }

    /// The map's texture and its size in pixels, once it is loaded.
    #[must_use]
    pub fn texture_sized(&self, map: &str) -> Option<(egui::TextureId, egui::Vec2)> {
        self.shots
            .get(map)
            .and_then(|(_, t)| t.as_ref())
            .map(|t| (t.id(), t.size_vec2()))
    }

    /// The map's texture, once it is loaded.
    #[must_use]
    pub fn texture(&self, map: &str) -> Option<egui::TextureId> {
        self.shots
            .get(map)
            .and_then(|(_, t)| t.as_ref())
            .map(egui::TextureHandle::id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levelshots_come_from_the_lobby_s_site() {
        assert_eq!(
            levelshot_base("wss://eu.washoi.net/lobby"),
            "https://eu.washoi.net/data/levelshots"
        );
        assert_eq!(
            levelshot_base("ws://localhost:9000"),
            "http://localhost:9000/data/levelshots"
        );
    }

    #[test]
    fn only_a_plain_map_name_is_fetched() {
        assert!(safe_map("washoi_dm2"));
        assert!(!safe_map("../etc/passwd"));
        assert!(!safe_map("a b"));
        assert!(!safe_map(""));
    }
}
