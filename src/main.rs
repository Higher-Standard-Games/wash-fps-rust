//! The native client binary. For now the headless client of milestone 2
//! (RUST.md 15): it logs in to the lobby, finds or queues for a match, joins
//! the game server, acknowledges snapshots, keeps the clock in sync and
//! prints the scoreboard.
//!
//! ```text
//! washoi view <map> [--data DIR] [--spawn N] [--screenshot FILE.png] [--size WxH] [--time T] [--at X,Y,Z,YAW] [--pitch DEG] [--player X,Y,Z,YAW[,CLIP[,FRAME[,quad][,WEAPON]]]]... [--projectile ID,X,Y,Z,YAW[,PITCH]]... [--explosion ID,X,Y,Z,AGE]... [--tracer X,Y,Z,X2,Y2,Z2,R,G,B,AGE]... [--muzzle X,Y,Z,R,G,B,STYLE,SCALE,DURATION,AGE]... [--impact X,Y,Z,NX,NY,NZ,IX,IY,IZ,R,G,B,HEAVY,AGE]... [--rail X,Y,Z,X2,Y2,Z2,R,G,B,KIND,NX,NY,NZ,AGE]... [--lightning MODE,X,Y,Z,X2,Y2,Z2,R,G,B,KIND,AGE]... [--hit X,Y,Z,DAMAGE,AGE[,head]]... [--teleport X,Y,Z,X2,Y2,Z2,YAW,AGE]... [--gib X,Y,Z,AGE]... [--ring X,Y,Z,KIND,RADIUS,ALPHA]... [--ragdoll X,Y,Z,YAW,DX,DY,DZ,AGE]... [--weapon ID [--aim] [--quad]] [--no-cull] [--no-lights] [--seconds N]
//! washoi demo <file.shagdemo> [--data DIR] [--screenshot FILE.png] [--size WxH] [--time T] [--at X,Y,Z,YAW] [--pitch DEG] [--follow N] [--reel MINUTES] [--audio-out FILE.wav] [--scoreboard] [--seconds N]
//! washoi watch <ws://game-server> [--name NAME] [--ticket TICKET] [--data DIR] [--screenshot FILE.png] [--time T] [--at X,Y,Z,YAW] [--follow N] [--seconds N]
//! washoi play <ws://game-server> [--name NAME] [--ticket TICKET] [--data DIR] [--screenshot FILE.png] [--time T] [--say TEXT] [--seconds N]
//! washoi connect <ws://game-server> [--name NAME] [--ticket TICKET] [--seconds N] [--play]
//! washoi menu <ws://lobby> [--key-file FILE] [--cvars FILE] [--screenshot FILE.png [--quick | --tab PAGE | --settings PAGE | --console "CMD; CMD"] [--seconds N] [--size WxH]]
//! washoi lobby <ws://lobby> [--key-file FILE] list
//! washoi lobby <ws://lobby> [--key-file FILE] spectate [--window] [--seconds N]
//! washoi lobby <ws://lobby> [--key-file FILE] queue <gametype> [--seconds N] [--play | --window]
//! washoi lobby <ws://lobby> [--key-file FILE] bot [gametype] [difficulty] [--seconds N] [--play | --window]
//! ```
//!
//! `--key-file` keeps the account key: it is read when the file exists, and a
//! new account's key (sent once, in `welcome`) is written to it. Without it
//! every login makes a new account. `--play` joins the match and runs forward
//! in a slow circle, so the command path is exercised too.
//!
//! `demo` plays a recorded match on its map in the window (or, with
//! `--screenshot`, draws it `--time` seconds in): the players move,
//! animate and turn as recorded.
//!
//! `--reel MINUTES` plays the demo's highlight reel of about that length
//! instead: its shots, slow motion, fly-bys, fades and cards. In the window,
//! Space pauses, Left and Right step through the shots and R replays it.
//!
//! `--follow N` puts the camera behind the Nth player (by id); in the
//! window F cycles through the players and back to the free camera. The
//! window plays the match's sound on the sound card; with `--screenshot`,
//! `--audio-out` mixes it offline into a WAV, heard from the camera.
//!
//! `watch` spectates a live match the same way: the window opens on the
//! server's map and draws its players and projectiles as snapshots arrive.
//!
//! `view` draws a map with the renderer (`view.rs`), from the
//! packages in `--data` (default `target/washoi-maps/pk3`, which
//! `tools/fetch_maps.sh` fills). `--time` steps shader animation that many
//! seconds before a screenshot; `--pitch` tilts the view (degrees, Quake's
//! sign: negative looks up); `--no-cull` draws every world batch instead of
//! leaving out what the PVS and the frustum hide. Each `--player` stands a
//! player model at a Quake origin and angle, playing a clip (idle by
//! default) held at a frame (0 by default), drawn in the FFA style (with
//! `quad`, glowing as a Quad Damage carrier; with a weapon id, holding that
//! weapon in the right hand). Each
//! `--projectile` holds a projectile (`rocket`, `grenade`, `plasma_bolt`,
//! `blade_blast`) at a Quake origin, flying along a Quake yaw and pitch.
//! Each `--explosion` holds that projectile's explosion at a Quake origin,
//! `AGE` seconds after it went off against a floor. `--tracer` holds a
//! bullet streak from one Quake point to another, `--muzzle` a muzzle flash
//! (`burst`, `plume` or `arcs`), and `--impact` an impact's sparks at a
//! point, surface normal and shot direction (`HEAVY` 1 adds the hit star),
//! and `--rail` an Electrobolt rail from the muzzle to a hit (`KIND` `none`,
//! `world` or `player`; a zero normal faces the shooter), each `AGE` seconds
//! in. `--lightning` holds a Voltaic beam: `hero` (the local player's, held
//! on), `remote` (another player's) or `chain` (an arc between chain
//! lightning targets). `--hit` holds a hit the player dealt to a victim at
//! a Quake origin, `AGE` seconds in: its damage number (orange with `head`)
//! and, from 75 damage, its cartoon sprite. `--teleport` holds a player's
//! teleport from one Quake origin to another (ghosts facing `YAW` degrees
//! and the bursts). `--gib` holds a body broken into shards at a Quake
//! origin, `AGE` seconds on. `--ragdoll` holds a body fallen as a ragdoll:
//! idle at a Quake origin facing `YAW`, launched along `DX,DY,DZ` (Quake
//! axes), `AGE` seconds on. `--weapon` holds a first-person weapon (a definition
//! id such as `plasmagun`), at the hip or, with `--aim`, down its sights; `--quad` puts the quad
//! outline on it.

