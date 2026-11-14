//! The window for `washoi view` and `washoi demo`: winit and a wgpu surface,
//! with a free camera. A demo or a watched match plays its players and
//! projectiles on the map in real time.
//!
//! Mouse looks (click to capture it, Escape to let go or quit), W A S D move,
//! F chases the next player of a demo or match (and back to free),
//! Space and C go up and down, Shift is faster. `--seconds` closes the window
//! after that long, for unattended runs. A highlight reel drives the camera
//! itself: Space pauses it, Left and Right step through its shots, R
//! replays it.

use std::sync::Arc;
use std::time::{Duration, Instant};
use washoi::client::fps_liquid_audio::{play_drown_gasp, play_drown_tick};
use washoi::client::player_appearance::{AppearanceView, PlayerAppearance};
use washoi::liquid::client::local_air_tracker::AirEvent;

use shagger::rendering::camera::Camera;
use shagger::rendering::glam::{DVec3, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::hdr_target::HdrTarget;
use shagger::rendering::offscreen::{begin_continue_pass, begin_main_pass, begin_overlay_pass, begin_ui_pass};

use shagger::camera::fov_settings::current_base_fov;
use shagger::core::cvar::with_cvars;
use shagger::input::input_system::{InputSystem, with_input};
use shagger::input::key_codes::{dom_code, dom_mouse_button};
use shagger::movement::movement_vars::movement_var;
use shagger::rendering::wgpu;
use shagger::viewport::ViewCvars;
use shagger::weapon::weapon_view_manager::{ViewModelCvars, ViewModelInput};
use std::cell::Cell;
use washoi::client::fps_actions::{action_key_label, device_key_label, fps, weapon_slot_pressed};
use washoi::client::fps_hud::FpsHud;
use washoi::client::fps_movement_audio::play_jumppad_launch;
use washoi::client::game_events::canonical_weapon_id;
use washoi::client::live_play::PlayInput;

use crate::bench::Bench;
use washoi::shared::entities::definitions::weapons::weapons_table::weapon_hipfire_blocked;
use winit::application::ApplicationHandler;

use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
#[cfg(not(target_os = "macos"))]
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

use crate::demo_overlay::{DemoOverlay, DemoOverlayAction};
use crate::reel_overlay::ReelOverlay;
use crate::view::{
    CLEAR, DeathFollow, DemoViewSounds, Feed, LoadedMap, MapRenderer, SceneReelWorld, antialias_on, apply_reel_camera,
    build_renderer, play_view_sounds, styled_players, update_watch_hud, view_entity,
};
use shagger::animation::player_model_manager::PlayerModelVisualStyle;
use shagger::babylon::post_processes::fxaa_post_process::FxaaPostProcess;
use shagger::debug::debug_console::LogType;
use shagger::input::gamepad_backend::GamepadBackend;
use shagger::input::gamepad_input::GamepadInput;
use shagger::input::input_device::InputDevice;
use shagger::loader::pk3::pk3_loader::locale_compare;
use shagger::network::client::snapshot_buffer::InterpolatedEntity;
use shagger::shared::js::js_hypot;
use shagger::shared::net::game_state::PlayerState;
use shagger::ui::egui_layer::EguiLayer;
use washoi::client::ammo_status::HudWeaponAmmo;
use washoi::client::cinematic_demo_player::{CinematicDemoPlayer, REEL_TITLE_MS, analyze_demo};
use washoi::client::controller_input::{ControllerContext, ControllerInput, PadCvars};
use washoi::client::death_camera::{DeathCamera, look_angles};
use washoi::client::demo_playback_keys::{DemoKeyAction, DemoKeyContext, Modifiers, demo_key_action, frag_seek_tick};
use washoi::client::demo_pov_state::{DemoPovState, watched_eye_pose};
use washoi::client::demo_service::analysis_report;
use washoi::client::first_person_view::{BodyFrame, FirstPersonView, ViewKeys, ViewOffset};
use washoi::client::game_events::GameAudio;
use washoi::client::hud::hint_coach::MovementSample;
use washoi::client::hud::widgets::hint_panel::{AIMING, can_aim_with};
use washoi::client::hud::widgets::loadout_picker::{self, LoadoutOutcome, LoadoutPicker};
use washoi::client::hud::widgets::pause_menu::{PauseMenu, PauseMenuAction};
use washoi::client::hud::widgets::pickup_prompt::{asks_to_swap, swap_text};
use washoi::client::hud::widgets::spectator_cue::pad_bindings;
use washoi::client::match_state_manager::MatchPhase;
use washoi::client::move_stick::{MoveAxis, stick_direction, stick_keys};
use washoi::client::remote_player_logic::CLIENT_KEY_PREFIX;
use washoi::client::settings::controller_model::controller_buttons;
use washoi::client::spectate_view::{DeathCameraCue, SpectateView};
use washoi::client::teleport_effect::TeamColor;
use washoi::client::washoi_style::install_style;
use washoi::client::weapon_view::{clip_reload_time, quad_view_weapon_outline, shot_style};
use washoi::console::{ConsoleRequest, DemoConsole, GameConsole};
use washoi::shared::net::snapshot_events::SnapshotEvent;

/// Units per second (Q3's run speed is 320).
const SPEED: f32 = 320.0;
/// Radians per count of mouse motion at `m_sensitivity` 1
/// (`CameraInputHandler`'s `mouseSensitivity`).
const BASE_SENSITIVITY: f64 = 0.002;

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    hdr: HdrTarget,
    /// The image and the view weapon smoothed (`r_msaa`).
    fxaa: FxaaPostProcess,
    renderer: MapRenderer,
    /// The HUD over a demo or a match.
    hud: Option<FpsHud>,
    /// The console's drawing.
    egui: EguiLayer,
    /// The title last given the window: asking the window for its own is a
    /// round trip to the X server on Linux, so it is never asked.
    title: Option<String>,
}

struct App {
    /// Controllers: the pads (`GamepadInput` over gilrs), the device in
    /// hand, and what the pad does in the match (`ControllerInput`).
    pads: GamepadBackend,
    pad: GamepadInput,
    device: InputDevice,
    controller: ControllerInput,
    /// The move stick this frame, and the scoreboard held on the pad.
    pad_move: Option<MoveAxis>,
    pad_scoreboard: bool,
    map: Option<LoadedMap>,
    camera: Camera,
    surface: Option<Surface>,
    held: std::collections::HashSet<KeyCode>,
    captured: bool,
    /// The cursor, in window pixels, for the post-game's way home.
    cursor: Option<(f64, f64)>,
    /// The post-game has opened up (`FPSHud`'s open-up beat).
    opened_up: bool,
    open_up_due: bool,
    last: Instant,
    started: Instant,
    seconds: Option<f64>,
    frames: u64,
    error: Option<String>,
    /// The free camera's velocity, which sways the view model.
    velocity: Vec3,
    /// The right mouse button, aiming the view model down its sights.
    aiming: bool,
    /// The left mouse button held while the mouse is captured.
    firing: bool,
    /// The view bob cycle (radians), advanced with horizontal speed.
    bob_cycle: f64,
    /// On the ground, for the view bob (always, flying free).
    grounded: bool,
    /// A demo or a live match playing on the map.
    feed: Option<Feed>,
    /// The player the camera chases this frame, by place in the drawn list;
    /// worked out each frame from [`App::follow_id`].
    follow: Option<usize>,
    /// Whom the camera chases, by entity id (`_followedEntityId`), so a
    /// player joining, leaving or dying does not hand the camera to another.
    follow_id: Option<String>,
    /// The viewer chose the free camera (F past the last player); until
    /// then a demo follows its first player (`refreshHudPlayerList`).
    free_camera: bool,
    /// The demo's last seek seen (`discontinuityId`).
    demo_discontinuity: u64,
    /// The view after our own death, and what it follows (`DeathCamera`).
    death_camera: DeathCamera,
    /// The match phase last seen, for the post-game showcase
    /// (`watchMatchPhase`), and the look the showcase's camera took over.
    showcase_phase: Option<MatchPhase>,
    showcase_look: Option<(f32, f32)>,
    /// The first-person camera's motion on top of the mouse, in play.
    first_person: FirstPersonView,
    /// The viewed player's weapon was reloading in the last weapon state.
    view_reloading: bool,
    /// Our Voltaic's beam should show this frame (`FPSPlayer.beamActive`).
    beam_wanted: bool,
    /// The eyes we look through are charging the blade (`_localCharging`):
    /// the shot that ends the charge is the throw, not a swing.
    view_charging: bool,
    /// This frame's: put on the camera for drawing only.
    view_offset: Option<ViewOffset>,
    /// Climbing a ladder this frame: the weapon is stowed.
    on_ladder: bool,
    /// The player whose eyes we look through in a demo or a watch, and
    /// their reconstructed view (`DemoPovState`).
    pov_entity: Option<String>,
    pov: DemoPovState,
    /// The free-for-all colours of this session (`ffaColors`).
    appearance: PlayerAppearance,
    /// The watched player's aim this frame, for their weapon.
    followed_aiming: Option<bool>,
    /// A weapon picked this frame, slid into the hand before the server
    /// confirms it (`FPSPlayer.switchWeapon`).
    predicted_weapon: Option<String>,
    death_follow: Option<DeathFollow>,
    /// The round's focus point the death camera watches at its end.
    death_focus: Option<DVec3>,
    /// Dead in a round: the teammate watched, and the round-end camera
    /// (`SpectateView`).
    spectate: SpectateView,
    /// The match's sound, when there is a sound card.
    audio: Option<GameAudio>,
    /// The drop-down console (the backquote key) and its commands.
    console: GameConsole,
    /// A command asked to leave (`quit`, `disconnect`).
    leave: bool,
    /// The Esc menu over a match we play in (`FPSPauseMenu`).
    pause_menu: PauseMenu,
    /// The kit dialog (`FPSLoadoutPicker`), before the join in a mode
    /// with kits and from the Esc menu's Loadout key.
    loadout_picker: LoadoutPicker,
    /// The settings are up over the match.
    settings_open: bool,
    /// The lobby's settings screen, over the match.
    native_settings: Option<washoi::client::settings::fps_settings_view::FpsSettingsView>,
    /// A demo's highlight reel, driving the demo and the camera.
    reel: Option<CinematicDemoPlayer>,
    overlay: ReelOverlay,
    /// A whole demo's transport bar (`DemoViewerHUD`), when no reel drives it.
    demo_hud: Option<DemoOverlay>,
    /// The players as drawn last frame, for the reel to aim at.
    drawn: Vec<(String, InterpolatedEntity, PlayerModelVisualStyle)>,
    /// `washoi bench`: the frame's stages timed, no vsync, a fixed step.
    bench: Option<Bench>,
    /// The drawing size the bench reports.
    surface_size: [u32; 2],
}

