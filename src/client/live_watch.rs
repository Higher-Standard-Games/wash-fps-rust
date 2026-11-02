//! Watching a live match as a spectator: the connection's snapshots fed to
//! [`SnapshotFeed`] as they arrive, as wash-fps's spectator view feeds
//! `RemotePlayerRenderer` and the projectile sync (`client/GameEvents.ts`).
//!
//! Snapshots are stamped with their arrival on this machine's clock; the
//! server-time bracketing through clock sync is not wired yet, so the
//! buffer adapts its delay to the arrival jitter alone, as the browser does
//! before its clock is synced.

use std::time::{Duration, Instant};

use shagger::network::client::hosts::network_game_host::HostEvent;
use shagger::network::client::network_session::{NetworkSession, SessionConfig, SessionEvent};

use crate::client::snapshot_feed::{DemoView, SnapshotFeed};
use crate::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

pub struct LiveWatch {
    session: NetworkSession<SnapshotEvent>,
    feed: SnapshotFeed,
    started: Instant,
    /// The map the server runs.
    pub map: String,
    /// The spectator's own entity, left out of the bodies drawn.
    entity_id: String,
    /// Why the connection ended, once it has.
    pub closed: Option<String>,
    pub snapshots: u64,
}

impl LiveWatch {
    /// Connects as a spectator and waits up to `timeout` for the welcome,
    /// which names the map.
    pub fn connect(url: &str, name: &str, ticket: Option<String>, timeout: Duration) -> Result<Self, String> {
        let session = NetworkSession::start(
            SessionConfig {
                url: url.to_owned(),
                player_name: name.to_owned(),
                admission_ticket: ticket,
            },
            WashoiEvents,
        );
        let started = Instant::now();
        while started.elapsed() < timeout {
            match session.next_event(Duration::from_millis(100)) {
                Some(SessionEvent::Host(HostEvent::ServerInfo(w))) => {
                    println!(
                        "welcome: \"{}\", map {}, {}/{} players",
                        w.server_name, w.map, w.current_players, w.max_players
                    );
                    return Ok(Self {
                        session,
                        feed: SnapshotFeed::new(),
                        started: Instant::now(),
                        map: w.map.clone(),
                        entity_id: String::new(),
                        closed: None,
                        snapshots: 0,
                    });
                }
                Some(SessionEvent::Host(HostEvent::Failed(why)) | SessionEvent::Closed(why)) => {
                    return Err(format!("{url}: {why}"));
                }
                _ => {}
            }
        }
        Err(format!("{url}: no welcome in {} s", timeout.as_secs()))
    }

    fn now_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    /// Takes the snapshots that arrived and samples everything at now.
    /// The match as the snapshots so far tell it.
    #[must_use]
    pub fn match_state(&self) -> &crate::client::match_state_manager::MatchStateManager {
        &self.feed.match_state
    }

    /// The newest snapshot's players.
    pub fn roster(&self) -> &[(String, shagger::shared::net::game_state::PlayerState)] {
        &self.feed.roster
    }

    pub fn advance(&mut self) -> DemoView {
        for event in self.session.poll_events() {
            match event {
                SessionEvent::Host(HostEvent::Joined { entity_id, .. }) => self.entity_id = entity_id,
                SessionEvent::Host(HostEvent::Snapshot(state)) => {
                    self.snapshots += 1;
                    let now = self.now_ms();
                    self.feed.push(now, &state, &self.entity_id);
                }
                SessionEvent::Host(HostEvent::Failed(why)) | SessionEvent::Closed(why) => {
                    self.closed.get_or_insert(why);
                }
                _ => {}
            }
        }
        let now = self.now_ms();
        self.feed.sample(now)
    }
}