mod menu;
mod reel_overlay;
mod view;
mod window;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use shagger::lobby::lobby_client::{LobbyClient, LobbyEvent};
use shagger::lobby::types::{LobbyConnect, LobbyIn};
use shagger::network::client::hosts::network_game_host::HostEvent;
use shagger::network::client::network_session::{NetworkSession, SessionConfig, SessionEvent, SessionRequest};
use shagger::shared::js::{js_cos, js_sin};
use shagger::shared::json::Value;
use shagger::shared::net::command::Command;
use shagger::shared::server::simulation::types::Vec3;
use shagger::weapon::muzzle::muzzle_flash_renderer::MuzzleFlashStyle;
use shagger::weapon::rail::rail_profiles::RailImpactKind;
use washoi::client::prediction;
use washoi::client::v2_network_client::{ClientInput, V2NetworkClient};
use washoi::shared::entities::definitions::weapons::weapons_table::weapon_speed_bonus;
use washoi::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

fn usage() -> ! {
    eprintln!(
        "washoi {}\n\
         usage: washoi view <map> [--data DIR] [--spawn N] [--screenshot FILE.png] [--size WxH] [--time T] [--at X,Y,Z,YAW] [--pitch DEG] [--player X,Y,Z,YAW[,CLIP[,FRAME[,quad][,WEAPON]]]]... [--projectile ID,X,Y,Z,YAW[,PITCH]]... [--explosion ID,X,Y,Z,AGE]... [--weapon ID [--aim] [--quad]] [--no-cull] [--no-lights] [--seconds N]\n\
         \x20      washoi demo <file.shagdemo> [--data DIR] [--screenshot FILE.png] [--size WxH] [--time T] [--at X,Y,Z,YAW] [--pitch DEG] [--follow N] [--reel MINUTES] [--audio-out FILE.wav] [--scoreboard] [--seconds N]\n\
         \x20      washoi watch <ws://game-server> [--name NAME] [--ticket TICKET] [--data DIR] [--screenshot FILE.png] [--time T] [--at X,Y,Z,YAW] [--follow N] [--seconds N]\n\
         \x20      washoi play <ws://game-server> [--name NAME] [--ticket TICKET] [--data DIR] [--screenshot FILE.png] [--time T] [--say TEXT] [--seconds N]\n\
         \x20      washoi connect <ws://game-server> [--name NAME] [--ticket TICKET] [--seconds N] [--play]\n\
         \x20      washoi menu <ws://lobby> [--key-file FILE] [--cvars FILE] [--screenshot FILE.png [--quick | --tab PAGE | --settings PAGE | --console \"CMD; CMD\"] [--seconds N] [--size WxH]]\n\
         \x20      washoi lobby <ws://lobby> [--key-file FILE] list\n\
         \x20      washoi lobby <ws://lobby> [--key-file FILE] spectate [--window] [--seconds N]\n\
         \x20      washoi lobby <ws://lobby> [--key-file FILE] queue <gametype> [--seconds N] [--play | --window]\n\
         \x20      washoi lobby <ws://lobby> [--key-file FILE] bot [gametype] [difficulty] [--seconds N] [--play | --window]",
        washoi::VERSION
    );
    std::process::exit(2);
}

