//! wash-fps `client`: what the game client draws (remote players and the
//! local first-person weapon so far).

pub mod ammo_status;
pub mod crosshair;
pub mod demo_playback;
pub mod fps_actions;
pub mod fps_announcer;
pub mod fps_bomb_audio;
pub mod fps_combat_audio;
pub mod fps_hud;
pub mod fps_movement_audio;
pub mod fps_pickup_audio;
pub mod fps_player_audio;
pub mod fps_reload_audio;
pub mod game_events;
pub mod hud;
pub mod hud_state;
pub mod input_history;
pub mod leaderboard_view;
pub mod live_play;
pub mod live_watch;
pub mod lobby_panel;
pub mod match_state_manager;
pub mod player_model_visual_settings;
pub mod prediction;
pub mod profile_view;
pub mod remote_player_logic;
pub mod remote_player_renderer;
pub mod settings;
pub mod snapshot_feed;
pub mod v2_network_client;
pub mod weapon_view;
