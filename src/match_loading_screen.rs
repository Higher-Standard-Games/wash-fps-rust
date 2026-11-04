//! The match loading screen: wash-fps `client/MatchMapLoader.ts`'s hold on
//! SHAG's `MapLoadingOverlay`, shown with `TransferLoadingView`
//! (`shagger::ui::transfer_loading_view`).
//!
//! The lobby page shows the match handoff until it asks for the match; this
//! screen takes over in the same window, the handoff first (connecting to
//! the game server), then the map's load in its place without a fade, as
//! the browser does. The connection, the map and the sound are made on a
//! thread of their own ([`view::prepare`]) while the screen keeps drawing.
//! The renderer is built in the match window itself, so the screen's last
//! frame stays up over that build ([`draw_curtain`]) until the first frame
//! of the match replaces it: the curtain comes up when the game is ready,
//! not just the map.
//!
//! The map's levelshot comes from the region's site
//! (`/data/levelshots/<map>.jpg`, as the browser's), kept in the cache.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use shagger::network::client::http_fetch::{FetchEvent, HttpFetch};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;
use shagger::ui::components::connecting_overlay::{ConnectingOverlay, paint_twin};
use shagger::ui::components::lobby_twin::{TwinRail, TwinStyle};
use shagger::ui::egui_layer::{EguiLayer, egui};
use shagger::ui::transfer_loading_view::{LoadingComplete, LoadingProgress, LoadingStart, TransferLoadingView};
use washoi::client::washoi_style::{display_family, install_style};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::view::{self, PrepareStep, Prepared, ViewOptions};
use crate::window::{OpenWindow, next_frame, open_window, present, run_on_event_loop};

/// The stage's blue, behind everything (`.twin`'s first stop).
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.026,
    g: 0.048,
    b: 0.105,
    a: 1.0,
};
/// How long a failure stays up before the lobby (`ConnectingOverlay`'s 3 s).
const FAILURE_SHOWN: Duration = Duration::from_secs(3);
const LEVELSHOT_IDLE: Duration = Duration::from_secs(5);
const LEVELSHOT_TOTAL: Duration = Duration::from_secs(10);

/// How the screen ended.
pub enum Loaded {
    /// Everything the match window needs.
    Ready(Box<Prepared>),
    /// Why the match could not start, for the lobby to show.
    Failed(String),
    /// The player pressed Esc: back to the lobby.
    Back,
    /// The window closed.
    Quit,
}

/// The match being joined, for the handoff: the game server, and whether
/// the player only watches.
pub struct Handoff {
    pub server_url: String,
    pub spectator: bool,
}

/// What the loader thread sends.
enum Message {
    Step(PrepareStep),
    Done(Result<Box<Prepared>, String>),
}

/// The screen's content: the handoff until the server names its map, then
/// the map's load.
#[derive(Clone)]
enum Screen {
    Handoff(ConnectingOverlay),
    Loading(TransferLoadingView),
}

impl Screen {
    fn parts(
        &mut self,
    ) -> (
        &TwinRail,
        &mut shagger::ui::components::connecting_overlay::OverlayStage,
    ) {
        match self {
            Self::Handoff(h) => (&h.rail, &mut h.stage),
            Self::Loading(v) => (&v.rail, &mut v.stage),
        }
    }
}

/// The game's look for the twin.
#[must_use]
pub fn twin_style() -> TwinStyle {
    TwinStyle {
        brand: "WASHOI".into(),
        display: display_family(),
        body: egui::FontFamily::Proportional,
        engine: "SHAGGER".into(),
        version: format!("v{}", washoi::VERSION),
    }
}

/// `https://eu.washoi.net/data/levelshots` for `wss://eu.washoi.net/lobby`.
#[must_use]
pub fn levelshot_base(lobby_url: &str) -> String {
    let (scheme, rest) = match lobby_url.split_once("://") {
        Some(("wss" | "https", rest)) => ("https", rest),
        Some((_, rest)) => ("http", rest),
        None => ("http", lobby_url),
    };
    let host = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{host}/data/levelshots")
}