/// Options shared by every mode.
#[derive(Default)]
struct Options {
    name: Option<String>,
    ticket: Option<String>,
    seconds: Option<f64>,
    play: bool,
    key_file: Option<PathBuf>,
    data: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    spawn: usize,
    size: Option<(u32, u32)>,
    time: f64,
    at: Option<[f32; 4]>,
    pitch: f32,
    no_cull: bool,
    no_lights: bool,
    weapon: Option<String>,
    aim: bool,
    quad: bool,
    follow: Option<usize>,
    reel: Option<f64>,
    audio_out: Option<PathBuf>,
    scoreboard: bool,
    say: Option<String>,
    /// From the lobby into the window rather than headless (`--window`).
    window: bool,
    /// The menu's screenshot presses Quick play (`--quick`).
    quick: bool,
    /// The menu's screenshot opens this settings page (`--settings`).
    settings: Option<String>,
    /// The menu's screenshot opens this rail page (`--tab`).
    tab: Option<String>,
    /// The menu's screenshot opens the console and runs these (`--console`).
    console: Option<String>,
    /// The cvar file instead of `~/.config/washoi/cvars.json` (`--cvars`).
    cvars: Option<PathBuf>,
    players: Vec<view::PlayerView>,
    projectiles: Vec<view::ProjectileView>,
    explosions: Vec<view::ExplosionView>,
    effects: Vec<view::EffectView>,
    hits: Vec<view::HitView>,
    teleports: Vec<view::TeleportView>,
    gibs: Vec<view::GibView>,
    rings: Vec<view::RingView>,
    ragdolls: Vec<view::RagdollView>,
    words: Vec<String>,
}

