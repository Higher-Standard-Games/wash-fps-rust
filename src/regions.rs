//! Where the game is hosted: washoi.net's `src/data/regions.ts`, which the
//! site's build publishes as `/regions.json` (`publishedRegions`), and the
//! ping its `/play` page measures (`src/hooks/use-region-ping.ts`).
//!
//! The list is read from the site so a new region reaches players without a
//! client release. The last list read is kept in the config directory, and
//! a copy of the site's list is built in for the first start without the
//! network.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde_json::Value;
use shagger::lobby::lobby_client::{LobbyClient, LobbyEvent};
use shagger::network::client::http_fetch::fetch;

/// Where the site publishes the list.
pub const REGIONS_URL: &str = "https://washoi.net/regions.json";
/// The list's format this client reads (`REGIONS_JSON_VERSION`).
pub const REGIONS_JSON_VERSION: u64 = 1;
/// The list last read from the site, and the region last launched.
const CACHE_FILE: &str = "regions.json";
const CHOICE_FILE: &str = "region";

/// `Region`.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub id: String,
    /// Short code set big on the card.
    pub code: String,
    pub name: String,
    /// The region's address, shown on the card.
    pub host: String,
    /// Where the marker sits on the map, degrees.
    pub lon: f64,
    pub lat: f64,
    /// The card sits below its pin rather than above.
    pub label_below: bool,
    /// The lobby's WebSocket address and the packages' base; `None` for a
    /// region that is not live yet.
    pub lobby_url: Option<String>,
    pub data_url: Option<String>,
}

impl Region {
    #[must_use]
    pub fn live(&self) -> bool {
        self.lobby_url.is_some() && self.data_url.is_some()
    }
}

/// The site's list as of this build, for a start that cannot reach it.
#[must_use]
pub fn builtin() -> Vec<Region> {
    let region = |id: &str, name: &str, lon: f64, lat: f64, label_below: bool| Region {
        id: id.to_owned(),
        code: id.to_uppercase(),
        name: name.to_owned(),
        host: format!("{id}.washoi.net"),
        lon,
        lat,
        label_below,
        lobby_url: Some(format!("wss://{id}.washoi.net/lobby")),
        data_url: Some(format!("https://{id}.washoi.net/data")),
    };
    vec![
        region("eu", "Europe", 10.0, 50.0, false),
        region("us", "North America", -92.0, 39.0, true),
    ]
}

/// A short lowercase name, safe as a file or command-line word.
fn is_region_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 16 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// `/regions.json` as the site writes it; `None` for another version or a
/// list with nothing usable. A region whose addresses are not `wss://…` and
/// `https://…` is shown as not live rather than trusted.
#[must_use]
pub fn parse_regions(text: &str) -> Option<Vec<Region>> {
    let doc: Value = serde_json::from_str(text).ok()?;
    if doc.get("version")?.as_u64()? != REGIONS_JSON_VERSION {
        return None;
    }
    let mut regions = Vec::new();
    for r in doc.get("regions")?.as_array()? {
        let s = |k: &str| r.get(k).and_then(Value::as_str).map(str::to_owned);
        let Some(id) = s("id").filter(|id| is_region_id(id)) else {
            continue;
        };
        if regions.iter().any(|known: &Region| known.id == id) {
            continue;
        }
        let pin = r.get("pin");
        let degrees = |k: &str| {
            pin.and_then(|p| p.get(k))
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
        };
        let (Some(lon), Some(lat)) = (degrees("lon"), degrees("lat")) else {
            continue;
        };
        let lobby_url = s("lobbyUrl").filter(|u| u.starts_with("wss://"));
        let data_url = s("dataUrl").filter(|u| u.starts_with("https://"));
        let live = lobby_url.is_some() && data_url.is_some();
        regions.push(Region {
            code: s("code").unwrap_or_else(|| id.to_uppercase()),
            name: s("name").unwrap_or_else(|| id.clone()),
            host: s("host").unwrap_or_default(),
            id,
            lon: lon.clamp(-180.0, 180.0),
            lat: lat.clamp(-90.0, 90.0),
            label_below: s("labelSide").as_deref() == Some("below"),
            lobby_url: lobby_url.filter(|_| live),
            data_url: data_url.filter(|_| live),
        });
    }
    regions.iter().any(Region::live).then_some(regions)
}