impl App {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let OpenWindow {
            window,
            gpu,
            surface,
            mut config,
        } = open_window(event_loop)?;
        if let Some(bench) = &mut self.bench {
            // As fast as it draws: no waiting for the display.
            config.present_mode = wgpu::PresentMode::AutoNoVsync;
            surface.configure(&gpu.device, &config);
            bench.attach(&gpu);
            self.surface_size = [config.width, config.height];
        }
        // The match loading screen stays up while the renderer is built.
        let mut surface = surface;
        crate::match_loading_screen::draw_curtain(&window, &gpu, &mut surface, &config);
        let map = self.map.as_mut().expect("the map is loaded before the window opens");
        let renderer = build_renderer(&gpu, map, config.format);
        let hdr = HdrTarget::new(&gpu, config.width, config.height);
        let fxaa = FxaaPostProcess::new(&gpu, config.format);
        // A reel is in movie mode: its own cards, no HUD (it is kept for
        // `demoplay`, which leaves the reel).
        let hud = self.feed.as_ref().and_then(|_| match FpsHud::new(&gpu, config.format) {
            Ok(hud) => Some(hud),
            Err(why) => {
                println!("no HUD: {why}");
                None
            }
        });
        let egui = EguiLayer::new(&gpu, &window, config.format);
        install_style(&egui.ctx);
        self.surface = Some(Surface {
            window,
            gpu,
            surface,
            config,
            hdr,
            fxaa,
            renderer,
            hud,
            egui,
            title: None,
        });
        Ok(())
    }

    /// The backquote opens and closes the console (`DebugConsole`'s
    /// keyboard handler); while it is open, the keys and the mouse are its.
    fn console_event(&mut self, event: &WindowEvent) -> bool {
        let chat_typing = self
            .surface
            .as_ref()
            .and_then(|s| s.hud.as_ref())
            .is_some_and(|h| h.chat.is_input_active());
        // Escape closes the open console (`DebugConsole`'s key handler).
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && self.console.console.open
            && key.physical_key == PhysicalKey::Code(KeyCode::Escape)
        {
            if key.state == ElementState::Pressed && !key.repeat {
                self.console.console.toggle();
            }
            return true;
        }
        // A backquote typed into the chat is the chat's.
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.physical_key == PhysicalKey::Code(KeyCode::Backquote)
            && !chat_typing
        {
            if key.state == ElementState::Pressed && !key.repeat {
                self.console.console.toggle();
                if self.console.console.open {
                    self.set_captured(false);
                    self.held.clear();
                    self.firing = false;
                    with_input(InputSystem::release_held_input);
                }
            }
            return true;
        }
        let Some(s) = &mut self.surface else { return false };
        let used = s.egui.on_window_event(&s.window, event);
        // A click the demo bar took is not a click on the game.
        // Its release still reaches the input system, or a button would stay held.
        if used
            && !self.captured
            && self.demo_hud.is_some()
            && matches!(
                event,
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                }
            )
        {
            return true;
        }
        self.console.console.open
            && (used
                || matches!(
                    event,
                    WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
                ))
    }

    /// Playing in a match, before its post-game (where the board shows
    /// the way home, and `showMatchMenu` stays away).
    fn in_play(&self) -> bool {
        matches!(self.feed, Some(Feed::Play(..))) && !self.opened_up
    }

    /// Escape in play: the mouse goes and the menu comes up; with the menu
    /// up, Escape resumes (`FPSHud`'s `pointerlockchange`, `MatchInputBindings`).
    /// While the menu is up the keys are its, not the game's.
    fn pause_menu_key(&mut self, code: KeyCode, event: &winit::event::KeyEvent) -> bool {
        if !self.in_play() {
            self.pause_menu.close();
            return false;
        }
        // The settings have the keyboard; what reaches the window is not
        // the game's. Escape closes what the settings have open, then the
        // settings, back to the menu.
        if self.settings_open {
            if code == KeyCode::Escape
                && event.state == ElementState::Pressed
                && !event.repeat
                && let Some(view) = &mut self.native_settings
                && !view.back()
            {
                self.close_native_settings();
            }
            return true;
        }
        // The kit dialog has the keys; Escape backs out when it can.
        if self.loadout_picker.is_open() {
            if code == KeyCode::Escape
                && event.state == ElementState::Pressed
                && !event.repeat
                && let Some(outcome) = self.loadout_picker.escape()
            {
                self.loadout_outcome(outcome);
            }
            return true;
        }
        if code == KeyCode::Escape {
            if event.state == ElementState::Pressed && !event.repeat {
                if self.pause_menu.open {
                    self.pause_menu_action(PauseMenuAction::Resume);
                } else {
                    self.set_captured(false);
                    self.pause_menu.open();
                }
            }
            return true;
        }
        self.pause_menu.open && event.state == ElementState::Pressed
    }

    /// A key on the Esc menu.
    fn pause_menu_action(&mut self, action: PauseMenuAction) {
        self.pause_menu.close();
        match action {
            PauseMenuAction::Resume => self.set_captured(true),
            PauseMenuAction::Loadout => {
                // `openLoadoutFromMenu`: the kit in use pre-selected.
                if let Some(Feed::Play(p, _)) = &self.feed {
                    let state = p.match_state();
                    let mode = state.gametype_name().to_owned();
                    let timing = state.loadout_mode().map(str::to_owned);
                    let current = loadout_picker::remembered(&mode);
                    self.loadout_picker.open(&mode, current, timing.as_deref(), true);
                }
            }
            PauseMenuAction::Settings => {
                self.native_settings = Some(washoi::client::settings::fps_settings_view::FpsSettingsView::new(true));
                self.settings_open = true;
            }
            PauseMenuAction::Leave => {
                println!("lobby: back to the lobby");
                self.leave = true;
            }
        }
    }

    /// Back from the native settings to the match menu.
    fn close_native_settings(&mut self) {
        if let Some(mut view) = self.native_settings.take() {
            view.pause_controls();
        }
        self.settings_open = false;
        if self.in_play() {
            self.pause_menu.open();
        }
    }

    /// `joinGame`'s picker: a mode with kits waits for one before the join,
    /// with the mouse free to choose it.
    fn offer_loadout(&mut self) {
        let Some(Feed::Play(p, _)) = &self.feed else { return };
        if !p.wants_loadout() || self.loadout_picker.is_open() {
            return;
        }
        let state = p.match_state();
        let mode = state.gametype_name().to_owned();
        let timing = state.loadout_mode().map(str::to_owned);
        let current = loadout_picker::remembered(&mode);
        self.pause_menu.close();
        self.loadout_picker.open(&mode, current, timing.as_deref(), false);
        self.set_captured(false);
    }

    /// The kit dialog closed: a kit is sent (and remembered) and the player
    /// goes back to the game; backing out returns to the Esc menu.
    fn loadout_outcome(&mut self, outcome: LoadoutOutcome) {
        match outcome {
            LoadoutOutcome::Confirm(kit) => {
                if let Some(Feed::Play(p, _)) = &mut self.feed {
                    loadout_picker::remember(p.match_state().gametype_name(), kit);
                    p.choose_loadout(kit.primary, kit.secondary);
                }
                if self.in_play() {
                    self.set_captured(true);
                }
            }
            LoadoutOutcome::Cancel => {
                if self.in_play() {
                    self.pause_menu.open();
                }
            }
        }
    }

    /// F watching: chase the next player by name, then back to the free
    /// camera.
    fn chase_next(&mut self) {
        let order = self
            .feed
            .as_ref()
            .map_or_else(Vec::new, |f| name_order(f.roster(), &self.drawn));
        let next = match self
            .follow_id
            .as_ref()
            .and_then(|id| order.iter().position(|o| o == id))
        {
            None if !self.free_camera || self.follow_id.is_none() => order.first(),
            None => None,
            Some(i) => order.get(i + 1),
        };
        self.follow_id = next.cloned();
        self.free_camera = self.follow_id.is_none();
    }

    /// The pads this frame (`GamepadInput.tick`, then `ControllerInput.update`):
    /// a press on a pad hands the controls to it, and in play the pad holds
    /// its actions, moves, turns and opens the menu; in the menu it walks
    /// the rail.
    fn poll_pads(&mut self, dt: f32) {
        let pads = self.pads.poll();
        self.pad.sample(pads);
        let standard = self.pad.pad.as_ref().is_some_and(|p| p.mapping == "standard");
        let stick = (0..4).any(|i| self.pad.axis(i).abs() > 0.5);
        if standard && (!self.pad.pressed.is_empty() || stick) {
            self.device.follow_pad();
        }
        let typing = self
            .surface
            .as_ref()
            .and_then(|s| s.hud.as_ref())
            .is_some_and(|h| h.chat.is_input_active());
        let ctx = ControllerContext {
            enabled: self.device.gamepad(),
            in_play: self.captured,
            menu: self.pause_menu.open || self.loadout_picker.is_open(),
            blocked: self.surface.is_none() || self.settings_open || self.console.console.open || typing,
        };
        let cvars = PadCvars::read();
        let (controller, pad) = (&mut self.controller, &mut self.pad);
        let frame = with_input(|i| controller.update(f64::from(dt), pad, i, ctx, cvars));
        self.pad_move = frame.move_axis;
        if let Some(held) = frame.scoreboard {
            self.pad_scoreboard = held;
        }
        if frame.leave_play || frame.menu {
            // `leavePlay`: in a match the menu comes up, as for Escape.
            if self.in_play() && !self.pause_menu.open {
                self.pause_menu.open();
            }
            self.set_captured(false);
        }
        if frame.enter_play
            && !self.opened_up
            && !self.pause_menu.open
            && !self.settings_open
            && !self.loadout_picker.is_open()
        {
            self.set_captured(true);
        }
        for intent in frame.nav {
            if self.loadout_picker.is_open() {
                if let Some(outcome) = self.loadout_picker.nav(intent) {
                    self.loadout_outcome(outcome);
                    break;
                }
                continue;
            }
            if let Some(action) = self.pause_menu.nav(intent) {
                self.pause_menu_action(action);
                break;
            }
        }
        // `MatchInputBindings`: watching, fire or the next weapon chases the
        // next player; jump lets go.
        if !matches!(self.feed, Some(Feed::Play(..))) {
            let next = [fps::FIRE, fps::WEAPON_NEXT, fps::WEAPON_3];
            if frame.pressed.iter().any(|a| next.contains(a)) {
                self.chase_next();
            } else if frame.pressed.contains(&fps::JUMP) && self.follow_id.is_some() {
                self.follow_id = None;
                self.free_camera = true;
            }
        }
        // `controllerLook`: the turn, slowed by the aim's share (`in_pad_ads`).
        if self.captured && frame.look != (0.0, 0.0) {
            let ads = self
                .surface
                .as_ref()
                .and_then(|s| s.renderer.view_model.as_ref())
                .map_or(0.0, |v| v.view.ads_progress);
            let share = with_cvars(|c| {
                if c.exists("in_pad_ads") {
                    c.get_number("in_pad_ads")
                } else {
                    0.5
                }
            });
            let scale = 1.0 - ads * (1.0 - if share > 0.0 { share } else { 0.5 });
            self.camera.yaw += (frame.look.0 * scale) as f32;
            let limit = std::f32::consts::FRAC_PI_2 - 0.01;
            self.camera.pitch = (self.camera.pitch + (frame.look.1 * scale) as f32).clamp(-limit, limit);
        }
    }

    fn set_captured(&mut self, captured: bool) {
        let Some(s) = &self.surface else { return };
        let grab = if captured {
            // Without a grab the game still plays: raw mouse motion turns
            // the view and the cursor stays hidden.
            if let Err(e) = s
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| s.window.set_cursor_grab(CursorGrabMode::Confined))
            {
                println!("the mouse could not be captured ({e}); playing without the lock");
            }
            true
        } else {
            let _ = s.window.set_cursor_grab(CursorGrabMode::None);
            false
        };
        s.window.set_cursor_visible(!grab);
        // On macOS a locked cursor keeps its arrow without this.
        shagger::input::native_cursor::set_cursor_hidden(grab);
        if self.captured && !grab {
            // Pointer lock lost: nothing stays held (`releaseHeldInput`).
            self.held.clear();
            self.firing = false;
            with_input(InputSystem::release_held_input);
        }
        self.captured = grab;
    }

    /// Talk mode's keys (`FPSChat`'s input): T or Y opens it while
    /// playing; while open, every key types, Enter sends and Escape
    /// cancels. True when the key was the chat's.
    fn chat_key(&mut self, code: KeyCode, event: &winit::event::KeyEvent) -> bool {
        let Some(Feed::Play(play, _)) = &self.feed else {
            return false;
        };
        let Some(hud) = self.surface.as_mut().and_then(|s| s.hud.as_mut()) else {
            return false;
        };
        let chat = &mut hud.chat;
        if !chat.is_input_active() {
            if event.state == ElementState::Pressed && !event.repeat && matches!(code, KeyCode::KeyT | KeyCode::KeyY) {
                chat.open_input(code == KeyCode::KeyY);
                // Nothing stays held while typing.
                self.held.clear();
                self.firing = false;
                with_input(InputSystem::release_held_input);
                return true;
            }
            return false;
        }
        if event.state != ElementState::Pressed {
            return true;
        }
        match code {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some((text, team_only)) = chat.submit() {
                    play.say(&text, team_only);
                }
                for command in chat.take_console_commands() {
                    self.console.run(&command);
                }
            }
            KeyCode::Escape => chat.close_input(),
            KeyCode::Backspace => chat.backspace(),
            _ => {
                if let Some(text) = &event.text {
                    chat.type_text(text);
                }
            }
        }
        true
    }

    /// Playing with a weapon that fires only aimed (`hipfireBlocked`: the
    /// launchers) in hand.
    fn hipfire_blocked_in_hand(&self) -> bool {
        matches!(&self.feed, Some(Feed::Play(play, _))
            if play.weapon_in_hand().is_some_and(|w| weapon_hipfire_blocked(canonical_weapon_id(w))))
    }

    /// This frame's actions as a player's input, through the player's
    /// bindings (`FPSActions`).
    fn play_input(&self) -> PlayInput {
        let mut input = with_input(|i| PlayInput {
            forward: i.is_active(fps::MOVE_FORWARD),
            backward: i.is_active(fps::MOVE_BACKWARD),
            left: i.is_active(fps::MOVE_LEFT),
            right: i.is_active(fps::MOVE_RIGHT),
            jump: i.is_active(fps::JUMP),
            crouch: i.is_active(fps::CROUCH),
            // `FPSController`: with cl_autorun (on by default) the sprint key walks.
            sprint: i.is_active(fps::SPRINT) != with_cvars(|c| c.get_bool("cl_autorun")),
            // The first click captures the mouse rather than firing.
            fire: self.captured && i.is_active(fps::FIRE),
            alt_fire: i.is_active(fps::AIM),
            reload: i.is_active(fps::RELOAD),
            use_key: i.is_active(fps::USE),
            yaw: f64::from(self.camera.yaw),
            // The slide's dip and the ledge lift aim too (`sendInput`).
            pitch: f64::from(self.camera.pitch) + self.first_person.aim_pitch,
            // Picks go through `LivePlay::select_weapon`.
            weapon_slot: 0,
            auto_ads: false,
            // `FPSController`: the move stick's own angle, holding the keys
            // it points at too.
            move_axis: stick_direction(self.pad_move),
            assisted_aim: self.device.gamepad(),
        });
        let keys = stick_keys(input.move_axis);
        input.forward |= keys.forward;
        input.backward |= keys.backward;
        input.left |= keys.left;
        input.right |= keys.right;
        // `hipfireBlocked`: the trigger on a launcher puts the aim up.
        if input.fire && !input.alt_fire && self.hipfire_blocked_in_hand() {
            input.alt_fire = true;
            input.auto_ads = true;
        }
        input
    }

    fn step(&mut self, dt: f32) {
        if matches!(self.feed, Some(Feed::Play(..))) {
            // `FPSController.update`: without the pointer lock, no keys play.
            let input = if self.captured {
                self.play_input()
            } else {
                PlayInput {
                    yaw: f64::from(self.camera.yaw),
                    pitch: f64::from(self.camera.pitch) + self.first_person.aim_pitch,
                    ..PlayInput::default()
                }
            };
            // Weapon select: number keys and the wheel, pressed this frame.
            let pick = if self.captured {
                with_input(|i| {
                    (
                        weapon_slot_pressed(i),
                        i.was_pressed(fps::WEAPON_NEXT),
                        i.was_pressed(fps::WEAPON_PREV),
                    )
                })
            } else {
                (0, false, false)
            };
            // `FPSController.syncKeysFromInput`: the ready key, pressed this frame.
            let ready = self.captured && with_input(|i| i.was_pressed(fps::READY));
            // No pick while the weapon slides (`setWeaponSwitchBlockedCheck`).
            let switching = self
                .surface
                .as_ref()
                .is_some_and(|s| s.renderer.switch_anim.is_switching());
            if let Some(Feed::Play(play, held)) = &mut self.feed {
                *held = input;
                if let Some(id) = play.select_weapon(pick.0, pick.1, pick.2, switching) {
                    self.predicted_weapon = Some(id);
                }
                if ready {
                    play.send_ready();
                }
            }
            self.velocity = Vec3::ZERO;
            return;
        }
        let key = |k| self.held.contains(&k);
        let axis = |plus, minus| f32::from(u8::from(key(plus))) - f32::from(u8::from(key(minus)));
        let speed = if key(KeyCode::ShiftLeft) { SPEED * 3.0 } else { SPEED };
        // A move stick flies its own way, jump and crouch up and down.
        let pad_held = |action| {
            with_input(|i| controller_buttons(i, action))
                .iter()
                .any(|b| usize::try_from(*b).is_ok_and(|b| self.pad.down.contains(&b)))
        };
        let (stick, pad_up, pad_down) = if self.captured && self.device.gamepad() {
            (self.pad_move, pad_held(fps::JUMP), pad_held(fps::CROUCH))
        } else {
            (None, false, false)
        };
        let (forward, right) = stick.map_or_else(
            || (axis(KeyCode::KeyW, KeyCode::KeyS), axis(KeyCode::KeyD, KeyCode::KeyA)),
            |m| (m.forward as f32, m.right as f32),
        );
        let vertical =
            axis(KeyCode::Space, KeyCode::KeyC) + f32::from(u8::from(pad_up)) - f32::from(u8::from(pad_down));
        let movement = self.camera.forward() * forward + self.camera.right() * right + Vec3::Y * vertical;
        let velocity = movement.normalize_or_zero() * speed;
        self.camera.position += velocity * dt;
        self.velocity = velocity;
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        // The bench steps the demo 60 times a second whatever the frame
        // took, so every run draws the same frames.
        let dt = if self.bench.is_some() {
            1.0 / 60.0
        } else {
            now.duration_since(self.last).as_secs_f32().min(0.1)
        };
        self.last = now;
        if let Some(b) = &mut self.bench {
            b.begin_frame();
        }
        self.poll_pads(dt);
        with_input(InputSystem::update);
        // `FPSController`'s `hipfireBlocked`: the trigger on a launcher puts
        // the aim up, its look and feel too (`keys.aim = true`), not only
        // the server's ALT_FIRE.
        let launcher = self.hipfire_blocked_in_hand();
        self.aiming = self.captured && with_input(|i| i.is_active(fps::AIM) || (launcher && i.is_active(fps::FIRE)));
        self.step(dt);
        with_input(InputSystem::end_frame);
        // `cl_fov`, as `currentBaseFov` gives every camera.
        self.camera.fov_y = with_cvars(|c| current_base_fov(c)) as f32;
        let Some(s) = &mut self.surface else { return };
        s.renderer.world.animate(&s.gpu, f64::from(dt));
        // The rotators turn and the lifts stand where the lift state says.
        let feed = self.feed.as_ref();
        if let Some(f) = feed {
            s.renderer.lifts.restore(&f.lift_states());
        }
        let lifts = &s.renderer.lifts;
        s.renderer.world.move_movers(
            &s.gpu,
            f64::from(dt),
            |model| feed.and_then(|f| f.lift_travel(model)),
            |model| {
                let o = lifts.button_offset(model);
                (o.x != 0.0 || o.y != 0.0 || o.z != 0.0).then_some([o.x, o.y, o.z])
            },
        );
        s.renderer.item_time += f64::from(dt);
        if let Some(feed) = &mut self.feed {
            if let (Some(reel), Feed::Demo(demo)) = (&mut self.reel, &mut *feed) {
                reel.update(
                    f64::from(dt) * 1000.0,
                    demo,
                    &mut SceneReelWorld {
                        shots: s.renderer.shot_world_mut(),
                        players: &self.drawn,
                    },
                );
                self.overlay.update(reel, f64::from(dt));
            }
            if matches!(feed, Feed::Demo(_)) && self.console.ctx.demo.is_none() {
                self.console.ctx.demo = Some(DemoConsole { hud_visible: true });
            }
            if let (None, Feed::Demo(demo)) = (&self.reel, &*feed) {
                let hud = self.demo_hud.get_or_insert_with(|| DemoOverlay::new(demo));
                hud.update(f64::from(dt), demo.paused());
            }
            let playhead = match &*feed {
                Feed::Demo(d) => Some(d.playhead_tick()),
                _ => None,
            };
            let view = feed.advance(f64::from(dt));
            if let Some(b) = &mut self.bench {
                b.mark("feed");
            }
            // `_syncDemoPresentationClock`: in a demo the effects and the
            // HUD's timed entries run on the demo clock (still while paused,
            // slow at a quarter speed), and a seek drops them.
            let mut effect_dt = f64::from(dt);
            let mut seeked = false;
            if let (Feed::Demo(d), Some(before)) = (&*feed, playhead) {
                effect_dt = ((d.playhead_tick() - before) / d.tick_rate()).clamp(0.0, 0.25);
                if d.discontinuity() != self.demo_discontinuity {
                    seeked = true;
                    self.demo_discontinuity = d.discontinuity();
                    effect_dt = 0.0;
                    s.renderer.discontinuity();
                    if let Some(audio) = &mut self.audio {
                        audio.discontinuity();
                    }
                    if let Some(hud) = &mut s.hud {
                        hud.state.kill_feed.clear();
                        hud.state.powerups.clear();
                        hud.chat.clear_messages();
                    }
                }
            }
            self.view_offset = None;
            self.on_ladder = false;
            if let Feed::Play(p, held) = feed {
                // First person: the eye where prediction puts it.
                if let Some(yaw) = p.take_spawn_facing() {
                    // `faceSpawn`: the spawn's facing, level.
                    self.camera.yaw = yaw as f32;
                    self.camera.pitch = 0.0;
                }
                if let Some(t) = p.take_teleport() {
                    // `setViewYaw(exitYaw)`, then `localTeleported`.
                    self.camera.yaw = t.exit_yaw as f32;
                    s.renderer.teleports.local_teleported(
                        DVec3::new(t.from.x, t.from.y, t.from.z),
                        DVec3::new(t.to.x, t.to.y, t.to.z),
                        t.entry_yaw,
                        t.exit_yaw,
                        TeamColor::Alpha,
                    );
                }
                if let Some(eye) = p.eye() {
                    self.camera.position = Vec3::new(eye.x as f32, eye.y as f32, eye.z as f32);
                }
                // `FPSController.update`: steps, slide, ledge and the view
                // effects, drawn but never aimed with.
                match p.feet() {
                    Some(feet) => {
                        let m = p.movement();
                        let v = p.velocity();
                        let body = BodyFrame {
                            feet: DVec3::new(feet.x, feet.y, feet.z),
                            eye_lift: shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT
                                + m.crouch_view_offset,
                            velocity: DVec3::new(v.x, v.y, v.z),
                            grounded: p.grounded(),
                            sliding: m.is_sliding,
                            horizontal_speed: m.actual_horiz_speed,
                            slide_phase: m.slide_phase,
                            ledge_grab_phase: m.ledge_grab_phase,
                            ledge_grab: m.ledge_grab_state,
                            wall_jump: m.wall_jump_state,
                        };
                        let keys = ViewKeys {
                            forward: held.forward,
                            backward: held.backward,
                            left: held.left,
                            right: held.right,
                            jump: held.jump,
                            sprint: held.sprint,
                            aim: self.aiming,
                            fire: held.fire,
                        };
                        let (cvars, step_height, smooth_factor) = view_cvars();
                        self.view_offset = Some(self.first_person.update(
                            f64::from(dt),
                            &body,
                            &keys,
                            &cvars,
                            step_height,
                            smooth_factor,
                        ));
                        self.on_ladder = m.on_ladder;
                        // `FPSHintPanel.moved`: the coach sees each step.
                        if let Some(hud) = &mut s.hud {
                            hud.hints.moved(
                                &MovementSample {
                                    grounded: body.grounded,
                                    sliding: body.sliding,
                                    horizontal_speed: js_hypot(&[v.x, v.z]),
                                    vertical_speed: v.y,
                                    wall_jump: body.wall_jump,
                                    ledge_grab: body.ledge_grab,
                                    forward: held.forward,
                                    strafing: held.left || held.right,
                                },
                                &hud.state.current_weapon,
                            );
                        }
                        // `Game`'s own trail, from where we are drawn.
                        s.renderer.slide_dust.emit(
                            "local",
                            body.feet,
                            body.velocity.x,
                            body.velocity.z,
                            p.alive && p.grounded() && m.is_sliding && m.water_level == 0 && !m.on_ladder,
                        );
                    }
                    None => self.first_person.reset(),
                }
                let v = p.velocity();
                self.velocity = Vec3::new(v.x as f32, v.y as f32, v.z as f32);
                self.grounded = p.grounded();
                self.follow = None;
                // `switchWeapon` slides a picked weapon in at once; the
                // server's weapon is put in hand when its `weapon_state`
                // arrives and no switch is under way (`handleWeaponState`),
                // which heals a pick the server did not take.
                let arrived = p.take_weapon_state();
                let predicted = self.predicted_weapon.take();
                // A teammate's eyes put their weapon in hand.
                match p.weapon_in_hand() {
                    _ if self.spectate.watching().is_some() => {}
                    None => s.renderer.hold(None),
                    Some(_) if predicted.is_some() => s.renderer.hold(predicted.as_deref()),
                    Some(server) => {
                        if s.renderer.view_model.is_none() || (arrived && !s.renderer.switch_anim.is_switching()) {
                            s.renderer.hold(Some(server));
                        }
                    }
                }
                if let Some(at) = p.take_jumppad_launch()
                    && let Some(audio) = &mut self.audio
                {
                    play_jumppad_launch(&mut audio.sfx, DVec3::new(at.x, at.y, at.z), true);
                }
                // `LocalLiquid.emitLocalCrossing`: our own splash and air.
                let local = p.take_local_liquid(f64::from(dt));
                if local.respawned {
                    s.renderer.liquid.forget("local");
                }
                let at = DVec3::new(local.position.x, local.position.y, local.position.z);
                s.renderer
                    .notify_liquid("local", local.level, at, local.vertical_speed, true);
                if let (Some(event), Some(audio)) = (local.air, &mut self.audio) {
                    match event {
                        AirEvent::StartedDrowning => play_drown_gasp(&mut audio.sfx),
                        AirEvent::DrownTick => play_drown_tick(&mut audio.sfx),
                    }
                }
            }
            let appearance_view = AppearanceView {
                teams: &view.teams,
                looks: &view.looks,
                perspective: self.pov_entity.as_deref(),
                own: feed.local_entity(),
                team_based: feed.match_state().team_based(),
            };
            let (players, models) = styled_players(view.players, &mut self.appearance, &appearance_view);
            // Our own body is never drawn; its corpse wears our style.
            let local_style = match feed {
                Feed::Play(..) => feed.local_entity().map(|id| {
                    self.appearance.style(
                        id,
                        &appearance_view,
                        washoi::client::remote_player_renderer::MODEL_SCALE,
                    )
                }),
                _ => None,
            };
            s.renderer.set_local_style(local_style);
            s.renderer.live_models = models;
            // `watchMatchPhase`: the winner on show through the post-game,
            // gone with the next phase.
            let phase = feed.match_state().phase();
            if self.showcase_phase != Some(phase) {
                self.showcase_phase = Some(phase);
                if s.renderer.showcase.take().is_some()
                    && let Some((yaw, pitch)) = self.showcase_look.take()
                {
                    (self.camera.yaw, self.camera.pitch) = (yaw, pitch);
                }
                let winner = (phase == MatchPhase::PostGame)
                    .then(|| feed.match_state().resolve_winner_player_id())
                    .flatten()
                    .and_then(|id| {
                        let key = format!("{CLIENT_KEY_PREFIX}{id}");
                        feed.roster()
                            .iter()
                            .find(|(k, _)| *k == key)
                            .map(|(_, p)| p.entity_id.clone())
                    });
                if let Some(entity) = winner {
                    // Our own gun is the one on screen; anyone else's the
                    // last seen in their hand.
                    let weapon = if feed.local_entity() == Some(entity.as_str()) {
                        s.renderer.view_model.as_ref().map(|v| v.view.weapon.clone())
                    } else {
                        None
                    }
                    .or_else(|| view.weapons.get(&entity).cloned());
                    let style = self.appearance.style(
                        &entity,
                        &appearance_view,
                        washoi::client::remote_player_renderer::MODEL_SCALE,
                    );
                    let aspect = f64::from(s.config.width) / f64::from(s.config.height.max(1));
                    s.renderer
                        .start_showcase(appearance_view.model_for(&entity), style, weapon, &self.camera, aspect);
                    self.showcase_look = Some((self.camera.yaw, self.camera.pitch));
                }
            }
            let base_fov = self.camera.fov_y;
            let eyes = self.reel.as_ref().and_then(|r| {
                self.follow = None;
                apply_reel_camera(&mut self.camera, r, &players, base_fov)
            });
            // Our own camera in play; a frame without players keeps
            // `--follow N` for the first frame that has them.
            if self.reel.is_none() && !matches!(feed, Feed::Play(..)) && !players.is_empty() {
                // `--follow N` names a place in the first frame's list.
                if let (Some(i), None) = (self.follow, &self.follow_id) {
                    self.follow_id = players.get(i).map(|(id, _, _)| id.clone());
                }
                // A demo watches someone until the viewer flies free, and
                // someone else when the one watched leaves.
                let gone = self
                    .follow_id
                    .as_ref()
                    .is_some_and(|id| !players.iter().any(|(p, _, _)| p == id));
                if (self.follow_id.is_none() || gone) && !self.free_camera && matches!(feed, Feed::Demo(_)) {
                    self.follow_id = name_order(feed.roster(), &players).into_iter().next();
                }
                self.follow = self
                    .follow_id
                    .as_ref()
                    .and_then(|id| players.iter().position(|(p, _, _)| p == id));
            }
            // `SpectateView`: dead in a round, a teammate's eyes after the
            // corpse, and what decided the round at its end.
            if let Feed::Play(p, _) = &*feed {
                let cue = self.spectate.update(
                    f64::from(dt),
                    !p.alive,
                    &p.entity_id,
                    feed.match_state(),
                    players.iter().map(|(id, _, _)| id.as_str()),
                );
                match cue {
                    DeathCameraCue::Keep => {}
                    DeathCameraCue::Focus(at) => {
                        // From behind wherever we were looking.
                        self.death_camera.start(at, f64::from(self.camera.yaw));
                        self.death_follow = None;
                        self.death_focus = Some(at);
                    }
                    DeathCameraCue::Stop => {
                        self.death_camera.stop();
                        self.death_focus = None;
                    }
                    DeathCameraCue::Resume => {
                        let at = self.camera.position.as_dvec3()
                            - DVec3::Y * shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT;
                        self.death_camera.start(at, f64::from(self.camera.yaw));
                        self.death_follow = None;
                        self.death_focus = None;
                    }
                }
                self.follow = self
                    .spectate
                    .watching()
                    .and_then(|id| players.iter().position(|(p, _, _)| p == id));
            }
            // `FollowedViewWeapon` and `SpectateEyeCamera`: the watched
            // player's eyes, their screen's view effects, and their weapon
            // in hand, swaying with their stride.
            self.followed_aiming = None;
            let followed = self
                .follow
                .and_then(|i| players.get(i))
                .map(|(id, p, _)| (id.clone(), *p));
            match followed {
                Some((id, p)) => {
                    let cut = self.pov_entity.as_deref() != Some(id.as_str());
                    if cut {
                        self.pov.reset();
                        self.pov_entity = Some(id.clone());
                    }
                    let aiming = p.aiming.unwrap_or(false);
                    self.pov.set_aiming(aiming);
                    self.followed_aiming = Some(aiming);
                    let (cvars, step_height, smooth_factor) = view_cvars();
                    let pose = self
                        .pov
                        .update(&id, &p, effect_dt, seeked, &cvars, step_height, smooth_factor);
                    let eye = watched_eye_pose(&p, Some(&pose));
                    self.camera.position = eye.position.as_vec3();
                    self.camera.yaw = eye.yaw as f32;
                    self.camera.pitch = eye.pitch as f32;
                    self.camera.roll = 0.0;
                    let r = pose.effects.rotation_offset * (std::f64::consts::PI / 180.0);
                    self.view_offset = Some(ViewOffset {
                        eye: eye.position,
                        pitch: r.x,
                        yaw: r.y,
                        roll: r.z,
                        fov_multiplier: pose.effects.fov_multiplier,
                        local_offset: pose.effects.position_offset,
                        horizontal_speed: pose.horizontal_speed,
                        bob_cycle: pose.bob_cycle,
                        landing_velocity: pose.landing_velocity,
                    });
                    self.velocity = Vec3::new(p.vx as f32, p.vy as f32, p.vz as f32);
                    self.grounded = p.grounded;
                    // `showWeapon`: the first weapon shown is a cut, later
                    // ones switch.
                    if let Some(weapon) = view.weapons.get(&id) {
                        let weapon = canonical_weapon_id(weapon);
                        if cut {
                            s.renderer.hold_cut(Some(weapon));
                        } else {
                            s.renderer.hold(Some(weapon));
                        }
                    }
                }
                None => {
                    if self.pov_entity.take().is_some() {
                        s.renderer.hold_cut(None);
                    }
                }
            }
            // Their own model would fill the screen from inside their head.
            let drawn: Vec<_> = players
                .iter()
                .filter(|(id, _, _)| Some(id) != eyes.as_ref() && Some(id) != self.pov_entity.as_ref())
                .cloned()
                .collect();
            s.renderer.set_live_weapons(&view.weapons);
            s.renderer.set_live_players(&drawn, f64::from(dt), effect_dt);
            s.renderer.play_showcase(f64::from(dt));
            let local = match feed {
                Feed::Play(..) => feed.local_entity(),
                _ => None,
            };
            s.renderer.set_local_player(local, f64::from(self.camera.yaw));
            s.renderer.set_followed_player(self.pov_entity.as_deref());
            s.renderer
                .set_live_projectiles(&view.projectiles, effect_dt, &self.camera);
            s.renderer
                .play_effects(&view.effects, &view.weapons, effect_dt, &self.camera);
            s.renderer.dropped_weapons.use_server_transforms = matches!(feed, Feed::Demo(_));
            s.renderer
                .play_dropped_weapons(&view.effects, &view.dropped_weapons, effect_dt);
            s.renderer.play_bomb_sites(feed.match_state(), effect_dt);
            // `triggerCameraShake`: explosions near our own body.
            let spawned = std::mem::take(&mut s.renderer.spawned_explosions);
            if let Feed::Play(p, _) = &*feed
                && let Some(body) = p.feet()
            {
                let screenshake = with_cvars(|c| c.get_bool("cg_screenshake"));
                let body = DVec3::new(body.x, body.y, body.z);
                for (at, look) in spawned {
                    self.first_person.explosion_at(look, at, body, screenshake);
                }
            }
            s.renderer.play_pickups(&view.pickup_active, effect_dt);
            s.renderer.play_gibs(&view.effects, effect_dt);
            // Our own death: the camera follows the corpse or the burst
            // until we are back (`applyDeathCamera`).
            if let Some((follow, at, yaw)) = s.renderer.take_local_death() {
                self.death_camera.start(at, yaw);
                self.death_follow = Some(follow);
            }
            if let Feed::Play(p, _) = &*feed
                && p.alive
            {
                self.death_camera.stop();
            }
            if self.death_camera.is_active() {
                let target = self
                    .death_focus
                    .or_else(|| self.death_follow.and_then(|f| s.renderer.follow_position(f)));
                let renderer = &mut s.renderer;
                let mut trace = |from, to| renderer.trace_map(from, to);
                if let Some((at, look)) = self.death_camera.update(f64::from(dt), target, Some(&mut trace)) {
                    let (yaw, pitch) = look_angles(at, look);
                    self.camera.position = at.as_vec3();
                    self.camera.yaw = yaw as f32;
                    self.camera.pitch = pitch as f32;
                }
            }
            // The diorama camera frames the post-game's winner.
            if let Some(showcase) = &s.renderer.showcase {
                showcase.frame(&mut self.camera);
            }
            // `presentFire` and the reload, for the eyes we look through:
            // the server's shots and weapon state (wash-fps `FPSPlayer`).
            if let Some(viewed) = view_entity(
                feed.local_entity().filter(|_| self.spectate.watching().is_none()),
                &players,
                self.follow,
            ) {
                let viewed = viewed.to_owned();
                for event in &view.effects {
                    match event {
                        SnapshotEvent::WeaponFire {
                            entity_id, weapon_id, ..
                        } if *entity_id == viewed => {
                            let projectile_fire = std::mem::take(&mut self.view_charging);
                            if let Some(view_model) = &mut s.renderer.view_model {
                                let style = shot_style(canonical_weapon_id(weapon_id));
                                view_model.view.present_fire(&style, projectile_fire);
                                // `kickLocalView`: only our own shots kick.
                                if feed.local_entity() == Some(entity_id.as_str()) {
                                    self.first_person
                                        .kick_for_shot(canonical_weapon_id(weapon_id), view_model.view.ads_progress);
                                    // `noteLocalShotForHints(canAimWith(weaponId))`.
                                    if let Some(hud) = &mut s.hud {
                                        hud.hints.shot(
                                            view_model.view.ads_progress < AIMING,
                                            can_aim_with(weapon_id),
                                            weapon_id,
                                        );
                                    }
                                }
                            }
                            s.renderer.fire_view_muzzle_flash(&self.camera, projectile_fire);
                        }
                        SnapshotEvent::WeaponCharge {
                            entity_id, charging, ..
                        } if *entity_id == viewed => {
                            self.view_charging = *charging;
                            // `noteLocalChargeForHints`.
                            if *charging
                                && feed.local_entity() == Some(entity_id.as_str())
                                && let Some(hud) = &mut s.hud
                            {
                                hud.hints.charged();
                            }
                        }
                        // The pain shake, our own only (`GameEvents`).
                        SnapshotEvent::Damage {
                            entity_id,
                            damage,
                            weapon_id,
                            ..
                        } if feed.local_entity() == Some(entity_id.as_str()) => {
                            // `noteLocalFallForHints`.
                            if matches!(weapon_id.as_str(), "fall" | "fall_crush")
                                && let Some(hud) = &mut s.hud
                            {
                                let weapon = hud.state.current_weapon.clone();
                                hud.hints.fell(&weapon);
                            }
                            let hitshake = with_cvars(|c| c.get_bool("cg_hitshake"));
                            self.first_person.hit(f64::from(*damage), weapon_id, hitshake);
                        }
                        SnapshotEvent::WeaponState {
                            entity_id,
                            current_weapon,
                            reloading,
                            ..
                        } if *entity_id == viewed => {
                            let weapon = canonical_weapon_id(current_weapon);
                            if let Some(view_model) = &mut s.renderer.view_model
                                && view_model.view.weapon == weapon
                            {
                                match (*reloading, view_model.view.reload_anim.is_reloading()) {
                                    (true, false) if !self.view_reloading => {
                                        if let Some(time) = clip_reload_time(weapon) {
                                            view_model.view.start_reload(time);
                                        }
                                    }
                                    (false, true) => view_model.view.cancel_reload(),
                                    _ => {}
                                }
                            }
                            self.view_reloading = *reloading;
                        }
                        _ => {}
                    }
                }
            }
            s.renderer.play_hit_feedback(
                &view.effects,
                view_entity(
                    feed.local_entity().filter(|_| self.spectate.watching().is_none()),
                    &players,
                    self.follow,
                ),
                effect_dt,
            );
            if self.audio.is_none() {
                s.renderer.drop_sounds();
            }
            if let Some(audio) = &mut self.audio {
                play_view_sounds(
                    audio,
                    &mut s.renderer,
                    &self.camera,
                    &DemoViewSounds {
                        effects: &view.effects,
                        teams: &view.teams,
                        looks: &view.looks,
                        snapshots: &view.snapshots,
                        match_state: feed.match_state(),
                        local: feed.local_entity(),
                    },
                    &players,
                    self.follow,
                    f64::from(dt),
                );
            }
            // `FPSPlayer`: our quad puts its outline on the gun.
            let quad = matches!(feed, Feed::Play(..))
                && s.hud.as_mut().is_some_and(|h| {
                    let now = h.state.now;
                    h.state.powerups.is_active("quad", now)
                });
            // `FPSPickup`'s and `DroppedWeaponRenderer`'s swap prompts: in
            // play, alive, with the mouse ours; asked for under each owner.
            if let Some(hud) = &mut s.hud {
                let at = match &*feed {
                    Feed::Play(p, _)
                        if !self.pause_menu.open && !self.settings_open && !self.loadout_picker.is_open() =>
                    {
                        p.feet()
                    }
                    _ => None,
                };
                let (map, floor) = match at {
                    Some(at) => s.renderer.weapons_in_reach(DVec3::new(at.x, at.y, at.z)),
                    None => (None, None),
                };
                let key = with_input(|i| action_key_label(i, fps::USE));
                let state = &hud.state;
                let ask = |weapon: Option<&str>| {
                    weapon
                        .filter(|w| asks_to_swap(w, &state.weapons, &state.current_weapon))
                        .map(|w| swap_text(&key, w))
                };
                let (pickup, dropped) = (ask(map), ask(floor));
                hud.pickup_prompt.set("pickup", pickup);
                hud.pickup_prompt.set("dropped", dropped);
                // `FPSHintPanel.update`'s inputs: the keys, fire held from
                // the hip with a gun that aims, the water; and `FPSPlayer`'s
                // emptied clip, where it reloads by itself.
                let gamepad = self.device.gamepad();
                with_input(|i| {
                    hud.hints.set_keys(i, gamepad);
                    // The cards that name keys, for the device in hand.
                    hud.ready.pad_key = gamepad.then(|| device_key_label(i, fps::READY, true));
                    hud.spectator_cue.pad = gamepad.then(|| {
                        let swap = with_cvars(|c| c.exists("in_pad_swap") && c.get_number("in_pad_swap") != 0.0);
                        let label = |a| device_key_label(i, a, true);
                        pad_bindings(swap, &label(fps::JUMP), &label(fps::CROUCH), &label(fps::FIRE))
                    });
                });
                let (ads, shown) = s
                    .renderer
                    .view_model
                    .as_ref()
                    .map_or((1.0, ""), |v| (v.view.ads_progress, v.view.weapon.as_str()));
                let (fire, water) = match &*feed {
                    Feed::Play(p, held) => (p.alive && held.fire, p.movement().water_level),
                    _ => (false, 0),
                };
                hud.hints.holding_fire = fire && ads < AIMING && can_aim_with(shown);
                hud.hints.water_level = water;
                let weapon = hud.state.current_weapon.clone();
                let dry = fire
                    && matches!(hud.state.ammo.get(&weapon),
                        Some(HudWeaponAmmo::Clip { clip, reserve }) if *clip <= 0.0 && *reserve > 0.0);
                hud.hints.dry_fire(dry, &weapon);
            }
            let outline = quad.then(|| quad_view_weapon_outline(s.renderer.item_time));
            if let Some(view_model) = &mut s.renderer.view_model {
                view_model.powerup_outline = outline;
            }
            let notices = feed.take_notices();
            if let Some(hud) = &mut s.hud {
                for notice in &notices {
                    hud.chat.add_notice(notice);
                }
            }
            for notice in &notices {
                println!("server: {notice}");
            }
            if let (Some(hud), None) = (&mut s.hud, &self.reel) {
                hud.scoreboard.held = self.held.contains(&KeyCode::Tab) || self.pad_scoreboard;
                hud.scale = s.window.scale_factor() as f32;
                hud.set_markers(
                    s.renderer.bomb_site_poses(),
                    &self.camera,
                    s.config.width as f32 / s.config.height.max(1) as f32,
                );
                update_watch_hud(
                    hud,
                    feed,
                    &players,
                    self.follow,
                    f64::from(self.camera.yaw),
                    &view.effects,
                    effect_dt,
                    s.config.width as f32,
                    s.config.height as f32,
                );
                hud.prepare(&s.gpu);
                // `FPSHud`: `setAmmoCount(currentClip(state))`.
                if let Some(view_model) = &mut s.renderer.view_model {
                    view_model.ammo = hud.state.current_clip();
                }
                // `FPSPlayer.updateFiring`: the Voltaic's beam shows while fire
                // is held, alive, with a loaded clip, neither reloading nor
                // switching, and not while a freeze holds movement (the
                // post-game, a round's countdown and its end).
                self.beam_wanted = matches!(&*feed, Feed::Play(p, held)
                    if p.alive
                        && held.fire
                        && p.weapon_in_hand().map(canonical_weapon_id) == Some("voltaic")
                        && !p.match_state().is_movement_frozen())
                    && hud.state.current_clip() > 0.0
                    && !self.view_reloading
                    && !s.renderer.switch_anim.is_switching();
                // The post-game opens up: the mouse comes back for the way
                // home, and the chat is ready (`FPSHud`).
                let offered = hud.scoreboard.return_offered();
                if offered && !self.opened_up {
                    self.open_up_due = true;
                }
                self.opened_up = offered;
            }
            self.drawn = players;
            // Hold the last frame; the title says so, until a seek goes back.
            let title = feed
                .ended()
                .map_or_else(|| "Washoi".to_owned(), |why| format!("Washoi ({why})"));
            if s.title.as_deref() != Some(title.as_str()) {
                s.window.set_title(&title);
                s.title = Some(title);
            }
        }
        s.renderer.advance_switch(f64::from(dt));
        let light = s.renderer.light_at(self.camera.position);
        let submersion = s.renderer.underwater.submersion_amount();
        if let Some(view_model) = &mut s.renderer.view_model {
            view_model.view.apply_cvars(&with_cvars(|c| ViewModelCvars::read(c)));
            // `setMovementHidden(onLadder)`: a climb takes both hands.
            view_model.view.set_movement_hidden(self.on_ladder);
            view_model.view.update_lighting(f64::from(dt), light.as_ref());
            view_model.view.submersion_amount = submersion;
            let mut horizontal = f64::from(self.velocity.x.hypot(self.velocity.z));
            // Our own eyes: the weapon bob follows the view's walk bob and
            // feels the landings it does (`updateWeaponBob`). Anyone else's:
            // a stride per 160 units, as a run reads.
            let mut landing_velocity = 0.0;
            self.bob_cycle += horizontal * f64::from(dt) / 160.0 * std::f64::consts::PI;
            let mut bob_cycle = self.bob_cycle;
            if let Some(o) = &self.view_offset {
                horizontal = o.horizontal_speed;
                bob_cycle = o.bob_cycle;
                landing_velocity = o.landing_velocity;
            }
            view_model.view.update(&ViewModelInput {
                delta_time: f64::from(dt),
                grounded: self.grounded,
                horizontal_speed: horizontal,
                velocity_y: f64::from(self.velocity.y),
                landing_velocity,
                bob_cycle,
                aiming: self.followed_aiming.unwrap_or(self.aiming),
                reloading: false,
            });
        }
        // The view's motion goes on for drawing and comes off after, so the
        // mouse and the aim never carry it.
        let mouse = (self.camera.pitch, self.camera.yaw);
        let offset = self
            .view_offset
            .take()
            .filter(|_| !self.death_camera.is_active() && s.renderer.showcase.is_none());
        if let Some(o) = offset {
            self.camera.pitch += o.pitch as f32;
            self.camera.yaw += o.yaw as f32;
            self.camera.roll = o.roll as f32;
            self.camera.fov_y *= o.fov_multiplier as f32;
            let eye = o.eye_in_world(self.camera.right().as_dvec3(), self.camera.forward().as_dvec3());
            self.camera.position = eye.as_vec3();
        }
        s.renderer.update_underwater(&mut self.camera, f64::from(dt));
        if let Some(b) = &mut self.bench {
            b.mark("scene");
        }
        // The frame is acquired only now: the match, the sounds and the
        // effects above advance whether or not there is one to draw.
        let frame = match next_frame(&s.gpu, &s.window, &mut s.surface, &s.config) {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(why) => {
                self.error = Some(why);
                return;
            }
        };
        let target = frame
            .texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        if let Some(b) = &mut self.bench {
            b.mark("acquire");
        }
        // `setBeamActive` and `updateBeam`: our beam from this frame's muzzle.
        s.renderer.set_beam_active(std::mem::take(&mut self.beam_wanted));
        s.renderer.update_beam(&self.camera);
        s.renderer
            .prepare(&s.gpu, &self.camera, [s.config.width as f32, s.config.height as f32]);
        if offset.is_some() {
            (self.camera.pitch, self.camera.yaw) = mouse;
            self.camera.roll = 0.0;
        }
        if let Some(b) = &mut self.bench {
            b.mark("prepare");
        }
        let mut encoder = s
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let mut stamp = |encoder: &mut wgpu::CommandEncoder, name| {
            if let Some(b) = &mut self.bench {
                b.stamp(encoder, name);
            }
        };
        stamp(&mut encoder, "start");
        {
            let mut pass = begin_main_pass(&mut encoder, &s.hdr.color, &s.hdr.depth, CLEAR);
            s.renderer.draw_opaque(&mut pass);
        }
        stamp(&mut encoder, "opaque");
        s.renderer.capture(&mut encoder, &s.hdr);
        stamp(&mut encoder, "capture");
        {
            let mut pass = begin_continue_pass(&mut encoder, &s.hdr.color, &s.hdr.depth);
            s.renderer.draw_transparent(&mut pass);
        }
        stamp(&mut encoder, "transparent");
        // With antialiasing on, the image and the weapon go through FXAA on
        // their way to the output; the HUD draws over it untouched.
        let smooth = antialias_on();
        let image = if smooth {
            s.fxaa.input(&s.gpu, (s.config.width, s.config.height))
        } else {
            &target
        };
        s.hdr.resolve(
            &s.gpu,
            &mut encoder,
            &s.renderer.post,
            &s.renderer.image_processing,
            image,
        );
        stamp(&mut encoder, "post");
        {
            // The weapon camera draws after the world camera's image
            // processing, straight to the output (`begin_overlay_pass`).
            let mut pass = begin_overlay_pass(&mut encoder, image, &s.hdr.depth);
            s.renderer.draw_overlay(&mut pass);
        }
        stamp(&mut encoder, "overlay");
        if smooth {
            s.fxaa.apply(&mut encoder, &target);
            stamp(&mut encoder, "fxaa");
        }
        if let (Some(hud), None) = (&s.hud, &self.reel) {
            let mut pass = begin_ui_pass(&mut encoder, &target);
            hud.draw(&mut pass);
        }
        let cards = self.console.ctx.demo.is_none_or(|d| d.hud_visible);
        let console = &mut self.console;
        let (overlay, reel, feed) = (&self.overlay, &self.reel, &self.feed);
        let followed = self
            .follow
            .and_then(|i| self.drawn.get(i))
            .map(|(id, _, _)| id.as_str());
        let mut demo_action = None;
        let mut menu_action = None;
        let pause_menu = &mut self.pause_menu;
        let (kicker, player) = match feed {
            Some(Feed::Play(p, _)) if pause_menu.open || self.settings_open => {
                let state = p.match_state();
                let kicker = [state.gametype_name(), p.map.as_str()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                let player = p
                    .roster()
                    .iter()
                    .find(|(id, _)| *id == p.player_id)
                    .map(|(_, s)| s.name.clone())
                    .unwrap_or_default();
                (kicker, player)
            }
            _ => (String::new(), String::new()),
        };
        self.pause_menu.settings = true;
        // `matchMenuModeKeys`: Loadout, in a mode with kits, hinted with the kit.
        self.pause_menu.loadout = match feed {
            Some(Feed::Play(p, _)) if p.match_state().loadout_mode().is_some() => Some(
                loadout_picker::remembered(p.match_state().gametype_name())
                    .map_or_else(|| "Choose your kit".to_owned(), loadout_picker::describe_loadout),
            ),
            _ => None,
        };
        let pause_menu = &mut self.pause_menu;
        let native_settings = &mut self.native_settings;
        let mut settings_back = false;
        let loadout = &mut self.loadout_picker;
        let mut loadout_outcome = None;
        let demo_hud = self.demo_hud.as_ref().filter(|_| cards);
        let ui = s.egui.run(&s.window, |ui| {
            if let (Some(reel), Some(Feed::Demo(demo)), true) = (reel, feed, cards) {
                overlay.ui(ui.ctx(), reel, demo);
            }
            if let (None, Some(Feed::Demo(demo)), Some(hud)) = (reel, feed, demo_hud) {
                let name = followed.and_then(|id| {
                    demo.roster()
                        .iter()
                        .find(|(_, p)| p.entity_id == id)
                        .map(|(_, p)| p.name.as_str())
                });
                demo_action = hud.ui(ui.ctx(), demo, followed.map(|id| (id, name.unwrap_or(id))));
            }
            menu_action = pause_menu.ui(ui.ctx(), &kicker, &player);
            if let Some(view) = native_settings {
                settings_back = washoi::client::settings::fps_settings_view::over_match(ui, view, &kicker);
            }
            loadout_outcome = loadout.ui(ui.ctx());
            console.ui(ui.ctx());
        });
        s.egui
            .paint(&s.gpu, &mut encoder, &target, [s.config.width, s.config.height], ui);
        if let Some(b) = &mut self.bench {
            b.stamp(&mut encoder, "ui");
            b.resolve(&mut encoder);
            b.mark("encode");
        }
        s.gpu.queue.submit([encoder.finish()]);
        if let Some(b) = &mut self.bench {
            b.mark("submit");
        }
        present(&s.gpu, &s.window, &mut s.surface, &s.config, frame);
        if let Some(b) = &mut self.bench {
            b.mark("present");
            b.end_frame(&s.gpu);
        }
        self.frames += 1;
        if let Some(action) = menu_action {
            self.pause_menu_action(action);
        }
        if let Some(outcome) = loadout_outcome {
            self.loadout_outcome(outcome);
        }
        self.offer_loadout();
        if settings_back {
            self.close_native_settings();
        }
        if let (Some(action), Some(Feed::Demo(demo))) = (demo_action, &mut self.feed) {
            match action {
                DemoOverlayAction::TogglePause => demo.toggle_play_pause(),
                DemoOverlayAction::Seek(progress) => demo.seek_to_progress(progress),
                DemoOverlayAction::Speed(step) => {
                    demo.step_speed(step);
                }
                DemoOverlayAction::Exit => {
                    println!("demo: back to the lobby");
                    self.leave = true;
                }
            }
            if let Some(hud) = &mut self.demo_hud {
                hud.wake();
            }
        }
        if std::mem::take(&mut self.open_up_due) {
            self.set_captured(false);
            if let Some(hud) = self.surface.as_mut().and_then(|s| s.hud.as_mut())
                && !hud.chat.is_input_active()
            {
                hud.chat.open_input(false);
            }
        }
        self.serve_console();
    }

    /// A whole demo's keys (`DemoPlaybackKeys` outside a reel): Space
    /// pauses, the arrows seek and change speed, comma and period step a
    /// frame, N and P jump between frags, Tab and the digits pick whose
    /// eyes to watch through. Flying free, Space and the arrows move.
    fn demo_key(&mut self, code: KeyCode, repeat: bool) -> bool {
        let (None, Some(Feed::Demo(demo)), Some(hud)) = (&self.reel, &mut self.feed, &mut self.demo_hud) else {
            return false;
        };
        let held = |k| self.held.contains(&k);
        let modifiers = Modifiers {
            shift: held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight),
            alt: held(KeyCode::AltLeft) || held(KeyCode::AltRight),
            ctrl: held(KeyCode::ControlLeft) || held(KeyCode::ControlRight),
            meta: held(KeyCode::SuperLeft) || held(KeyCode::SuperRight),
        };
        let context = DemoKeyContext {
            reel: false,
            free_camera: self.free_camera,
        };
        let Some(action) = demo_key_action(&dom_code(code), modifiers, context) else {
            return false;
        };
        let repeats = matches!(action, DemoKeyAction::Seek { .. } | DemoKeyAction::Frame { .. });
        if repeat && !repeats {
            return true;
        }
        hud.wake();
        let rate = demo.tick_rate();
        let order = name_order(demo.roster(), &self.drawn);
        let players = order.len();
        match action {
            DemoKeyAction::TogglePause => demo.toggle_play_pause(),
            DemoKeyAction::Seek { seconds } => demo.seek_to_tick(demo.playhead_tick() + seconds * rate),
            DemoKeyAction::Frame { step } => {
                demo.pause();
                demo.seek_to_frame(demo.current_frame() as isize + isize::from(step));
            }
            DemoKeyAction::Speed { step } => {
                demo.step_speed(step);
            }
            DemoKeyAction::Frag { step } => {
                if let Some(tick) = frag_seek_tick(&hud.frag_ticks(), demo.playhead_tick(), step, rate) {
                    demo.seek_to_tick(tick);
                }
            }
            DemoKeyAction::Player { step } => {
                if players > 0 {
                    let current = self
                        .follow_id
                        .as_ref()
                        .and_then(|id| order.iter().position(|o| o == id));
                    let at = current.unwrap_or(if step > 0 { players - 1 } else { 0 });
                    let next = (at as isize + isize::from(step)).rem_euclid(players as isize) as usize;
                    self.follow_id = Some(order[next].clone());
                    self.free_camera = false;
                }
            }
            DemoKeyAction::PlayerAt { index } => {
                if let Some(id) = order.get(index) {
                    self.follow_id = Some(id.clone());
                    self.free_camera = false;
                }
            }
            DemoKeyAction::Shot { .. } => return false,
        }
        true
    }

    /// The reel's keys (`DemoPlaybackKeys` in reel mode): Space pauses,
    /// Left and Right step through the shots, R plays it again.
    fn reel_key(&mut self, code: KeyCode) -> bool {
        let (Some(reel), Some(Feed::Demo(demo))) = (&mut self.reel, &mut self.feed) else {
            return false;
        };
        let shots = self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut());
        let mut world = SceneReelWorld {
            shots,
            players: &self.drawn,
        };
        match code {
            KeyCode::Space => reel.toggle_play_pause(demo),
            KeyCode::ArrowRight => reel.next_segment(demo, &mut world),
            KeyCode::ArrowLeft => reel.prev_segment(demo, &mut world),
            KeyCode::KeyR => reel.replay(demo, &mut world),
            _ => return false,
        }
        true
    }

    /// What the console's commands asked for this frame.
    fn serve_console(&mut self) {
        for request in std::mem::take(&mut self.console.ctx.requests) {
            match request {
                ConsoleRequest::Say { text, team } => {
                    if let Some(Feed::Play(p, _)) = &self.feed {
                        p.say(&text, team);
                    }
                }
                ConsoleRequest::CallPause(timeout) => {
                    if let Some(Feed::Play(p, _)) = &self.feed {
                        p.call_pause(timeout);
                    }
                }
                ConsoleRequest::Disconnect | ConsoleRequest::DemoStop => self.leave = true,
                ConsoleRequest::DemoPlay => {
                    if let Some(Feed::Demo(demo)) = &mut self.feed {
                        self.reel = None;
                        demo.set_speed(1.0);
                        demo.seek_to_frame(0);
                        demo.play();
                    }
                }
                ConsoleRequest::DemoCinematic { minutes } => {
                    if let Some(Feed::Demo(demo)) = &mut self.feed {
                        let mut reel = CinematicDemoPlayer::new(REEL_TITLE_MS);
                        reel.load_and_play(
                            demo,
                            &mut SceneReelWorld {
                                shots: self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut()),
                                players: &self.drawn,
                            },
                            minutes,
                        );
                        self.follow = None;
                        self.reel = Some(reel);
                    }
                }
                ConsoleRequest::DemoAnalyze => {
                    if let Some(Feed::Demo(demo)) = &self.feed {
                        let analysis = analyze_demo(
                            demo,
                            &mut SceneReelWorld {
                                shots: self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut()),
                                players: &self.drawn,
                            },
                        );
                        let report = analysis_report(&analysis, &demo.header.map, demo.duration_ms);
                        self.console.console.log(LogType::Result, &report);
                    }
                }
            }
        }
        if self.console.ctx.quit {
            self.leave = true;
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_some() {
            return;
        }
        if let Err(why) = self.open(event_loop) {
            self.error = Some(why);
            end_screen(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Keys winit reports as held when focus returns were pressed
        // elsewhere (Alt after Alt+Tab): they are not this window's.
        if let WindowEvent::KeyboardInput {
            event: key,
            is_synthetic: true,
            ..
        } = &event
            && key.state == ElementState::Pressed
        {
            return;
        }
        if self.console_event(&event) {
            return;
        }
        match &event {
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(b) = dom_mouse_button(*button) {
                    with_input(|i| match state {
                        ElementState::Pressed => i.mouse_down(b),
                        ElementState::Released => i.mouse_up(b),
                    });
                }
            }
            // `WheelEvent.deltaY`: positive scrolls down, where winit's
            // positive scrolls up.
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -f64::from(*y) * 100.0,
                    MouseScrollDelta::PixelDelta(p) => -p.y,
                };
                with_input(|i| i.wheel(dy));
            }
            _ => {}
        }
        match event {
            WindowEvent::CloseRequested => end_screen(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(s) = &mut self.surface {
                    s.config.width = size.width.max(1);
                    s.config.height = size.height.max(1);
                    s.surface.configure(&s.gpu.device, &s.config);
                    s.hdr.resize(&s.gpu, s.config.width, s.config.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                if event.state == ElementState::Pressed {
                    self.device.follow_keyboard_mouse();
                }
                if self.chat_key(code, &event) {
                    return;
                }
                if self.pause_menu_key(code, &event) {
                    return;
                }
                with_input(|i| match event.state {
                    ElementState::Pressed => i.key_down(&dom_code(code)),
                    ElementState::Released => i.key_up(&dom_code(code)),
                });
                match event.state {
                    // A held Escape frees the mouse once; its repeats do not quit.
                    ElementState::Pressed if code == KeyCode::Escape && event.repeat => {}
                    ElementState::Pressed if code == KeyCode::Escape => {
                        if self.captured {
                            self.set_captured(false);
                        } else {
                            end_screen(event_loop);
                        }
                    }
                    ElementState::Pressed if !event.repeat && self.reel_key(code) => {}
                    // A reel's keys do not repeat (`bindPlaybackKeys`).
                    ElementState::Pressed if event.repeat && self.reel.is_some() => {}
                    ElementState::Pressed if self.demo_key(code, event.repeat) => {}
                    ElementState::Pressed
                        if code == KeyCode::KeyF && !event.repeat && !matches!(self.feed, Some(Feed::Play(..))) =>
                    {
                        self.chase_next();
                    }
                    ElementState::Pressed => {
                        self.held.insert(code);
                    }
                    ElementState::Released => {
                        self.held.remove(&code);
                    }
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                // The post-game's way home (`setReturnAction`).
                let home = state == ElementState::Pressed
                    && !self.captured
                    && self.surface.as_ref().and_then(|s| s.hud.as_ref()).is_some_and(|h| {
                        self.cursor
                            .is_some_and(|(x, y)| h.scoreboard.return_hit(x as f32 / h.scale, y as f32 / h.scale))
                    });
                if home {
                    println!("lobby: back to the lobby");
                    end_screen(event_loop);
                } else if !self.captured
                    && state == ElementState::Pressed
                    && !self.opened_up
                    && !self.pause_menu.open
                    && !self.settings_open
                    && !self.loadout_picker.is_open()
                {
                    self.set_captured(true);
                } else {
                    self.firing = state == ElementState::Pressed;
                }
            }
            // `DemoViewerHUD`: a move of the mouse brings the bar back
            // (not looking around, which moves a confined cursor on Windows).
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x, position.y));
                if !self.captured
                    && let Some(hud) = &mut self.demo_hud
                {
                    hud.wake();
                }
            }
            WindowEvent::Focused(false) => {
                self.held.clear();
                self.firing = false;
                with_input(InputSystem::release_held_input);
                // The mouse lost in play brings the menu up (`pointerlockchange`).
                if self.captured && self.in_play() {
                    self.pause_menu.open();
                }
                self.set_captured(false);
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.error.is_some() {
                    end_screen(event_loop);
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event
            && delta != (0.0, 0.0)
        {
            self.device.follow_keyboard_mouse();
        }
        if let DeviceEvent::MouseMotion { delta } = event
            && self.captured
        {
            // `m_sensitivity`, and `cl_ads_sensitivity`'s share while aiming
            // (the browser eases it in with the aim; this takes it whole).
            let sensitivity = with_cvars(|c| {
                let ads = if self.aiming {
                    movement_var(c, "cl_ads_sensitivity")
                } else {
                    1.0
                };
                BASE_SENSITIVITY * movement_var(c, "m_sensitivity") * ads
            }) as f32;
            self.camera.yaw += delta.0 as f32 * sensitivity;
            let limit = std::f32::consts::FRAC_PI_2 - 0.01;
            self.camera.pitch = (self.camera.pitch + delta.1 as f32 * sensitivity).clamp(-limit, limit);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.leave {
            end_screen(event_loop);
            return;
        }
        if let Some(limit) = self.seconds
            && self.started.elapsed() >= Duration::from_secs_f64(limit)
        {
            end_screen(event_loop);
            return;
        }
        // The bench ends when it has its frames, or the demo does.
        if let Some(b) = &self.bench
            && (b.done() || self.feed.as_ref().is_some_and(|f| f.ended().is_some()))
        {
            end_screen(event_loop);
            return;
        }
        if let Some(s) = &self.surface {
            s.window.request_redraw();
        }
    }
}

thread_local! {
    /// One event loop for the process: winit makes only one, so the menu
    /// and then the match take turns on it (`run_app_on_demand`).
    static EVENT_LOOP: std::cell::RefCell<Option<EventLoop<()>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `app` on the process's event loop until it exits.
pub fn run_on_event_loop(app: &mut impl ApplicationHandler) -> Result<(), String> {
    EVENT_LOOP.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let mut builder = EventLoop::builder();
            *slot = Some(builder.build().map_err(|e| format!("no window system: {e}"))?);
        }
        let event_loop = slot.as_mut().expect("made above");
        event_loop.set_control_flow(ControlFlow::Poll);
        SCREEN_ENDED.with(|e| e.set(false));
        let mut app = Exiting {
            app,
            resume: cfg!(target_os = "macos") && LOOP_STARTED.with(Cell::get),
        };
        LOOP_STARTED.with(|s| s.set(true));
        #[cfg(target_os = "macos")]
        {
            // The loop never exits between screens here: an exit closes
            // every window (winit #4135), and with it the fullscreen Space,
            // so each screen would land the player on another desktop. It
            // is pumped instead, until the screen ends or the app quits.
            use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
            loop {
                if let PumpStatus::Exit(_) = event_loop.pump_app_events(None, &mut app) {
                    break;
                }
                if SCREEN_ENDED.with(Cell::get) {
                    break;
                }
            }
            shagger::core::atomic_file::flush_pending(Duration::from_secs(2));
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        event_loop
            .run_app_on_demand(&mut app)
            .map_err(|e| format!("event loop: {e}"))
    })
}

thread_local! {
    /// The running screen called [`end_screen`].
    static SCREEN_ENDED: Cell<bool> = const { Cell::new(false) };
    /// A screen has run on the event loop before: the next one is not sent
    /// `resumed` by winit when the loop is pumped (macOS), so it is sent here.
    static LOOP_STARTED: Cell<bool> = const { Cell::new(false) };
}

/// Ends the screen running on the event loop: [`run_on_event_loop`] returns.
/// On macOS the loop itself keeps going (pumped), so the window and its
/// fullscreen Space stay for the next screen; elsewhere it exits, and the
/// kept window stays all the same.
pub fn end_screen(event_loop: &ActiveEventLoop) {
    SCREEN_ENDED.with(|e| e.set(true));
    #[cfg(not(target_os = "macos"))]
    event_loop.exit();
    #[cfg(target_os = "macos")]
    let _ = event_loop;
}

thread_local! {
    static ALT_HELD: Cell<bool> = const { Cell::new(false) };
    /// The window open now, for Alt+Enter.
    static CURRENT_WINDOW: std::cell::RefCell<std::sync::Weak<Window>> = const { std::cell::RefCell::new(std::sync::Weak::new()) };
}

/// Borderless fullscreen on the window's own monitor, or back.
pub fn toggle_fullscreen() {
    let Some(window) = CURRENT_WINDOW.with(|w| w.borrow().upgrade()) else {
        return;
    };
    let full = window.fullscreen().is_none();
    window.set_fullscreen(full.then_some(Fullscreen::Borderless(None)));
    // The game remembers it for the next start; `washoi view` does not.
    if KEEP_WINDOW.with(Cell::get) {
        with_cvars(|c| c.set("vid_fullscreen", i32::from(full)));
    }
}

/// The game's first window opens fullscreen unless the player left it
/// windowed (`vid_fullscreen`, on by default).
fn starts_fullscreen() -> bool {
    with_cvars(|c| !c.exists("vid_fullscreen") || c.get_number("vid_fullscreen") != 0.0)
}

/// Every screen's handler, with one addition: when the system ends the
/// process from inside the loop (Cmd+Q on macOS terminates the app there,
/// so nothing after the loop runs), the settings still on their way to
/// disk land first.
/// The view effects' cvars, the step height and the view smoothing
/// (`FPSController`'s and `SpectateEyeCamera`'s reads), in one look.
fn view_cvars() -> (ViewCvars, f64, f64) {
    with_cvars(|c| {
        (
            ViewCvars::read(c),
            movement_var(c, "sv_stepheight"),
            movement_var(c, "cl_smoothfactor"),
        )
    })
}

struct Exiting<'a, A> {
    app: &'a mut A,
    /// Send `resumed` with the first events: the pumped loop (macOS) sends
    /// it only once, to the first screen.
    resume: bool,
}