fn parse_options(args: impl Iterator<Item = String>) -> Options {
    let mut o = Options::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match arg.as_str() {
            "--name" => o.name = Some(value()),
            "--ticket" => o.ticket = Some(value()),
            "--seconds" => o.seconds = Some(value().parse().unwrap_or_else(|_| usage())),
            "--key-file" => o.key_file = Some(PathBuf::from(value())),
            "--play" => o.play = true,
            "--data" => o.data = Some(PathBuf::from(value())),
            "--screenshot" => o.screenshot = Some(PathBuf::from(value())),
            "--at" => {
                let v: Vec<f32> = value().split(',').filter_map(|n| n.trim().parse().ok()).collect();
                o.at = Some(<[f32; 4]>::try_from(v).unwrap_or_else(|_| usage()));
            }
            "--no-cull" => o.no_cull = true,
            "--no-lights" => o.no_lights = true,
            "--weapon" => o.weapon = Some(value()),
            "--aim" => o.aim = true,
            "--quad" => o.quad = true,
            "--follow" => o.follow = Some(value().parse().unwrap_or_else(|_| usage())),
            "--reel" => o.reel = Some(value().parse().unwrap_or_else(|_| usage())),
            "--audio-out" => o.audio_out = Some(PathBuf::from(value())),
            "--scoreboard" => o.scoreboard = true,
            "--say" => o.say = Some(value()),
            "--window" => o.window = true,
            "--quick" => o.quick = true,
            "--settings" => o.settings = Some(value()),
            "--console" => o.console = Some(value()),
            "--tab" => o.tab = Some(value()),
            "--cvars" => o.cvars = Some(PathBuf::from(value())),
            "--player" => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| {
                    parts
                        .get(i)
                        .and_then(|p| p.parse::<f32>().ok())
                        .unwrap_or_else(|| usage())
                };
                o.players.push(view::PlayerView {
                    origin: [n(0), n(1), n(2)],
                    angle: n(3),
                    clip: parts.get(4).map_or_else(|| "idle".to_owned(), |c| (*c).to_owned()),
                    frame: parts.get(5).map_or(0.0, |f| f.parse().unwrap_or_else(|_| usage())),
                    quad: parts.get(6..).is_some_and(|rest| rest.contains(&"quad")),
                    weapon: parts
                        .get(6..)
                        .and_then(|rest| rest.iter().find(|p| **p != "quad"))
                        .map(|w| (*w).to_owned()),
                });
            }
            "--projectile" => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| {
                    parts
                        .get(i)
                        .and_then(|p| p.parse::<f32>().ok())
                        .unwrap_or_else(|| usage())
                };
                o.projectiles.push(view::ProjectileView {
                    id: parts.first().map_or_else(|| usage(), |s| (*s).to_owned()),
                    origin: [n(1), n(2), n(3)],
                    yaw: n(4),
                    pitch: parts.get(5).map_or(0.0, |p| p.parse().unwrap_or_else(|_| usage())),
                });
            }
            "--explosion" => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| {
                    parts
                        .get(i)
                        .and_then(|p| p.parse::<f32>().ok())
                        .unwrap_or_else(|| usage())
                };
                o.explosions.push(view::ExplosionView {
                    id: parts.first().map_or_else(|| usage(), |s| (*s).to_owned()),
                    origin: [n(1), n(2), n(3)],
                    age: parts.get(4).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()),
                });
            }
            "--ragdoll" => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| -> f64 { parts.get(i).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()) };
                o.ragdolls.push(view::RagdollView {
                    player: view::PlayerView {
                        origin: [n(0) as f32, n(1) as f32, n(2) as f32],
                        angle: n(3) as f32,
                        clip: "idle".to_owned(),
                        frame: 0.0,
                        quad: false,
                        weapon: None,
                    },
                    dir: [n(4), n(5), n(6)],
                    age: n(7),
                });
            }
            "--gib" => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| -> f64 { parts.get(i).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()) };
                o.gibs.push(view::GibView {
                    origin: [n(0), n(1), n(2)],
                    age: n(3),
                });
            }
            "--ring" => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| -> f64 { parts.get(i).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()) };
                let kind = match parts.get(3).copied() {
                    Some("water") => washoi::liquid::liquid_types::LiquidKind::Water,
                    Some("slime") => washoi::liquid::liquid_types::LiquidKind::Slime,
                    Some("lava") => washoi::liquid::liquid_types::LiquidKind::Lava,
                    _ => usage(),
                };
                o.rings.push(view::RingView {
                    origin: [n(0), n(1), n(2)],
                    kind,
                    radius: n(4),
                    alpha: n(5),
                });
            }
            "--teleport" => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| -> f64 { parts.get(i).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()) };
                o.teleports.push(view::TeleportView {
                    from: [n(0), n(1), n(2)],
                    to: [n(3), n(4), n(5)],
                    yaw: n(6).to_radians(),
                    age: n(7),
                });
            }
            "--hit" => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| -> f64 { parts.get(i).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()) };
                o.hits.push(view::HitView {
                    origin: [n(0), n(1), n(2)],
                    damage: parts.get(3).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()),
                    age: n(4),
                    head: parts.get(5) == Some(&"head"),
                });
            }
            flag @ ("--tracer" | "--muzzle" | "--impact" | "--rail" | "--lightning") => {
                let v = value();
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                let n = |i: usize| -> f64 { parts.get(i).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()) };
                let v3 = |i: usize| [n(i), n(i + 1), n(i + 2)];
                let c3 = |i: usize| [n(i) as f32, n(i + 1) as f32, n(i + 2) as f32];
                o.effects.push(match flag {
                    "--tracer" => view::EffectView::Tracer {
                        start: v3(0),
                        end: v3(3),
                        color: c3(6),
                        age: n(9),
                    },
                    "--muzzle" => view::EffectView::Muzzle {
                        position: v3(0),
                        color: c3(3),
                        style: parts
                            .get(6)
                            .and_then(|s| MuzzleFlashStyle::parse(s))
                            .unwrap_or_else(|| usage()),
                        scale: n(7),
                        duration: n(8),
                        age: n(9),
                    },
                    "--lightning" => {
                        let n = |i: usize| -> f64 {
                            parts.get(i + 1).and_then(|p| p.parse().ok()).unwrap_or_else(|| usage())
                        };
                        view::EffectView::Lightning {
                            mode: parts
                                .first()
                                .and_then(|s| view::LightningMode::parse(s))
                                .unwrap_or_else(|| usage()),
                            start: [n(0), n(1), n(2)],
                            end: [n(3), n(4), n(5)],
                            color: [n(6) as f32, n(7) as f32, n(8) as f32],
                            kind: parts
                                .get(10)
                                .and_then(|s| RailImpactKind::parse(s))
                                .unwrap_or_else(|| usage()),
                            age: n(10),
                        }
                    }
                    "--rail" => view::EffectView::Rail {
                        start: v3(0),
                        end: v3(3),
                        color: c3(6),
                        kind: parts
                            .get(9)
                            .and_then(|s| RailImpactKind::parse(s))
                            .unwrap_or_else(|| usage()),
                        normal: Some(v3(10)).filter(|n| n.iter().any(|c| *c != 0.0)),
                        age: n(13),
                    },
                    _ => view::EffectView::Impact {
                        position: v3(0),
                        normal: v3(3),
                        incoming: v3(6),
                        color: c3(9),
                        heavy: n(12) != 0.0,
                        age: n(13),
                    },
                });
            }
            "--pitch" => o.pitch = value().trim().parse().unwrap_or_else(|_| usage()),
            "--time" => o.time = value().parse().unwrap_or_else(|_| usage()),
            "--spawn" => o.spawn = value().parse().unwrap_or_else(|_| usage()),
            "--size" => {
                let v = value();
                let (w, h) = v.split_once('x').unwrap_or_else(|| usage());
                o.size = Some((
                    w.parse().unwrap_or_else(|_| usage()),
                    h.parse().unwrap_or_else(|_| usage()),
                ));
            }
            flag if flag.starts_with("--") => usage(),
            _ => o.words.push(arg),
        }
    }
    o
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| usage());
    let url = args.next().unwrap_or_else(|| usage());
    let o = parse_options(args);
    // The cvars, saved as the browser saves `engine_cvars`. Release builds
    // are the shipped game, which keeps `developer` at 0.
    let storage = o.cvars.clone().or_else(|| config_dir().map(|d| d.join("cvars.json")));
    // The bindings sit beside the cvars, in `synced-storage.json`.
    let input_dir = storage
        .as_ref()
        .and_then(|p| p.parent())
        .map(std::path::Path::to_path_buf);
    shagger::core::cvar::init_cvars(storage, !cfg!(debug_assertions));
    shagger::input::input_system::with_input(washoi::client::fps_actions::register_fps_actions);
    shagger::input::input_system::init_input(input_dir);
    shagger::core::cvar::with_cvars(washoi::client::settings::settings_model::register_client_cvars);
    let code = match mode.as_str() {
        "connect" => play_match(
            &url,
            o.name.as_deref().unwrap_or("RustClient"),
            o.ticket.clone(),
            o.seconds.unwrap_or(10.0),
            o.play,
            &data_dir(&o),
        ),
        "lobby" => lobby(&url, &o),
        "menu" => menu_mode(&url, &o),
        "view" | "demo" | "watch" | "play" => {
            let demo = (mode == "demo").then(|| PathBuf::from(&url));
            let watch = (mode == "watch" || mode == "play").then(|| {
                let fallback = if mode == "play" { "RustPlayer" } else { "RustSpectator" };
                (
                    url.clone(),
                    o.name.clone().unwrap_or_else(|| fallback.to_owned()),
                    o.ticket.clone(),
                )
            });
            view::run(&view_options(&o, url, demo, watch, mode == "play"))
        }
        _ => usage(),
    };
    std::process::exit(code);
}