/// A map name that is safe in a URL and a file name.
fn safe_map(map: &str) -> bool {
    !map.is_empty() && map.len() <= 64 && map.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn levelshot_cache(map: &str) -> Option<PathBuf> {
    washoi::cache_dir().map(|d| d.join("levelshots").join(format!("{map}.jpg")))
}

fn decode_levelshot(bytes: &[u8]) -> Option<egui::ColorImage> {
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
}

/// The levelshot: from the cache, or fetched and then kept.
enum Levelshot {
    None,
    Fetching(String, HttpFetch),
    Ready(Arc<egui::ColorImage>),
}

impl Levelshot {
    fn start(base: Option<&str>, map: &str) -> Self {
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

    fn poll(&mut self) {
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

/// The screen as it last stood, its levelshot, and its clock.
type Curtain = (Screen, Option<Arc<egui::ColorImage>>, f64);

thread_local! {
    /// The screen as it last stood, for the match window to draw while it
    /// builds its renderer ([`draw_curtain`]).
    static CURTAIN: RefCell<Option<Curtain>> = const { RefCell::new(None) };
}

struct LoadingWindow {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    layer: EguiLayer,
    shot: Option<egui::TextureHandle>,
}

struct LoadingApp {
    screen: Screen,
    networked: bool,
    events: Receiver<Message>,
    levelshots: Option<String>,
    levelshot: Levelshot,
    started: Instant,
    /// When the map's load began, for "Map loaded in".
    map_started: Option<Instant>,
    failed: Option<(String, Instant)>,
    open: Option<LoadingWindow>,
    outcome: Option<Loaded>,
    error: Option<String>,
}

impl LoadingApp {
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn take_events(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(Message::Step(step)) => self.step(step),
                Ok(Message::Done(Ok(prepared))) => {
                    if let Screen::Loading(view) = &mut self.screen {
                        let duration = self.map_started.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
                        view.complete(
                            &LoadingComplete {
                                success: true,
                                error: None,
                                duration,
                            },
                            &["fps:postload"],
                        );
                    }
                    self.outcome = Some(Loaded::Ready(prepared));
                    return;
                }
                Ok(Message::Done(Err(why))) => {
                    self.fail(why);
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    // The loader panicked.
                    self.fail("The match could not be prepared.".into());
                    return;
                }
            }
        }
    }

    fn step(&mut self, step: PrepareStep) {
        match step {
            PrepareStep::Connected { map } => {
                self.levelshot = Levelshot::start(self.levelshots.as_deref(), &map);
                let start = LoadingStart {
                    id: map,
                    display_name: None,
                };
                let handoff = match &mut self.screen {
                    Screen::Handoff(h) if self.networked => {
                        let spectating = h.rail.context == "Live channel";
                        h.set_status(if spectating { "SPECTATING" } else { "CONNECTED" });
                        Some(h.clone())
                    }
                    _ => None,
                };
                self.screen = Screen::Loading(TransferLoadingView::mount(&start, self.networked, handoff));
                self.map_started = Some(Instant::now());
            }
            PrepareStep::Phase { phase, detail } => {
                if let Screen::Loading(view) = &mut self.screen {
                    view.progress(&LoadingProgress {
                        phase: phase.into(),
                        percent: None,
                        detail,
                    });
                }
            }
        }
    }

    fn fail(&mut self, why: String) {
        match &mut self.screen {
            Screen::Handoff(h) => {
                h.set_status("CONNECTION FAILED");
                h.show_error(&why);
            }
            Screen::Loading(view) => view.complete(
                &LoadingComplete {
                    success: false,
                    error: Some(why.clone()),
                    duration: 0.0,
                },
                &[],
            ),
        }
        self.failed = Some((why, Instant::now()));
    }

    fn redraw(&mut self) {
        if self.failed.is_none() {
            self.take_events();
        }
        if let Some((why, at)) = &self.failed
            && at.elapsed() >= FAILURE_SHOWN
        {
            self.outcome = Some(Loaded::Failed(why.clone()));
        }
        self.levelshot.poll();
        let now = self.now();
        let Some(m) = &mut self.open else { return };
        if m.shot.is_none()
            && let Levelshot::Ready(image) = &self.levelshot
        {
            m.shot = Some(
                m.layer
                    .ctx
                    .load_texture("levelshot", (**image).clone(), egui::TextureOptions::LINEAR),
            );
        }
        if let (Some(shot), Screen::Loading(view)) = (&m.shot, &mut self.screen) {
            view.stage.shot = Some((shot.id(), shot.size_vec2()));
        }
        if let Err(why) = paint(
            &m.window,
            &m.gpu,
            &mut m.surface,
            &m.config,
            &mut m.layer,
            &mut self.screen,
            now,
        ) {
            self.error = Some(why);
        }
    }
}

/// One frame of `screen` into a window's surface.
fn paint(
    window: &Arc<Window>,
    gpu: &Gpu,
    surface: &mut wgpu::Surface<'static>,
    config: &wgpu::SurfaceConfiguration,
    layer: &mut EguiLayer,
    screen: &mut Screen,
    now: f64,
) -> Result<(), String> {
    let Some(frame) = next_frame(gpu, window, surface, config)? else {
        return Ok(());
    };
    let view = frame
        .texture
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let style = twin_style();
    let ui = layer.run(window, |ui| {
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);
        let (rail, stage) = screen.parts();
        paint_twin(&painter, rect, &style, rail, stage, now);
    });
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("match loading"),
    });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("match loading clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(BACKGROUND),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    layer.paint(gpu, &mut encoder, &view, [config.width, config.height], ui);
    gpu.queue.submit([encoder.finish()]);
    present(gpu, window, surface, config, frame);
    Ok(())
}

