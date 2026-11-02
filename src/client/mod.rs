//! wash-fps `client`: what the game client draws (remote players and the
//! local first-person weapon so far).

pub mod demo_playback;
pub mod fps_announcer;
pub mod fps_combat_audio;
pub mod fps_movement_audio;
pub mod fps_pickup_audio;
pub mod fps_player_audio;
pub mod fps_reload_audio;
pub mod game_events;
pub mod input_history;
pub mod live_watch;
pub mod match_state_manager;
pub mod player_model_visual_settings;
pub mod prediction;
pub mod remote_player_logic;
pub mod remote_player_renderer;
pub mod snapshot_feed;
pub mod v2_network_client;
pub mod weapon_view;