/// The window's options from the command line.
/// Where the menu keeps the account key when `--key-file` does not say: the
/// browser keeps it in local storage, which outlives a session the same way.
fn default_key_file() -> Option<PathBuf> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("lobby-key"))
}

use washoi::config_dir;

fn menu_mode(url: &str, o: &Options) -> i32 {
    let key_file = o.key_file.clone().or_else(default_key_file);
    let go = |c: &washoi::client::lobby_panel::LobbyChoice| {
        if let Some(demo) = &c.demo {
            match c.reel {
                Some(minutes) => println!("lobby: demo {} as a {minutes} min reel", demo.display()),
                None => println!("lobby: demo {}", demo.display()),
            }
            return;
        }
        let how = if c.play { "play" } else { "watch" };
        println!("lobby: {how} at {} on {}", c.server_url, c.map);
    };
    if let Some(path) = &o.screenshot {
        let size = o.size.unwrap_or((1280, 720));
        let seconds = o.seconds.unwrap_or(5.0);
        let shot = menu::Shot {
            quick: o.quick,
            settings: o.settings.clone(),
            console: o.console.clone(),
            tab: o.tab.clone(),
            seconds,
            size,
        };
        return match menu::screenshot(url, key_file, &shot, path) {
            Ok(choice) => {
                if let Some(c) = &choice {
                    go(c);
                }
                0
            }
            Err(e) => {
                eprintln!("menu: {e}");
                1
            }
        };
    }
    loop {
        match menu::choose(url, key_file.clone()) {
            Ok(Some(c)) => {
                go(&c);
                if let Some(demo) = &c.demo {
                    let mut v = view_options(o, c.map.clone(), Some(demo.clone()), None, false);
                    v.reel = c.reel.or(o.reel);
                    view::run(&v);
                    continue;
                }
                let watch = Some((c.server_url.clone(), c.name.clone(), c.admission_ticket.clone()));
                view::run(&view_options(o, c.server_url.clone(), None, watch, c.play));
            }
            Ok(None) => return 0,
            Err(e) => {
                eprintln!("menu: {e}");
                return 1;
            }
        }
    }
}

fn view_options(
    o: &Options,
    map: String,
    demo: Option<PathBuf>,
    watch: Option<(String, String, Option<String>)>,
    join: bool,
) -> view::ViewOptions {
    let (width, height) = o.size.unwrap_or((1280, 720));
    view::ViewOptions {
        seconds: o.seconds,
        map,
        data: data_dir(o),
        screenshot: o.screenshot.clone(),
        spawn: o.spawn,
        width,
        height,
        time: o.time,
        at: o.at,
        pitch: o.pitch,
        no_cull: o.no_cull,
        no_lights: o.no_lights,
        players: o.players.clone(),
        projectiles: o.projectiles.clone(),
        explosions: o.explosions.clone(),
        effects: o.effects.clone(),
        hits: o.hits.clone(),
        teleports: o.teleports.clone(),
        gibs: o.gibs.clone(),
        rings: o.rings.clone(),
        ragdolls: o.ragdolls.clone(),
        weapon: o.weapon.clone(),
        aim: o.aim,
        quad: o.quad,
        demo,
        watch,
        follow: o.follow,
        reel: o.reel,
        audio_out: o.audio_out.clone(),
        scoreboard: o.scoreboard,
        join,
        say: o.say.clone(),
    }
}