impl<A: ApplicationHandler> ApplicationHandler for Exiting<'_, A> {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        self.app.new_events(event_loop, cause);
        if std::mem::take(&mut self.resume) {
            self.app.resumed(event_loop);
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resume = false;
        self.app.resumed(event_loop);
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        self.app.user_event(event_loop, event);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        if let WindowEvent::ModifiersChanged(m) = &event {
            ALT_HELD.with(|a| a.set(m.state().alt_key()));
        }
        // `InputSystem.onKeyDown`: Alt+Enter toggles fullscreen, on every
        // screen, and goes no further.
        if let WindowEvent::KeyboardInput { event: key, .. } = &event
            && key.state == ElementState::Pressed
            && matches!(
                key.physical_key,
                PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter)
            )
            && ALT_HELD.with(Cell::get)
        {
            if !key.repeat {
                toggle_fullscreen();
            }
            return;
        }
        self.app.window_event(event_loop, window_id, event);
    }
    fn device_event(&mut self, event_loop: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
        self.app.device_event(event_loop, device_id, event);
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.app.about_to_wait(event_loop);
    }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.app.suspended(event_loop);
    }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.app.exiting(event_loop);
        shagger::core::atomic_file::flush_pending(Duration::from_secs(2));
    }
    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.app.memory_warning(event_loop);
    }
}