/// The list: the site's if it answers, else the one read last time, else
/// the built-in one. A list read from the site is kept for next time.
#[must_use]
pub fn load(config_dir: Option<&Path>, cancel: &AtomicBool) -> Vec<Region> {
    let quick = Duration::from_secs(5);
    if let Ok(bytes) = fetch(REGIONS_URL, quick, quick, cancel, &mut |_, _| {})
        && let Some(text) = String::from_utf8(bytes).ok()
        && let Some(regions) = parse_regions(&text)
    {
        if let Some(dir) = config_dir {
            let _ = std::fs::create_dir_all(dir);
            let _ = write_whole(&dir.join(CACHE_FILE), &text);
        }
        return regions;
    }
    cached(config_dir).unwrap_or_else(builtin)
}

/// The list read from the site last time.
#[must_use]
pub fn cached(config_dir: Option<&Path>) -> Option<Vec<Region>> {
    parse_regions(&std::fs::read_to_string(config_dir?.join(CACHE_FILE)).ok()?)
}

/// The region launched last time.
#[must_use]
pub fn saved_choice(config_dir: Option<&Path>) -> Option<String> {
    let id = std::fs::read_to_string(config_dir?.join(CHOICE_FILE)).ok()?;
    Some(id.trim().to_owned()).filter(|id| is_region_id(id))
}

/// Remembers the region launched, for the next start.
pub fn save_choice(config_dir: Option<&Path>, id: &str) {
    if let Some(dir) = config_dir
        && is_region_id(id)
    {
        let _ = std::fs::create_dir_all(dir);
        let _ = write_whole(&dir.join(CHOICE_FILE), id);
    }
}

fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    let mut part = path.as_os_str().to_os_string();
    part.push(".part");
    std::fs::write(&part, text)?;
    std::fs::rename(&part, path)
}

/// `use-region-ping`'s attempts and how long each may take.
pub const PING_ATTEMPTS: u32 = 3;
pub const PING_TIMEOUT: Duration = Duration::from_secs(4);

/// `probe`: the time to the lobby's WebSocket being open, in ms, or `None`.
///
/// A handshake costs more than one round trip (TCP, TLS, upgrade), so this
/// overstates in-game ping; it is a fair way to compare regions, which is all
/// it is for.
#[must_use]
pub fn probe(lobby_url: &str) -> Option<f64> {
    let started = Instant::now();
    let client = LobbyClient::connect(lobby_url);
    let deadline = started + PING_TIMEOUT;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match client.next_event(left) {
            Some(LobbyEvent::Connected) => {
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                client.disconnect();
                return Some(ms.round());
            }
            Some(LobbyEvent::Error(_) | LobbyEvent::Disconnected { .. }) | None => return None,
            Some(LobbyEvent::Message(_)) => {}
        }
    }
    None
}

/// Best of [`PING_ATTEMPTS`], one after another so the first connection's DNS
/// lookup does not count against it.
/// Stops between attempts once `cancel` is set (a region was chosen).
#[must_use]
pub fn ping(lobby_url: &str, cancel: &AtomicBool) -> Option<f64> {
    (0..PING_ATTEMPTS)
        .take_while(|_| !cancel.load(std::sync::atomic::Ordering::Relaxed))
        .filter_map(|_| probe(lobby_url))
        .min_by(f64::total_cmp)
}