// ── Lobby ──────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn lobby(url: &str, o: &Options) -> i32 {
    let key = o
        .key_file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|k| k.trim().to_owned())
        .filter(|k| !k.is_empty());
    let how = if key.is_some() {
        "with the stored key"
    } else {
        "as a new account"
    };
    println!("lobby {url}: logging in {how}");
    let client = LobbyClient::connect(url);
    let mut name = String::new();
    let mut lobby_id: Option<String> = None;
    let mut queued = false;
    let mut bot_match = false;
    let mut readied = false;
    let deadline = Instant::now() + Duration::from_secs(120);

    while Instant::now() < deadline {
        let Some(event) = client.next_event(Duration::from_millis(200)) else {
            continue;
        };
        match event {
            LobbyEvent::Connected => client.auth(key.as_deref()),
            LobbyEvent::Error(why) => {
                println!("lobby error: {why}");
                return 1;
            }
            LobbyEvent::Disconnected { reason, code } => {
                println!("lobby closed ({code}): {reason}");
                return 1;
            }
            LobbyEvent::Message(LobbyIn::AuthError(e)) => {
                println!("login refused: {}", e.code);
                return 1;
            }
            LobbyEvent::Message(LobbyIn::Welcome(w)) => {
                name = w
                    .identity
                    .as_ref()
                    .map_or_else(|| w.player_id.clone(), |i| i.name.clone());
                println!("logged in as {name}, {} online", w.online_players.len());
                if let (Some(new_key), Some(file)) = (&w.key, &o.key_file) {
                    match std::fs::write(file, new_key) {
                        Ok(()) => println!("new account: key saved to {}", file.display()),
                        Err(e) => println!("new account: could not save its key: {e}"),
                    }
                }
                let gametypes: Vec<&str> = w.gametypes.iter().map(|g| g.id.as_str()).collect();
                println!("gametypes: {}", gametypes.join(", "));
                for m in &w.live_matches {
                    println!(
                        "  match {} {} on {}: {}/{} players, {}{}",
                        m.id,
                        m.gametype_name,
                        if m.map.is_empty() { "?" } else { &m.map },
                        m.player_count,
                        m.max_players,
                        m.state,
                        m.server_url.as_deref().map(|u| format!(" at {u}")).unwrap_or_default()
                    );
                }
                match o.words.first().map(String::as_str) {
                    Some("list") => return 0,
                    Some("spectate") => {
                        let Some(server) = w
                            .live_matches
                            .iter()
                            .filter(|m| m.state == "playing")
                            .find_map(|m| m.server_url.clone())
                        else {
                            println!("no match is being played right now");
                            return 0;
                        };
                        client.disconnect();
                        if o.window {
                            let watch = Some((server.clone(), name.clone(), None));
                            return view::run(&view_options(o, server, None, watch, false));
                        }
                        return play_match(&server, &name, None, o.seconds.unwrap_or(15.0), false, &data_dir(o));
                    }
                    Some("bot") => {
                        let gametype = o.words.get(1).map_or("duel", String::as_str);
                        println!("creating a private {gametype} room against a bot");
                        client.create_lobby(gametype, true);
                        bot_match = true;
                    }
                    Some("queue") => {
                        let gametype = o.words.get(1).map_or("duel", String::as_str);
                        println!("queueing for {gametype}");
                        client.join_queue(gametype);
                        queued = true;
                    }
                    _ => usage(),
                }
            }
            LobbyEvent::Message(LobbyIn::RoomState(room)) if bot_match && lobby_id.is_none() => {
                let id = room.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
                println!("in room {id}; inviting a bot");
                client.invite_bot(&id, o.words.get(2).map(String::as_str));
                lobby_id = Some(id);
            }
            LobbyEvent::Message(LobbyIn::Other { kind, message }) if kind == "lobby:bot_invited" && !readied => {
                let bot = message.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let level = message.get("difficulty").and_then(|v| v.as_str()).unwrap_or("?");
                println!("bot {bot} ({level}) is joining; ready");
                if let Some(id) = &lobby_id {
                    client.set_ready(id, true);
                }
                readied = true;
            }
            LobbyEvent::Message(LobbyIn::Other { kind, message }) if kind == "lobby:bot_invite_failed" => {
                let why = message.get("error").and_then(|v| v.as_str()).unwrap_or("?");
                println!("bot invite failed: {why}");
                return 1;
            }
            LobbyEvent::Message(LobbyIn::RoomState(room)) if queued => {
                let id = room.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
                if lobby_id.as_deref() != Some(id.as_str()) {
                    println!("in room {id}; ready");
                    client.set_ready(&id, true);
                    lobby_id = Some(id);
                }
            }
            LobbyEvent::Message(LobbyIn::Other { kind, message }) if kind == "lobby:starting" => {
                let map = message.get("map").and_then(|v| v.as_str()).unwrap_or("?");
                println!("match starting on {map}");
            }
            LobbyEvent::Message(LobbyIn::Connect(LobbyConnect {
                server_url,
                map,
                gametype,
                admission_ticket,
                ..
            })) => {
                println!("lobby: go to {server_url} ({gametype} on {map})");
                // The browser keeps the lobby open during the match; this
                // client does not need it until the next one.
                client.disconnect();
                if o.window {
                    // Into the window, playing (or watching, for a spectator's seat).
                    let watch = Some((server_url.clone(), name.clone(), admission_ticket));
                    return view::run(&view_options(o, server_url, None, watch, true));
                }
                return play_match(
                    &server_url,
                    &name,
                    admission_ticket,
                    o.seconds.unwrap_or(15.0),
                    o.play,
                    &data_dir(o),
                );
            }
            LobbyEvent::Message(LobbyIn::LobbyError(why)) => println!("lobby: {why}"),
            LobbyEvent::Message(_) => {}
        }
    }
    println!("gave up waiting for the lobby");
    1
}