/// A window with a wgpu surface on it, configured.
pub struct OpenWindow {
    pub window: Arc<Window>,
    pub gpu: Gpu,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

/// A frame to draw into; `stale` when the surface should be configured again
/// once it is shown (`Suboptimal`, as after a move to another monitor).
pub struct Frame {
    pub texture: wgpu::SurfaceTexture,
    stale: bool,
}

/// The next frame, or `None` to skip this one: the window is hidden or
/// minimised (`Occluded`, which Metal reports), the wait timed out, or the
/// surface had to be configured or made again. Only a validation error is
/// returned as one.
///
/// # Errors
/// The surface failed validation.
pub fn next_frame(
    gpu: &Gpu,
    window: &Arc<Window>,
    surface: &mut wgpu::Surface<'static>,
    config: &wgpu::SurfaceConfiguration,
) -> Result<Option<Frame>, String> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(texture) => Ok(Some(Frame { texture, stale: false })),
        wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Ok(Some(Frame { texture, stale: true })),
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => Ok(None),
        wgpu::CurrentSurfaceTexture::Outdated => {
            surface.configure(&gpu.device, config);
            Ok(None)
        }
        wgpu::CurrentSurfaceTexture::Lost => {
            // Configuring a lost surface again does not bring it back; a new
            // one for the same window does.
            *surface = gpu
                .instance
                .create_surface(window.clone())
                .map_err(|e| format!("surface lost: {e}"))?;
            surface.configure(&gpu.device, config);
            Ok(None)
        }
        other => Err(format!("surface: {other:?}")),
    }
}

