//! The native client binary. For now the headless client of milestone 2
//! (RUST.md 15): it joins a game server, acknowledges snapshots, keeps the
//! clock in sync and prints the scoreboard.
//!
//! ```text
//! washoi connect <ws://host:port> [--name NAME] [--ticket TICKET] [--seconds N] [--play]
//! ```
//!
//! `--play` asks to join the match and then runs forward in a slow circle,
//! printing where the server puts the player, so the command path is
//! exercised too.

use std::time::{Duration, Instant};

use shagger::network::client::hosts::network_game_host::HostEvent;
use shagger::network::client::network_session::{NetworkSession, SessionConfig, SessionEvent, SessionRequest};
use shagger::shared::js::{js_cos, js_sin};
use shagger::shared::json::Value;
use shagger::shared::net::command::Command;
use washoi::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

struct Args {
    url: String,
    name: String,
    ticket: Option<String>,
    seconds: f64,
    play: bool,
}

fn usage() -> ! {
    eprintln!("usage: washoi connect <ws://host:port> [--name NAME] [--ticket TICKET] [--seconds N] [--play]");
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("connect") {
        println!("washoi {} on shagger", washoi::VERSION);
        usage();
    }
    let Some(url) = args.next() else { usage() };
    let mut parsed = Args {
        url,
        name: "RustClient".into(),
        ticket: None,
        seconds: 10.0,
        play: false,
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--name" => parsed.name = args.next().unwrap_or_else(|| usage()),
            "--ticket" => parsed.ticket = Some(args.next().unwrap_or_else(|| usage())),
            "--seconds" => parsed.seconds = args.next().and_then(|s| s.parse().ok()).unwrap_or_else(|| usage()),
            "--play" => parsed.play = true,
            _ => usage(),
        }
    }
    parsed
}

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

fn main() {
    let args = parse_args();
    println!("connecting to {} as {}", args.url, args.name);
    let session = NetworkSession::start(
        SessionConfig {
            url: args.url.clone(),
            player_name: args.name.clone(),
            admission_ticket: args.ticket.clone(),
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

    while start.elapsed().as_secs_f64() < args.seconds {
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
                    if args.play && !joined {
                        session.send(SessionRequest::JoinGame {
                            team_id: None,
                            manual_pick: false,
                            admission_ticket: None,
                        });
                        joined = true;
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
                    std::process::exit(if exit == 0 { 1 } else { exit });
                }
            }
        }

        if joined && Instant::now() >= next_command {
            // 125 commands a second: forward, turning slowly.
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
    drop(session);
    std::process::exit(exit);
}