// ── The match ──────────────────────────────────────────────────────────────

fn n(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// The scoreboard. The server leaves the roster (`standings`) out of
/// `match_state` once this client has acknowledged it (`rosterOmitted`), so the
/// last one received stands until a new one arrives (RUST.md 18.1).
fn print_standings(match_state: &Value, standings: &[Value]) {
    let phase = match_state.get("phase").and_then(Value::as_f64).unwrap_or(-1.0);
    let gametype = match_state.get("gametypeName").and_then(Value::as_str).unwrap_or("?");
    let left = n(match_state.get("timeRemaining"));
    println!("  {gametype}, phase {phase}, {left:.0}s left");
    println!(
        "  {:<20} {:>6} {:>6} {:>6} {:>5}",
        "player", "score", "frags", "deaths", "ping"
    );
    for row in standings {
        println!(
            "  {:<20} {:>6} {:>6} {:>6} {:>5}",
            row.get("name").and_then(Value::as_str).unwrap_or("?"),
            n(row.get("score")),
            n(row.get("frags")),
            n(row.get("deaths")),
            n(row.get("ping")),
        );
    }
}

#[allow(clippy::too_many_lines)]
/// `--data`, or the packages `tools/fetch_maps.sh` fetched.
fn data_dir(o: &Options) -> PathBuf {
    o.data
        .clone()
        .unwrap_or_else(|| PathBuf::from("target/washoi-maps/pk3"))
}

fn play_match(url: &str, name: &str, ticket: Option<String>, seconds: f64, play: bool, data: &Path) -> i32 {
    println!("connecting to {url} as {name}");
    let session = NetworkSession::start(
        SessionConfig {
            url: url.to_owned(),
            player_name: name.to_owned(),
            admission_ticket: ticket,
        },
        WashoiEvents,
    );

    let start = Instant::now();
    let mut last_report = Instant::now();
    let mut snapshots = 0u32;
    let mut events = 0usize;
    let mut tick = 0u32;
    let mut entity_id = String::new();
    let mut match_state: Option<Value> = None;
    let mut standings: Vec<Value> = Vec::new();
    let mut position: Option<[f64; 3]> = None;
    let mut joined = false;
    let mut next_command = Instant::now();
    let mut yaw = 0.0f64;
    let mut exit = 0;
    let mut clock = (0.0, 0.0);
    // Prediction (milestone 4): the commands run through V2NetworkClient
    // on the map's collision, and each snapshot confirms or corrects it.
    let mut predictor: Option<V2NetworkClient> = None;
    let mut alive = false;
    let now_ms = || start.elapsed().as_secs_f64() * 1000.0;

    while start.elapsed().as_secs_f64() < seconds {
        for event in session.poll_events() {
            match event {
                SessionEvent::Host(HostEvent::ServerInfo(w)) => {
                    println!(
                        "welcome: \"{}\" ({}), map {}, {} Hz, {}/{} players",
                        w.server_name, w.server_id, w.map, w.tick_rate, w.current_players, w.max_players
                    );
                    if play {
                        let mut client = V2NetworkClient::new(f64::from(w.tick_rate));
                        match prediction::install_map(&mut client, data, &w.map) {
                            Ok(()) => println!("predicting on {}", w.map),
                            Err(why) => println!("predicting without collision: {why}"),
                        }
                        predictor = Some(client);
                    }
                }
                SessionEvent::Host(HostEvent::Joined {
                    player_id,
                    entity_id: e,
                }) => {
                    println!("joined as {player_id} (entity {e:?})");
                    entity_id = e;
                    if play && !joined {
                        session.send(SessionRequest::JoinGame {
                            team_id: None,
                            manual_pick: false,
                            admission_ticket: None,
                        });
                        joined = true;
                        next_command = Instant::now();
                    }
                }
                SessionEvent::Host(HostEvent::ResumeToken { grace_ms }) => {
                    println!("seat held for {grace_ms} ms after a drop");
                }
                SessionEvent::Host(HostEvent::Snapshot(state)) => {
                    snapshots += 1;
                    events += state.events.len();
                    tick = state.tick;
                    for event in &state.events {
                        if let SnapshotEvent::MatchState(v) = event {
                            if let Some(rows) = v.get("standings").and_then(Value::as_array) {
                                standings = rows.to_vec();
                            }
                            match_state = Some(v.clone());
                        }
                    }
                    if let Some(t) = state.entities.get(&entity_id).and_then(|e| e.transform) {
                        position = Some([t.position.x, t.position.y, t.position.z]);
                    }
                    if let Some(client) = &mut predictor {
                        // GameEvents' weapon sync: the held weapon's speed bonus.
                        for event in &state.events {
                            if let SnapshotEvent::WeaponState {
                                entity_id: id,
                                current_weapon,
                                ..
                            } = event
                                && *id == entity_id
                            {
                                client.set_speed_bonus(weapon_speed_bonus(current_weapon));
                            }
                        }
                        match prediction::server_snapshot(&state, &entity_id) {
                            Some(snapshot) => {
                                if !alive {
                                    // Back from the dead (or in for the first time).
                                    client.reset_for_respawn();
                                    alive = true;
                                }
                                client.buffer_snapshot(snapshot, now_ms());
                            }
                            None => alive = false,
                        }
                    }
                }
                SessionEvent::Host(HostEvent::ClockSync { offset, rtt }) => clock = (offset, rtt),
                SessionEvent::Host(HostEvent::ServerMessage(m)) => println!("server [{}]: {}", m.kind, m.message),
                SessionEvent::Host(HostEvent::Stall { stage, silent_ms }) => {
                    println!("no snapshot for {:.1}s ({stage:?})", silent_ms / 1000.0);
                }
                SessionEvent::Host(HostEvent::Recovered { stalled_ms }) => {
                    println!("snapshots resumed after {:.1}s", stalled_ms / 1000.0);
                }
                SessionEvent::Host(HostEvent::BadMessage(why)) => println!("bad message: {why}"),
                SessionEvent::Host(HostEvent::Failed(why)) => {
                    println!("failed: {why}");
                    exit = 1;
                }
                SessionEvent::Resuming { attempt } => println!("connection lost, resuming (attempt {attempt})"),
                SessionEvent::Closed(why) => {
                    println!("closed: {why}");
                    return if exit == 0 { 1 } else { exit };
                }
            }
        }

        if let Some(client) = &mut predictor {
            client.process_buffered_snapshots(now_ms());
        }
        // 125 commands a second: forward, turning slowly.
        while joined && Instant::now() >= next_command {
            next_command += Duration::from_millis(8);
            yaw += 0.004;
            let command = Command {
                direction: [js_sin(yaw), 0.0, js_cos(yaw)],
                pitch: 0.0,
                yaw,
                buttons: 1,
                tick,
                weapon_slot: 0,
            };
            if let Some(client) = &mut predictor {
                // Predict what the server simulates: the f32 wire values (19.6).
                let sent = command.as_sent();
                client.process_input(
                    ClientInput {
                        direction: Vec3::new(sent.direction[0], sent.direction[1], sent.direction[2]),
                        pitch: sent.pitch,
                        yaw: sent.yaw,
                        buttons: u32::from(sent.buttons),
                        client_tick: sent.tick,
                    },
                    now_ms(),
                );
            }
            session.send(SessionRequest::Command(command));
        }

        if last_report.elapsed() >= Duration::from_secs(2) {
            let secs = last_report.elapsed().as_secs_f64();
            println!(
                "tick {tick}: {:.1} snapshots/s, {events} events, clock offset {:.1} ms, rtt {:.1} ms",
                f64::from(snapshots) / secs,
                clock.0,
                clock.1
            );
            if let Some(p) = position {
                println!("  me at ({:.1}, {:.1}, {:.1})", p[0], p[1], p[2]);
            }
            if let Some(client) = &predictor {
                let info = client.debug_info(now_ms());
                let p = client.position();
                println!(
                    "  predicted ({:.1}, {:.1}, {:.1}): {} confirmed, {} corrected (last error {:.2}), {} suppressed, {} resyncs, {} pending",
                    p.x,
                    p.y,
                    p.z,
                    info.confirmed_count,
                    info.reconcile_count,
                    info.last_error,
                    info.suppressed_snapshots,
                    info.resync_count,
                    info.pending_inputs
                );
            }
            if let Some(m) = &match_state {
                print_standings(m, &standings);
            }
            snapshots = 0;
            events = 0;
            last_report = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    println!("done after {:.0}s", start.elapsed().as_secs_f64());
    exit
}