/// Shows `frame`, then configures the surface again if it had gone stale.
pub fn present(
    gpu: &Gpu,
    window: &Window,
    surface: &mut wgpu::Surface<'static>,
    config: &wgpu::SurfaceConfiguration,
    frame: Frame,
) {
    window.pre_present_notify();
    gpu.queue.present(frame.texture);
    // Some drivers report every frame suboptimal, and configuring the same
    // surface again does not stop them: at most once a second, then.
    thread_local! {
        static LAST_RECONFIGURE: Cell<Option<Instant>> = const { Cell::new(None) };
    }
    if frame.stale
        && LAST_RECONFIGURE.with(|last| {
            let due = last.get().is_none_or(|at| at.elapsed() >= Duration::from_secs(1));
            if due {
                last.set(Some(Instant::now()));
            }
            due
        })
    {
        surface.configure(&gpu.device, config);
    }
}

/// Opens Washoi's window, 1280 × 720, with a non-sRGB surface in vsync.
/// The game's icon for the title bar and the task bar (macOS takes the
/// app bundle's instead).
fn window_icon() -> Option<winit::window::Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/icon/washoi-128.png"))
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    winit::window::Icon::from_rgba(image.into_raw(), width, height).ok()
}

#[derive(Clone, Copy)]
struct Placement {
    size: PhysicalSize<u32>,
    /// Wayland does not tell.
    position: Option<PhysicalPosition<i32>>,
    maximized: bool,
    fullscreen: bool,
}

