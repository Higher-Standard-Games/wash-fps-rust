//! The native client binary. For now the headless client of milestone 2
//! (RUST.md 15): it logs in to the lobby, finds or queues for a match, joins
//! the game server, acknowledges snapshots, keeps the clock in sync and
//! prints the scoreboard.
//!
//! ```text
//! washoi view <map> [--data DIR] [--spawn N] [--screenshot FILE.png] [--size WxH] [--time T] [--at X,Y,Z,YAW] [--pitch DEG] [--no-cull] [--seconds N]
//! washoi connect <ws://game-server> [--name NAME] [--ticket TICKET] [--seconds N] [--play]
//! washoi lobby <ws://lobby> [--key-file FILE] list
//! washoi lobby <ws://lobby> [--key-file FILE] spectate [--seconds N]
//! washoi lobby <ws://lobby> [--key-file FILE] queue <gametype> [--seconds N] [--play]
//! washoi lobby <ws://lobby> [--key-file FILE] bot [gametype] [difficulty] [--seconds N] [--play]
//! ```
//!
//! `--key-file` keeps the account key: it is read when the file exists, and a
//! new account's key (sent once, in `welcome`) is written to it. Without it
//! every login makes a new account. `--play` joins the match and runs forward
//! in a slow circle, so the command path is exercised too.
//!
//! `view` draws a map with the renderer (`view.rs`), from the
//! packages in `--data` (default `target/washoi-maps/pk3`, which
//! `tools/fetch_maps.sh` fills). `--time` steps shader animation that many
//! seconds before a screenshot; `--pitch` tilts the view (degrees, Quake's
//! sign: negative looks up); `--no-cull` draws every world batch instead of
//! leaving out what the PVS and the frustum hide.

mod view;
mod window;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use shagger::lobby::lobby_client::{LobbyClient, LobbyEvent};
use shagger::lobby::types::{LobbyConnect, LobbyIn};
use shagger::network::client::hosts::network_game_host::HostEvent;
use shagger::network::client::network_session::{NetworkSession, SessionConfig, SessionEvent, SessionRequest};
use shagger::shared::js::{js_cos, js_sin};
use shagger::shared::json::Value;
use shagger::shared::net::command::Command;
use washoi::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

fn usage() -> ! {
    eprintln!(
        "washoi {}\n\
         usage: washoi view <map> [--data DIR] [--spawn N] [--screenshot FILE.png] [--size WxH] [--time T] [--at X,Y,Z,YAW] [--pitch DEG] [--no-cull] [--seconds N]\n\
         \x20      washoi connect <ws://game-server> [--name NAME] [--ticket TICKET] [--seconds N] [--play]\n\
         \x20      washoi lobby <ws://lobby> [--key-file FILE] list\n\
         \x20      washoi lobby <ws://lobby> [--key-file FILE] spectate [--seconds N]\n\
         \x20      washoi lobby <ws://lobby> [--key-file FILE] queue <gametype> [--seconds N] [--play]\n\
         \x20      washoi lobby <ws://lobby> [--key-file FILE] bot [gametype] [difficulty] [--seconds N] [--play]",
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
    let code = match mode.as_str() {
        "connect" => play_match(
            &url,
            o.name.as_deref().unwrap_or("RustClient"),
            o.ticket,
            o.seconds.unwrap_or(10.0),
            o.play,
        ),
        "lobby" => lobby(&url, &o),
        "view" => {
            let (width, height) = o.size.unwrap_or((1280, 720));
            view::run(&view::ViewOptions {
                seconds: o.seconds,
                map: url,
                data: o
                    .data
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("target/washoi-maps/pk3")),
                screenshot: o.screenshot.clone(),
                spawn: o.spawn,
                width,
                height,
                time: o.time,
                at: o.at,
                pitch: o.pitch,
                no_cull: o.no_cull,
            })
        }
        _ => usage(),
    };
    std::process::exit(code);
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
                        return play_match(&server, &name, None, o.seconds.unwrap_or(15.0), false);
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
                return play_match(&server_url, &name, admission_ticket, o.seconds.unwrap_or(15.0), o.play);
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
fn play_match(url: &str, name: &str, ticket: Option<String>, seconds: f64, play: bool) -> i32 {
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

    while start.elapsed().as_secs_f64() < seconds {
        for event in session.poll_events() {
            match event {
                SessionEvent::Host(HostEvent::ServerInfo(w)) => println!(
                    "welcome: \"{}\" ({}), map {}, {} Hz, {}/{} players",
                    w.server_name, w.server_id, w.map, w.tick_rate, w.current_players, w.max_players
                ),
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
                    for event in state.events {
                        if let SnapshotEvent::MatchState(v) = event {
                            if let Some(rows) = v.get("standings").and_then(Value::as_array) {
                                standings = rows.to_vec();
                            }
                            match_state = Some(v);
                        }
                    }
                    if let Some(t) = state.entities.get(&entity_id).and_then(|e| e.transform) {
                        position = Some([t.position.x, t.position.y, t.position.z]);
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

        // 125 commands a second: forward, turning slowly.
        while joined && Instant::now() >= next_command {
            next_command += Duration::from_millis(8);
            yaw += 0.004;
            session.send(SessionRequest::Command(Command {
                direction: [js_sin(yaw), 0.0, js_cos(yaw)],
                pitch: 0.0,
                yaw,
                buttons: 1,
                tick,
                weapon_slot: 0,
            }));
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
