//! The maps' levelshots (`/data/levelshots/<map>.jpg`, as the browser's),
//! fetched from the region's site and kept in the cache: the loading
//! screen's backdrop and the room's map cards. No TypeScript source: the
//! browser loads them as CSS backgrounds.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use shagger::network::client::http_fetch::{FetchEvent, HttpFetch};
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

/// The levelshot: from the cache, or fetched and then kept.
pub enum Levelshot {
    None,
    Fetching(String, HttpFetch),
    Ready(Arc<egui::ColorImage>),
}

impl Levelshot {
    pub fn start(base: Option<&str>, map: &str) -> Self {
        if !safe_map(map) {
            return Self::None;
        }
        if let Some(image) = levelshot_cache(map)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| decode_levelshot(&b))
        {
            return Self::Ready(Arc::new(image));
        }
        match base {
            Some(base) => Self::Fetching(
                map.to_owned(),
                HttpFetch::start(&format!("{base}/{map}.jpg"), LEVELSHOT_IDLE, LEVELSHOT_TOTAL),
            ),
            None => Self::None,
        }
    }

    pub fn poll(&mut self) {
        let Self::Fetching(map, fetch) = self else { return };
        while let Some(event) = fetch.poll() {
            match event {
                FetchEvent::Progress { .. } => {}
                FetchEvent::Done(bytes) => {
                    let Some(image) = decode_levelshot(&bytes) else {
                        *self = Self::None;
                        return;
                    };
                    if let Some(path) = levelshot_cache(map) {
                        let _ = path.parent().map(std::fs::create_dir_all);
                        let _ = std::fs::write(path, &bytes);
                    }
                    *self = Self::Ready(Arc::new(image));
                    return;
                }
                // A map with no levelshot keeps the stage's own gradient.
                FetchEvent::Failed(_) => {
                    *self = Self::None;
                    return;
                }
            }
        }
    }
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