thread_local! {
    static PLACEMENT: Cell<Option<Placement>> = const { Cell::new(None) };
}

/// Keeps `window`'s size, place and maximised state for the next screen's
/// window ([`open_window`]); called as each screen closes its window.
pub fn remember_placement(window: &Window) {
    // Minimised, Windows reports 0 × 0 at (-32000, -32000): the next window
    // would open there, unseen. Keep the last placement instead.
    let size = window.inner_size();
    if window.is_minimized() == Some(true) || size.width == 0 || size.height == 0 {
        return;
    }
    PLACEMENT.with(|p| {
        p.set(Some(Placement {
            size: window.inner_size(),
            position: window.outer_position().ok(),
            maximized: window.is_maximized(),
            fullscreen: window.fullscreen().is_some(),
        }));
    });
}

thread_local! {
    /// The window every screen shares once [`keep_window_between_screens`]
    /// is on: the lobby and every match.
    static KEPT_WINDOW: std::cell::RefCell<Option<Arc<Window>>> = const { std::cell::RefCell::new(None) };
    static KEEP_WINDOW: Cell<bool> = const { Cell::new(false) };
}

/// From now on every screen opens on one window, made once: the lobby,
/// the loading screen and the match.
pub fn keep_window_between_screens() {
    KEEP_WINDOW.with(|k| k.set(true));
}

