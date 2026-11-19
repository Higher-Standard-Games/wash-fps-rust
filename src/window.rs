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
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
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
use washoi::client::quad_damage_visual::mask_has_quad;
use washoi::client::remote_player_logic::CLIENT_KEY_PREFIX;
use washoi::client::settings::controller_model::controller_buttons;
use washoi::client::spectate_view::{DeathCameraCue, SpectateView};
use washoi::client::teleport_effect::TeamColor;
use washoi::client::washoi_style::install_style;
use washoi::client::weapon_view::{clip_reload_time, quad_view_weapon_outline, shot_style};
use washoi::console::{ConsoleRequest, DemoConsole, GameConsole};
use washoi::shared::net::snapshot_events::SnapshotEvent;

mod input;
mod redraw;

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
    /// The pointing hand is up, over a post-game key.
    pointing: bool,
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
    /// The watched player changed since the view model last drew.
    followed_changed: bool,
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
                if let Some(s) = &mut self.surface
                    && resize_surface(&s.gpu, &s.surface, &mut s.config, size)
                {
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
                // A campaign mission not won, played again (`setRetryAction`).
                let retry = (state == ElementState::Pressed && !self.captured)
                    .then(|| {
                        let h = self.surface.as_ref()?.hud.as_ref()?;
                        let (x, y) = self.cursor?;
                        h.scoreboard
                            .retry_hit(x as f32 / h.scale, y as f32 / h.scale)
                            .map(str::to_owned)
                    })
                    .flatten();
                if let Some(mission) = &retry {
                    washoi::client::campaign::campaign_session::request_campaign_retry(mission);
                }
                // The post-game's way home (`setReturnAction`).
                let home = retry.is_some()
                    || state == ElementState::Pressed
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
            WindowEvent::CursorLeft { .. } => self.cursor = None,
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
            request_next_frame(event_loop, &s.window);
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
/// The match reached its post-game, so the way home opens Play rather than
/// the room (`ReturnToLobby.ts`). Set by the window, taken once per match.
static MATCH_COMPLETED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the match just left had reached its post-game; clears it.
pub fn take_match_completed() -> bool {
    MATCH_COMPLETED.swap(false, std::sync::atomic::Ordering::Relaxed)
}

pub fn end_screen(event_loop: &ActiveEventLoop) {
    SCREEN_ENDED.with(|e| e.set(true));
    #[cfg(not(target_os = "macos"))]
    event_loop.exit();
    #[cfg(target_os = "macos")]
    let _ = event_loop;
}

/// The modifier keys held now (`KeyboardEvent`'s `shiftKey`, `altKey`, …).
fn modifiers() -> ModifiersState {
    MODIFIERS.with(Cell::get)
}

thread_local! {
    /// The modifier keys held now, as winit last reported them
    /// (`ModifiersChanged`): Alt+Enter, and Shift for team chat.
    static MODIFIERS: Cell<ModifiersState> = const { Cell::new(ModifiersState::empty()) };
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
/// `r_frame_latency`: how many finished frames the driver may queue before
/// showing them. 2 (wgpu's own) keeps the GPU busy; 1 shows each frame about
/// one frame sooner, so aiming answers sooner, for a few frames a second on a
/// busy GPU.
pub fn frame_latency() -> u32 {
    with_cvars(|c| {
        if c.exists("r_frame_latency") {
            c.get_number("r_frame_latency").clamp(1.0, 3.0) as u32
        } else {
            2
        }
    })
}

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
            MODIFIERS.with(|a| a.set(m.state()));
        }
        // `InputSystem.onKeyDown`: Alt+Enter toggles fullscreen, on every
        // screen, and goes no further.
        if let WindowEvent::KeyboardInput { event: key, .. } = &event
            && key.state == ElementState::Pressed
            && matches!(
                key.physical_key,
                PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter)
            )
            && modifiers().alt_key()
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
    let skipped = |at: Option<Instant>| FRAME_SKIPPED_AT.with(|s| s.set(at));
    // Asking the window whether it is minimised is a round trip to the
    // X server; a 0 × 0 size has been told already.
    if WINDOW_EMPTY.with(Cell::get) {
        skipped(Some(Instant::now()));
        return Ok(None);
    }
    let texture = surface.get_current_texture();
    skipped(match &texture {
        wgpu::CurrentSurfaceTexture::Success(_) | wgpu::CurrentSurfaceTexture::Suboptimal(_) => None,
        _ => Some(Instant::now()),
    });
    match texture {
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

thread_local! {
    /// When the last frame was skipped because the window showed none;
    /// `None` once one is shown.
    static FRAME_SKIPPED_AT: Cell<Option<Instant>> = const { Cell::new(None) };
    /// The window is 0 × 0, as Windows makes a minimised one.
    static WINDOW_EMPTY: Cell<bool> = const { Cell::new(false) };
}

/// How long a screen waits to try again while its window shows no frames:
/// about one frame of a 60 Hz display.
const HIDDEN_RETRY: Duration = Duration::from_millis(16);

/// Asks for the next frame, as each screen does once its events are
/// handled. While the window shows none (minimised or covered: Metal says
/// `Occluded` at once, and a minimised window on Windows has no size), the
/// next try waits [`HIDDEN_RETRY`] instead of spinning a core on frames
/// that are never drawn. A match keeps running at that pace.
pub fn request_next_frame(event_loop: &ActiveEventLoop, window: &Window) {
    match FRAME_SKIPPED_AT.with(Cell::get) {
        Some(at) if at.elapsed() < HIDDEN_RETRY => {
            event_loop.set_control_flow(ControlFlow::WaitUntil(at + HIDDEN_RETRY));
        }
        _ => {
            event_loop.set_control_flow(ControlFlow::Poll);
            window.request_redraw();
        }
    }
}

/// A window's new size: configures its surface for it and returns true,
/// or false for a minimised window's 0 × 0, which no surface takes; the
/// frames are skipped until it comes back.
pub fn resize_surface(
    gpu: &Gpu,
    surface: &wgpu::Surface<'static>,
    config: &mut wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
) -> bool {
    let empty = size.width == 0 || size.height == 0;
    WINDOW_EMPTY.with(|e| e.set(empty));
    if empty {
        return false;
    }
    config.width = size.width;
    config.height = size.height;
    surface.configure(&gpu.device, config);
    true
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

/// Opens the next window at `width` × `height` physical pixels unless a
/// screen already left one: `washoi bench --size`, so a run measures the
/// size it reports.
pub fn request_size(width: u32, height: u32) {
    PLACEMENT.with(|p| {
        if p.get().is_none() {
            p.set(Some(Placement {
                size: PhysicalSize::new(width, height),
                position: None,
                maximized: false,
                fullscreen: false,
            }));
        }
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
    // A kept window may still be minimised from the screen before.
    WINDOW_EMPTY.with(|e| e.set(size.width == 0 || size.height == 0));
    let mut config = surface
        .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
        .ok_or("the surface does not fit this adapter")?;
    // Babylon writes display-space bytes as they are: prefer a non-sRGB target.
    let formats = surface.get_capabilities(&gpu.adapter).formats;
    if let Some(&plain) = formats.iter().find(|f| !f.is_srgb()) {
        config.format = plain;
    }
    config.present_mode = wgpu::PresentMode::AutoVsync;
    config.desired_maximum_frame_latency = frame_latency();
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
        pointing: false,
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
        followed_changed: false,
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
        overlay: ReelOverlay,
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

/// `presentDemoEnds`' final scores: every player's from the scoreboard, and
/// each side's where the mode has sides.
pub(crate) fn demo_final(state: Option<&washoi::client::hud_state::HudState>) -> washoi::client::reel_cards::DemoFinal {
    use washoi::client::reel_cards::{DemoFinal, Standing};
    let Some(state) = state else {
        return DemoFinal::default();
    };
    let spectator = |team: &str| team == "spectator";
    DemoFinal {
        players: state
            .scoreboard
            .iter()
            .filter(|e| !e.team_id.as_deref().is_some_and(spectator))
            .map(|e| Standing {
                name: e.name.clone(),
                score: e.score,
            })
            .collect(),
        sides: state.team_based.then(|| {
            state
                .team_scores
                .iter()
                .filter(|(team, _)| !spectator(team))
                .map(|(team, score)| Standing {
                    name: state.team_names.get(team).cloned().unwrap_or_else(|| team.clone()),
                    score: *score,
                })
                .collect()
        }),
        frags: 0,
        duration_ms: 0.0,
    }
}