fn loading_window(open: OpenWindow) -> LoadingWindow {
    let OpenWindow {
        window,
        gpu,
        surface,
        config,
    } = open;
    let layer = EguiLayer::new(&gpu, &window, config.format);
    install_style(&layer.ctx);
    LoadingWindow {
        window,
        gpu,
        surface,
        config,
        layer,
        shot: None,
    }
}

impl ApplicationHandler for LoadingApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.open.is_some() {
            return;
        }
        match open_window(event_loop) {
            Ok(open) => {
                open.window.set_title("Washoi");
                self.open = Some(loading_window(open));
            }
            Err(why) => {
                self.error = Some(why);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(m) = &mut self.open {
            m.layer.on_window_event(&m.window, &event);
        }
        let key = match &event {
            WindowEvent::KeyboardInput { event: key, .. } if key.state == ElementState::Pressed && !key.repeat => {
                Some(key.physical_key)
            }
            _ => None,
        };
        match event {
            WindowEvent::CloseRequested => {
                self.outcome = Some(Loaded::Quit);
            }
            // Esc leaves for the lobby (`MatchMapLoader.cancel`); on a
            // failure, any key goes on to it at once.
            _ if key == Some(PhysicalKey::Code(KeyCode::Escape)) && self.failed.is_none() => {
                self.outcome = Some(Loaded::Back);
            }
            _ if key.is_some() && self.failed.is_some() => {
                let why = self.failed.as_ref().map(|(w, _)| w.clone()).unwrap_or_default();
                self.outcome = Some(Loaded::Failed(why));
            }
            WindowEvent::Resized(size) => {
                if let Some(m) = &mut self.open {
                    m.config.width = size.width.max(1);
                    m.config.height = size.height.max(1);
                    m.surface.configure(&m.gpu.device, &m.config);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
        if self.outcome.is_some() || self.error.is_some() {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(m) = &self.open {
            m.window.request_redraw();
        }
    }
}

/// The screen until the match in `o` is ready to open (or failed, or the
/// player left). `handoff` is the match being joined; `None` for a demo,
/// a local map. `lobby_url` names the site the levelshots come from.
///
/// # Errors
/// No window could open.
pub fn run(o: Arc<ViewOptions>, handoff: Option<Handoff>, lobby_url: Option<&str>) -> Result<Loaded, String> {
    let (tx, events) = channel();
    let networked = handoff.is_some();
    std::thread::spawn(move || {
        let step = tx.clone();
        let result = view::prepare(&o, &mut |s| {
            let _ = step.send(Message::Step(s));
        });
        // Whoever left the screen first drops what arrives.
        let _ = tx.send(Message::Done(result.map(Box::new)));
    });
    let screen = match &handoff {
        Some(h) => {
            let mut overlay = ConnectingOverlay::new(&h.server_url, h.spectator);
            // The page's handoff got this far before it asked for the match.
            overlay.set_status("CONNECTING");
            Screen::Handoff(overlay)
        }
        // A demo or a local map: its screen from the start, named once
        // the file says which map it is.
        None => Screen::Loading(TransferLoadingView::mount(
            &LoadingStart {
                id: String::new(),
                display_name: None,
            },
            false,
            None,
        )),
    };
    let mut app = LoadingApp {
        screen,
        networked,
        events,
        levelshots: lobby_url.map(levelshot_base),
        levelshot: Levelshot::None,
        started: Instant::now(),
        map_started: None,
        failed: None,
        open: None,
        outcome: None,
        error: None,
    };
    let ran = run_on_event_loop(&mut app);
    if let Some(m) = &app.open {
        crate::window::remember_placement(&m.window);
    }
    app.open = None;
    ran?;
    if let Some(why) = app.error {
        return Err(why);
    }
    let outcome = app.outcome.unwrap_or(Loaded::Quit);
    if matches!(outcome, Loaded::Ready(_)) {
        let shot = match &app.levelshot {
            Levelshot::Ready(image) => Some(image.clone()),
            _ => None,
        };
        let now = app.started.elapsed().as_secs_f64();
        CURTAIN.with(|c| *c.borrow_mut() = Some((app.screen, shot, now)));
    }
    Ok(outcome)
}

/// Draws the loading screen as it last stood into the match window, before
/// the renderer is built there; nothing when no loading screen ran.
pub fn draw_curtain(
    window: &Arc<Window>,
    gpu: &Gpu,
    surface: &mut wgpu::Surface<'static>,
    config: &wgpu::SurfaceConfiguration,
) {
    let Some((mut screen, shot, now)) = CURTAIN.with(|c| c.borrow_mut().take()) else {
        return;
    };
    let mut layer = EguiLayer::new(gpu, window, config.format);
    install_style(&layer.ctx);
    let _texture = shot.map(|image| {
        let texture = layer
            .ctx
            .load_texture("levelshot", (*image).clone(), egui::TextureOptions::LINEAR);
        if let Screen::Loading(view) = &mut screen {
            view.stage.shot = Some((texture.id(), texture.size_vec2()));
        }
        texture
    });
    // Twice: egui settles its layout on the frame after the first.
    for _ in 0..2 {
        if let Err(why) = paint(window, gpu, surface, config, &mut layer, &mut screen, now) {
            eprintln!("loading screen: {why}");
            return;
        }
    }
}

/// `washoi loading <map> --screenshot FILE.png`: the screen part way into
/// `map`'s load after a match handoff, `seconds` in, with the levelshot in
/// `levelshot` when given.
///
/// # Errors
/// No GPU, or the file could not be written.
pub fn screenshot(
    map: &str,
    levelshot: Option<&Path>,
    path: &Path,
    size: (u32, u32),
    seconds: f64,
) -> Result<(), String> {
    use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};
    let (width, height) = size;
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let mut layer = EguiLayer::offscreen(&gpu, OFFSCREEN_FORMAT);
    install_style(&layer.ctx);
    let mut handoff = ConnectingOverlay::new("wss://eu.washoi.net/game/1", false);
    handoff.set_status("CONNECTING");
    handoff.set_status("CONNECTED");
    let mut view = TransferLoadingView::mount(
        &LoadingStart {
            id: map.into(),
            display_name: None,
        },
        true,
        Some(handoff),
    );
    for (phase, detail) in [
        ("Dependencies", Some("Reading the packages".to_owned())),
        ("Geometry", Some(format!("maps/{map}.bsp"))),
        ("Collision", None),
    ] {
        view.progress(&LoadingProgress {
            phase: phase.into(),
            percent: None,
            detail,
        });
    }
    let shot = levelshot
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| decode_levelshot(&b))
        .map(|image| layer.ctx.load_texture("levelshot", image, egui::TextureOptions::LINEAR));
    view.stage.shot = shot.as_ref().map(|t| (t.id(), t.size_vec2()));
    let mut screen = Screen::Loading(view);
    let style = twin_style();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width as f32, height as f32),
        )),
        ..Default::default()
    };
    // The first frame starts the animations; the second is `seconds` on.
    let mut pixels = Vec::new();
    for t in [0.0, seconds] {
        let frame = layer.run_input(input(), |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            let (rail, stage) = screen.parts();
            paint_twin(&painter, rect, &style, rail, stage, t);
        });
        pixels = render_encoded_to_rgba(&gpu, width, height, BACKGROUND, |encoder, target| {
            layer.paint(&gpu, encoder, target, [width, height], frame);
        });
    }
    image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    println!("loading: saved {}", path.display());
    Ok(())
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