/// The shared window, once a screen has made it.
#[must_use]
pub fn kept_window() -> Option<Arc<Window>> {
    KEPT_WINDOW.with(|w| w.borrow().clone())
}

pub fn open_window(event_loop: &ActiveEventLoop) -> Result<OpenWindow, String> {
    let kept = if KEEP_WINDOW.with(Cell::get) {
        kept_window()
    } else {
        None
    };
    let window = match kept {
        Some(window) => window,
        None => {
            let window = create_window(event_loop)?;
            if KEEP_WINDOW.with(Cell::get) {
                KEPT_WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
            }
            window
        }
    };
    surface_on(event_loop, window)
}

fn create_window(event_loop: &ActiveEventLoop) -> Result<Arc<Window>, String> {
    // 1280 × 720 in points, so a HiDPI screen does not halve it; then where
    // the last screen's window was, so going from the menu to a match keeps
    // the player's size, place and maximised state.
    let mut attributes = Window::default_attributes()
        .with_title("Washoi")
        .with_inner_size(LogicalSize::new(1280.0, 720.0))
        .with_window_icon(window_icon());
    // The desktop's name for the window, which a `.desktop` file matches
    // to its icon and launcher entry on Wayland and X11.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::wayland::WindowAttributesExtWayland;
        attributes = WindowAttributesExtWayland::with_name(attributes, "net.washoi.Washoi", "washoi");
    }
    // The game (one window for every screen) starts as the player left it;
    // `washoi view` and the tools open a window.
    if PLACEMENT.with(Cell::get).is_none() && KEEP_WINDOW.with(Cell::get) && starts_fullscreen() {
        attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
    }
    if let Some(last) = PLACEMENT.with(Cell::get) {
        attributes = attributes.with_inner_size(last.size).with_maximized(last.maximized);
        if last.fullscreen {
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        if let Some(position) = last.position {
            attributes = attributes.with_position(position);
        }
    }
    let window = Arc::new(event_loop.create_window(attributes).map_err(|e| e.to_string())?);
    CURRENT_WINDOW.with(|w| *w.borrow_mut() = Arc::downgrade(&window));
    Ok(window)
}