/// `recommended`: the live region with the lowest ping, else the first live one.
#[must_use]
pub fn nearest(regions: &[Region], ping_of: impl Fn(&str) -> Option<f64>) -> Option<&Region> {
    regions
        .iter()
        .filter(|r| r.live())
        .filter_map(|r| ping_of(&r.id).map(|ms| (r, ms)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
        .or_else(|| regions.iter().find(|r| r.live()))
}

/// The world as the site's dot matrix (`worldDots.ts`): 2° cells from 78°N
/// and 170°W, `true` on land.
pub mod world_dots {
    pub const STEP: f64 = 2.0;
    pub const LAT_TOP: f64 = 78.0;
    pub const LON_LEFT: f64 = -170.0;
    const ROWS: &str = include_str!("../assets/world/world_dots.txt");

    /// The rows, top to bottom.
    pub fn rows() -> impl Iterator<Item = &'static [u8]> {
        ROWS.lines().map(str::as_bytes)
    }

    /// The grid's size in cells (columns, rows).
    #[must_use]
    pub fn size() -> (usize, usize) {
        (rows().next().map_or(0, <[u8]>::len), rows().count())
    }

    /// `toGrid`: a longitude and latitude in cells from the top left.
    #[must_use]
    pub fn to_grid(lon: f64, lat: f64) -> (f64, f64) {
        let l = if lon < LON_LEFT { lon + 360.0 } else { lon };
        ((l - LON_LEFT) / STEP, (LAT_TOP - lat) / STEP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = r#"{"version":1,"regions":[
        {"id":"eu","code":"EU","name":"Europe","host":"eu.washoi.net","pin":{"lon":10,"lat":50},
         "labelSide":"above","timeZones":["Europe/"],"playUrl":"https://eu.washoi.net/",
         "lobbyUrl":"wss://eu.washoi.net/lobby","dataUrl":"https://eu.washoi.net/data"},
        {"id":"asia","code":"AS","name":"Asia","host":"as.washoi.net","pin":{"lon":120,"lat":20},
         "labelSide":"below","timeZones":[],"playUrl":null,"lobbyUrl":null,"dataUrl":null},
        {"id":"../x","pin":{"lon":0,"lat":0}},
        {"id":"evil","pin":{"lon":0,"lat":0},"lobbyUrl":"ws://plain/lobby","dataUrl":"http://plain/data"}
    ]}"#;

    #[test]
    fn the_sites_list_reads_as_the_built_in_one() {
        let regions = parse_regions(SITE).unwrap();
        assert_eq!(regions[0], builtin()[0]);
        assert_eq!(
            regions.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["eu", "asia", "evil"]
        );
        // Not live yet, and addresses that are not secure: shown, not used.
        assert!(!regions[1].live() && regions[1].label_below);
        assert!(!regions[2].live());
        assert!(parse_regions(&SITE.replace("\"version\":1", "\"version\":2")).is_none());
        assert!(parse_regions(r#"{"version":1,"regions":[]}"#).is_none());
    }

    #[test]
    fn the_nearest_is_the_fastest_live_region() {
        let regions = builtin();
        let ping = |id: &str| match id {
            "eu" => Some(97.0),
            "us" => Some(35.0),
            _ => None,
        };
        assert_eq!(nearest(&regions, ping).unwrap().id, "us");
        assert_eq!(nearest(&regions, |_| None).unwrap().id, "eu");
    }

    #[test]
    fn the_choice_and_list_are_kept() {
        let dir = std::env::temp_dir().join(format!("washoi-regions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(saved_choice(Some(&dir)), None);
        save_choice(Some(&dir), "us");
        assert_eq!(saved_choice(Some(&dir)).as_deref(), Some("us"));
        save_choice(Some(&dir), "../no");
        assert_eq!(saved_choice(Some(&dir)).as_deref(), Some("us"));
        std::fs::write(dir.join(CACHE_FILE), SITE).unwrap();
        assert_eq!(cached(Some(&dir)).unwrap().len(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_map_matches_the_sites() {
        assert_eq!(world_dots::size(), (180, 68));
        let (x, y) = world_dots::to_grid(10.0, 50.0);
        assert_eq!((x, y), (90.0, 14.0));
        // Central Europe is land; the middle of the Atlantic is not.
        let land = |lon: f64, lat: f64| {
            let (x, y) = world_dots::to_grid(lon, lat);
            world_dots::rows().nth(y as usize).unwrap()[x as usize] == b'1'
        };
        assert!(land(10.0, 50.0));
        assert!(!land(-35.0, 40.0));
    }
}