/// A wgpu surface on `window`, configured.
fn surface_on(event_loop: &ActiveEventLoop, window: Arc<Window>) -> Result<OpenWindow, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
        event_loop.owned_display_handle(),
    )));
    let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
    let gpu = Gpu::new(instance, Some(&surface)).map_err(|e| e.to_string())?;
    println!("gpu: {}", gpu.describe());
    let size = window.inner_size();
    let mut config = surface
        .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
        .ok_or("the surface does not fit this adapter")?;
    // Babylon writes display-space bytes as they are: prefer a non-sRGB target.
    let formats = surface.get_capabilities(&gpu.adapter).formats;
    if let Some(&plain) = formats.iter().find(|f| !f.is_srgb()) {
        config.format = plain;
    }
    config.present_mode = wgpu::PresentMode::AutoVsync;
    surface.configure(&gpu.device, &config);
    Ok(OpenWindow {
        window,
        gpu,
        surface,
        config,
    })
}

/// Opens the window and runs until it closes.
///
/// # Errors
/// What ended the match early (the event loop, the surface, the server),
/// for the menu to show.
#[allow(clippy::too_many_arguments)]
pub fn run(
    map: LoadedMap,
    camera: Camera,
    seconds: Option<f64>,
    feed: Option<Feed>,
    follow: Option<usize>,
    audio: Option<GameAudio>,
    reel: Option<CinematicDemoPlayer>,
    bench: Option<Bench>,
) -> Result<i32, String> {
    // A key still down when the last window closed never saw its release.
    with_input(InputSystem::release_held_input);
    let mut app = App {
        map: Some(map),
        camera,
        surface: None,
        held: std::collections::HashSet::new(),
        captured: false,
        cursor: None,
        opened_up: false,
        open_up_due: false,
        last: Instant::now(),
        started: Instant::now(),
        seconds,
        frames: 0,
        error: None,
        velocity: Vec3::ZERO,
        aiming: false,
        firing: false,
        bob_cycle: 0.0,
        grounded: true,
        console: {
            let mut console = GameConsole::new(washoi::config_dir());
            console.ctx.server = feed.as_ref().and_then(Feed::server_url);
            console
        },
        feed,
        follow,
        follow_id: None,
        free_camera: false,
        demo_discontinuity: 0,
        death_camera: DeathCamera::default(),
        showcase_phase: None,
        showcase_look: None,
        first_person: FirstPersonView::default(),
        view_reloading: false,
        beam_wanted: false,
        view_charging: false,
        view_offset: None,
        on_ladder: false,
        pov_entity: None,
        pov: DemoPovState::default(),
        appearance: PlayerAppearance::default(),
        followed_aiming: None,
        predicted_weapon: None,
        death_follow: None,
        death_focus: None,
        spectate: SpectateView::default(),
        audio,
        leave: false,
        pause_menu: PauseMenu::default(),
        loadout_picker: LoadoutPicker::default(),
        settings_open: false,
        native_settings: None,
        reel,
        overlay: ReelOverlay::default(),
        demo_hud: None,
        drawn: Vec::new(),
        bench,
        surface_size: [0, 0],
        pads: GamepadBackend::new(),
        pad: GamepadInput::default(),
        device: InputDevice::default(),
        controller: ControllerInput::default(),
        pad_move: None,
        pad_scoreboard: false,
    };
    let ran = run_on_event_loop(&mut app);
    // Close the window before the next one opens on the same loop.
    if let Some(s) = &app.surface {
        remember_placement(&s.window);
    }
    app.surface = None;
    ran.map_err(|why| format!("The match stopped: {why}"))?;
    let elapsed = app.started.elapsed().as_secs_f64();
    println!(
        "{} frames in {elapsed:.1} s ({:.1} fps)",
        app.frames,
        app.frames as f64 / elapsed
    );
    if let Some(bench) = &app.bench {
        let size = app.surface_size;
        print!("{}", bench.report(size));
        if let Some(out) = &bench.out {
            match std::fs::write(out, serde_json::to_string_pretty(&bench.json(size)).unwrap_or_default()) {
                Ok(()) => println!("bench: wrote {}", out.display()),
                Err(e) => eprintln!("bench: {}: {e}", out.display()),
            }
        }
    }
    match app.error {
        // Before a frame was drawn it never started (the window would not open).
        Some(why) if app.frames == 0 => Err(why),
        Some(why) => Err(format!("The match stopped: {why}")),
        None => Ok(0),
    }
}

/// The drawn players' ids by name, as the viewer cycles them
/// (`playerOrder`: `name.localeCompare`, then id).
fn name_order(
    roster: &[(String, PlayerState)],
    drawn: &[(String, InterpolatedEntity, PlayerModelVisualStyle)],
) -> Vec<String> {
    let name = |id: &str| {
        roster
            .iter()
            .find(|(_, p)| p.entity_id == id && !p.name.is_empty())
            .map_or_else(|| id.to_lowercase(), |(_, p)| p.name.to_lowercase())
    };
    let mut ids: Vec<&str> = drawn.iter().map(|(id, _, _)| id.as_str()).collect();
    ids.sort_by(|a, b| locale_compare(&name(a), &name(b)).then(a.cmp(b)));
    ids.into_iter().map(str::to_owned).collect()
}
