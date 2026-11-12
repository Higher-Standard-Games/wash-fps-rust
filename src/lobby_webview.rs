//! The lobby as the browser shows it: the region's `/client` page in the
//! system webview, a child of the game window (docs/research/WEBVIEW_LOBBY.md).
//!
//! The page keeps the lobby connection, the account and the settings sync.
//! When it would start a match it asks over the bridge (`match:play`,
//! `match:watch`); this screen answers, hides the page and hands the choice
//! to the match, which runs in the same window. The page stays loaded,
//! hidden, until the match ends and this screen shows it again with
//! `match:ended`.
//!
//! When the page does not come up (no network, an old deploy without the
//! bridge, no webview on this machine) the screen says why and offers Retry
//! and Quit. There is no native lobby behind it.

use std::cell::{Cell, RefCell};
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use shagger::core::cvar::with_cvars;
use shagger::lobby::storage_mirror::StorageMirror;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;
use shagger::ui::egui_layer::{EguiLayer, egui};
use shagger::webview::{
    HostEvent, WebviewConfig, WebviewHost, open_external, origin_of, safe_file_name, save_download,
};
use washoi::client::demo_download::{DemoDownload, DemoDownloadState};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::window::{OpenWindow, next_frame, open_window, present, run_on_event_loop, toggle_fullscreen};

/// The bridge version this client speaks (WEBVIEW_LOBBY.md 5).
pub const BRIDGE_VERSION: u32 = 1;

/// What the client offers the page, for `hello`.
const CAPABILITIES: [&str; 8] = [
    "match",
    "external",
    "fullscreen",
    "quit",
    "storage",
    "demo",
    "file-save",
    "update",
];

/// How long the page has to say hello before the screen gives up on it.
const HELLO_TIMEOUT: Duration = Duration::from_secs(30);

/// The page's global.
const GLOBAL: &str = "__washoiNative";

/// NVIDIA's proprietary kernel module is loaded (`main` turns WebKitGTK's
/// DMA-BUF renderer off for it).
#[cfg(target_os = "linux")]
pub fn nvidia_driver_loaded() -> bool {
    std::path::Path::new("/proc/driver/nvidia/version").exists()
}

/// With the DMA-BUF renderer off (`main`'s `webkit_environment`, or the
/// player's own `WEBKIT_DISABLE_DMABUF_RENDERER`), WebKitGTK copies every
/// frame and has been seen drawing the lobby page at a few frames a second;
/// `backdrop-filter` blurs are the page's most expensive effect. There the
/// page goes without them; its panels keep their tinted backgrounds, as
/// with `hud_blur 0` (`HudBlur.ts`).
const NO_BLUR: &str = r#"document.addEventListener("DOMContentLoaded", () => {
  const style = document.createElement("style");
  style.id = "washoi-native-no-blur";
  style.textContent = "* { backdrop-filter: none !important; -webkit-backdrop-filter: none !important; }";
  document.head.appendChild(style);
});"#;

/// The page's extra script: [`NO_BLUR`] on WebKitGTK's copying path.
fn extra_script() -> String {
    let copying = cfg!(target_os = "linux")
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some_and(|v| !v.is_empty() && v != "0");
    if copying { NO_BLUR.to_owned() } else { String::new() }
}

/// Behind the page: Washoi's night blue.
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.035,
    g: 0.03,
    b: 0.07,
    a: 1.0,
};

thread_local! {
    /// The page, alive from the first lobby to the end of the process, and
    /// never dropped: on macOS a quit from the Dock or Cmd+Q ends the process
    /// inside the event loop, and the web view's teardown in the thread-local
    /// destructor then aborts, which macOS reports as a crash.
    static HOST: RefCell<Option<ManuallyDrop<WebviewHost>>> = const { RefCell::new(None) };
    /// The page said hello at least once.
    static GREETED: Cell<bool> = const { Cell::new(false) };
    /// The page has been told of the newer release (`native:update`).
    static UPDATE_TOLD: Cell<bool> = const { Cell::new(false) };
    /// The mirrored keys as the last match started, to tell the page what
    /// the match changed (`storage:changed`).
    static BEFORE_MATCH: RefCell<Option<Map<String, Value>>> = const { RefCell::new(None) };
    /// The page can show its settings over a match (`settings:ready`).
    static SETTINGS_READY: Cell<bool> = const { Cell::new(false) };
    /// The settings files, for what the page changes over a match.
    static MIRROR: RefCell<Option<StorageMirror>> = const { RefCell::new(None) };
}

/// How the last match ended, for the page.
#[derive(Debug, Clone, Default)]
pub struct AfterMatch {
    /// Why it did not start or broke off; `None` when the player went back.
    pub error: Option<String>,
    /// It was a recording from the Demos page, not a match: the page is
    /// shown again as it was (`demo:ended`), not reloaded.
    pub demo: bool,
}

/// Where the lobby page sent this player.
#[derive(Debug, Clone, PartialEq)]
pub struct LobbyChoice {
    pub server_url: String,
    pub map: String,
    pub admission_ticket: Option<String>,
    /// The account's name, to join the game server as.
    pub name: String,
    /// True to play; false to watch.
    pub play: bool,
    /// A recording to play instead (the Demos page), saved on disk.
    pub demo: Option<std::path::PathBuf>,
    /// Its highlight reel instead, about this many minutes long.
    pub reel: Option<f64>,
    /// The side this player took in the room (`alpha` or `bravo`), for the
    /// game server to honour on join; `None` in a mode without teams.
    pub team: Option<String>,
}

/// What the lobby decided.
#[derive(Debug)]
pub enum LobbyOutcome {
    Quit,
    /// A match, or a recording (`demo` set) the page asked for.
    Match(LobbyChoice),
}

/// The `/client` page for a region's lobby WebSocket address:
/// `wss://eu.washoi.net/lobby` is `https://eu.washoi.net/client`.
/// `WASHOI_LOBBY_PAGE` overrides it (a local SHAG dev server).
#[must_use]
pub fn page_url(lobby_url: &str) -> Option<String> {
    if let Ok(page) = std::env::var("WASHOI_LOBBY_PAGE")
        && !page.is_empty()
    {
        return Some(page);
    }
    let (scheme, rest) = lobby_url.split_once("://")?;
    let scheme = match scheme {
        "wss" => "https",
        "ws" => "http",
        _ => return None,
    };
    let host = rest.split('/').next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}/client"))
}

/// The answer to a page's request, carrying its `id`.
fn reply(request: &Value, kind: &str, mut fields: Value) -> Value {
    if let Some(obj) = fields.as_object_mut() {
        obj.insert("v".into(), BRIDGE_VERSION.into());
        obj.insert("type".into(), kind.into());
        if let Some(id) = request.get("id") {
            obj.insert("id".into(), id.clone());
        }
    }
    fields
}

fn text(message: &Value, field: &str) -> Option<String> {
    message.get(field).and_then(Value::as_str).map(str::to_owned)
}

/// A `match:play` or `match:watch` request as the match's choice, or why not.
pub fn match_choice(message: &Value) -> Result<LobbyChoice, String> {
    let play = text(message, "type").as_deref() == Some("match:play");
    let server_url = text(message, "serverUrl").ok_or("no serverUrl")?;
    if !server_url.starts_with("ws://") && !server_url.starts_with("wss://") {
        return Err(format!("not a game server: {server_url}"));
    }
    let admission_ticket = text(message, "admissionTicket");
    if play && admission_ticket.is_none() {
        return Err("playing needs an admissionTicket".into());
    }
    Ok(LobbyChoice {
        server_url,
        map: text(message, "map").unwrap_or_default(),
        admission_ticket,
        name: text(message, "name").unwrap_or_else(|| "Player".into()),
        play,
        demo: None,
        reel: None,
        team: text(message, "team"),
    })
}

/// What one message from the page asks of the screen.
#[derive(Debug)]
enum Action {
    Reply(Value),
    Quit,
    Fullscreen,
    External(String),
    Match(Box<LobbyChoice>, Value),
    /// `storage:set`: a mirrored key and its new value (`None`: removed).
    Storage(String, Option<String>),
    /// `demo:play`: the request, where recordings are served, the file and
    /// the reel length.
    Demo(Value, String, String, Option<f64>),
    /// `demo:cancel`.
    CancelDemo,
    Nothing,
}

fn act(message: &Value) -> Action {
    let kind = text(message, "type").unwrap_or_default();
    match kind.as_str() {
        "hello" => Action::Reply(reply(
            message,
            "hello",
            json!({
                "clientVersion": washoi::VERSION,
                "bridgeVersion": BRIDGE_VERSION,
                "capabilities": CAPABILITIES,
            }),
        )),
        "open:external" => text(message, "url").map_or(Action::Nothing, Action::External),
        "app:quit" => Action::Quit,
        "window:fullscreen" => Action::Fullscreen,
        "demo:play" => match (text(message, "base"), text(message, "filename")) {
            (Some(base), Some(file)) if base.starts_with("https://") || base.starts_with("http://") => {
                let reel = message.get("reel").and_then(Value::as_f64).filter(|m| *m > 0.0);
                Action::Demo(message.clone(), base.trim_end_matches('/').to_owned(), file, reel)
            }
            _ => Action::Reply(reply(
                message,
                "error",
                json!({"code": "bad_request", "message": "demo:play needs an http(s) base and a filename"}),
            )),
        },
        "demo:cancel" => Action::CancelDemo,
        "update:dismiss" => {
            washoi::update_check::dismiss();
            Action::Nothing
        }
        "file:save" => match text(message, "text") {
            Some(body) => {
                let name = safe_file_name(&text(message, "suggestedName").unwrap_or_default(), "washoi-file.txt");
                Action::Reply(match save_download(&name, body.as_bytes()) {
                    Ok(path) => reply(
                        message,
                        "file:save",
                        json!({"saved": true, "path": path.display().to_string()}),
                    ),
                    Err(why) => reply(
                        message,
                        "error",
                        json!({"code": "save_failed", "message": why.to_string()}),
                    ),
                })
            }
            None => Action::Reply(reply(
                message,
                "error",
                json!({"code": "bad_request", "message": "file:save needs text"}),
            )),
        },
        "storage:set" => match (text(message, "key"), message.get("value")) {
            (Some(key), Some(Value::String(v))) => Action::Storage(key, Some(v.clone())),
            (Some(key), Some(Value::Null) | None) => Action::Storage(key, None),
            _ => Action::Nothing,
        },
        "match:play" | "match:watch" => match match_choice(message) {
            Ok(choice) => Action::Match(Box::new(choice), reply(message, "ok", json!({}))),
            Err(why) => Action::Reply(reply(message, "error", json!({"code": "bad_request", "message": why}))),
        },
        _ if message.get("id").is_some() => Action::Reply(reply(
            message,
            "error",
            json!({"code": "unknown_type", "message": format!("this client does not know {kind}")}),
        )),
        _ => Action::Nothing,
    }
}

struct Open {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    layer: EguiLayer,
}

/// A recording the page asked for, downloading.
struct PendingDemo {
    request: Value,
    download: DemoDownload,
    reel: Option<f64>,
}

struct LobbyApp {
    url: String,
    mirror: StorageMirror,
    demo: Option<PendingDemo>,
    after: Option<AfterMatch>,
    open: Option<Open>,
    outcome: Option<LobbyOutcome>,
    /// Why the page is not up, shown instead of it.
    problem: Option<String>,
    waiting_since: Instant,
    error: Option<String>,
}

impl LobbyApp {
    fn config(&self) -> WebviewConfig {
        let origin = origin_of(&self.url).unwrap_or_default();
        WebviewConfig {
            url: self.url.clone(),
            allowed_origins: vec![origin],
            global: GLOBAL.into(),
            info: json!({
                "shell": "washoi-native",
                "bridgeVersion": BRIDGE_VERSION,
                "clientVersion": washoi::VERSION,
                "platform": std::env::consts::OS,
                "capabilities": CAPABILITIES,
                // The page applies these once per webview session; after
                // that its own storage is current (WEBVIEW_LOBBY.md 8).
                "storage": self.mirror.snapshot(),
                // The client's cvars, for the page to register the ones
                // only the renderer declares (the Video page's).
                "cvars": with_cvars(|c| c.declarations()),
            }),
            extra_script: extra_script(),
            data_dir: washoi::cache_dir().map(|d| d.join("webview")),
            devtools: cfg!(debug_assertions),
        }
    }

    fn with_host<R>(f: impl FnOnce(&mut WebviewHost) -> R) -> Option<R> {
        HOST.with(|h| h.borrow_mut().as_mut().map(|host| f(host)))
    }

    fn handle(&mut self, event_loop: &ActiveEventLoop) {
        let events = Self::with_host(|h| h.drain()).unwrap_or_default();
        for event in events {
            match event {
                HostEvent::Message(message) => {
                    if text(&message, "type").as_deref() == Some("hello") {
                        if !GREETED.with(Cell::get) {
                            let web = text(&message, "webVersion").unwrap_or_default();
                            println!("lobby: the page said hello (web {web})");
                        }
                        GREETED.with(|g| g.set(true));
                        // A new page load says again what it can do.
                        SETTINGS_READY.with(|r| r.set(false));
                        // A new page load: it has not seen the update notice.
                        UPDATE_TOLD.with(|t| t.set(false));
                        self.problem = None;
                    }
                    if text(&message, "type").as_deref() == Some("settings:ready") {
                        SETTINGS_READY.with(|r| r.set(true));
                    }
                    match act(&message) {
                        Action::Reply(answer) => {
                            Self::with_host(|h| h.send(&answer));
                        }
                        Action::Quit => {
                            self.outcome = Some(LobbyOutcome::Quit);
                            event_loop.exit();
                        }
                        Action::Fullscreen => toggle_fullscreen(),
                        Action::External(url) => {
                            if !open_external(&url) {
                                eprintln!("lobby: not opening {url}");
                            }
                        }
                        Action::Storage(key, value) => {
                            if !self.mirror.apply(&key, value.as_deref()) {
                                eprintln!("lobby: not keeping {key}");
                            }
                        }
                        Action::Demo(request, base, file, reel) => {
                            if let Some(old) = self.demo.take() {
                                Self::refuse(&old.request, "cancelled", "Another recording was picked.");
                            }
                            let download = DemoDownload::start(&base, &file);
                            self.demo = Some(PendingDemo {
                                request,
                                download,
                                reel,
                            });
                        }
                        Action::CancelDemo => {
                            if let Some(mut old) = self.demo.take() {
                                old.download.cancel();
                                Self::refuse(&old.request, "cancelled", "Cancelled.");
                            }
                        }
                        Action::Match(choice, answer) => {
                            BEFORE_MATCH.with(|b| *b.borrow_mut() = Some(self.mirror.snapshot()));
                            Self::with_host(|h| {
                                h.send(&answer);
                                h.send(&json!({"v": BRIDGE_VERSION, "type": "match:started"}));
                                h.hide();
                            });
                            self.outcome = Some(LobbyOutcome::Match(*choice));
                            event_loop.exit();
                            return;
                        }
                        Action::Nothing => {}
                    }
                }
                HostEvent::Loaded(url) => println!("lobby: page loaded {url}"),
                HostEvent::External(url) => println!("lobby: opened {url} in the browser"),
                HostEvent::Malformed(text) => eprintln!("lobby: page sent {text}"),
            }
        }
        if self.poll_demo(event_loop) {
            return;
        }
        // A newer release, once the page can show it (its update notice).
        if GREETED.with(Cell::get)
            && !UPDATE_TOLD.with(Cell::get)
            && let Some(update) = washoi::update_check::available()
        {
            UPDATE_TOLD.with(|t| t.set(true));
            let told =
                json!({"v": BRIDGE_VERSION, "type": "native:update", "version": update.version, "url": update.url});
            Self::with_host(|h| h.send(&told));
        }
        if !GREETED.with(Cell::get) && self.problem.is_none() && self.waiting_since.elapsed() > HELLO_TIMEOUT {
            self.problem = Some(format!("The lobby at {} did not start.", self.url));
            Self::with_host(WebviewHost::hide);
        }
    }

    fn refuse(request: &Value, code: &str, message: &str) {
        let answer = reply(request, "error", json!({"code": code, "message": message}));
        Self::with_host(|h| h.send(&answer));
    }

    /// The page's recording: true once it is in and the screen hands it on.
    fn poll_demo(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(pending) = &mut self.demo else {
            return false;
        };
        pending.download.poll();
        let path = match &pending.download.state {
            DemoDownloadState::Loading { .. } => return false,
            DemoDownloadState::Failed(why) => {
                let why = why.clone();
                if let Some(p) = self.demo.take() {
                    Self::refuse(&p.request, "download_failed", &why);
                }
                return false;
            }
            DemoDownloadState::Ready(path) => path.clone(),
        };
        let Some(p) = self.demo.take() else { return false };
        BEFORE_MATCH.with(|b| *b.borrow_mut() = Some(self.mirror.snapshot()));
        Self::with_host(|h| {
            h.send(&reply(&p.request, "ok", json!({})));
            h.send(&json!({"v": BRIDGE_VERSION, "type": "demo:started"}));
            h.hide();
        });
        self.outcome = Some(LobbyOutcome::Match(LobbyChoice {
            server_url: String::new(),
            map: path.display().to_string(),
            admission_ticket: None,
            name: String::new(),
            play: false,
            demo: Some(path),
            reel: p.reel,
            team: None,
        }));
        event_loop.exit();
        true
    }

    fn retry(&mut self) {
        self.problem = None;
        self.waiting_since = Instant::now();
        GREETED.with(|g| g.set(false));
        Self::with_host(|h| {
            h.load(&self.url);
            h.show();
        });
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        self.handle(event_loop);
        if self.outcome.is_some() {
            return;
        }
        let Some(m) = &mut self.open else { return };
        let frame = match next_frame(&m.gpu, &m.window, &mut m.surface, &m.config) {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(why) => {
                self.error = Some(why);
                event_loop.exit();
                return;
            }
        };
        let view = frame
            .texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut retry = false;
        let mut quit = false;
        let problem = self.problem.clone();
        let ui = m.layer.run(&m.window, |ui| {
            if let Some(problem) = &problem {
                problem_screen(ui, problem, &mut retry, &mut quit);
            }
        });
        let mut encoder = m
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("lobby") });
        clear(&mut encoder, &view);
        m.layer
            .paint(&m.gpu, &mut encoder, &view, [m.config.width, m.config.height], ui);
        m.gpu.queue.submit([encoder.finish()]);
        present(&m.gpu, &m.window, &mut m.surface, &m.config, frame);
        if retry {
            self.retry();
        }
        if quit {
            self.outcome = Some(LobbyOutcome::Quit);
            event_loop.exit();
        }
    }
}

/// Why the lobby is not showing, and the way on.
fn problem_screen(ui: &mut egui::Ui, problem: &str, retry: &mut bool, quit: &mut bool) {
    let cream = egui::Color32::from_rgb(0xf6, 0xec, 0xd2);
    let gold = egui::Color32::from_rgb(0xff, 0xc2, 0x1a);
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.label(egui::RichText::new("NO LOBBY").size(40.0).strong().color(gold));
            ui.add_space(12.0);
            ui.label(egui::RichText::new(problem).size(18.0).color(cream));
            ui.label(
                egui::RichText::new("Check the connection, then try again.")
                    .size(15.0)
                    .color(cream.gamma_multiply(0.7)),
            );
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 220.0).max(0.0) / 2.0);
                if ui.add_sized([100.0, 36.0], egui::Button::new("RETRY")).clicked() {
                    *retry = true;
                }
                if ui.add_sized([100.0, 36.0], egui::Button::new("QUIT")).clicked() {
                    *quit = true;
                }
            });
        });
    });
}

fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("lobby clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
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
}

impl ApplicationHandler for LobbyApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.open.is_some() {
            return;
        }
        let OpenWindow {
            window,
            gpu,
            surface,
            config,
        } = match open_window(event_loop) {
            Ok(open) => open,
            Err(why) => {
                self.error = Some(why);
                event_loop.exit();
                return;
            }
        };
        window.set_title("Washoi");
        window.set_cursor_visible(true);
        let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
        let size = window.inner_size();
        let made = HOST.with(|h| {
            let mut slot = h.borrow_mut();
            match slot.as_mut() {
                Some(host) => {
                    // What the hidden page said during the match is stale
                    // (it reloads on `match:ended`); the client offers no
                    // seat change yet (WEBVIEW_LOBBY.md 6).
                    drop(host.drain());
                    host.resize(size);
                    host.show();
                    Ok(())
                }
                None => {
                    WebviewHost::new(&*window, size, &self.config()).map(|host| *slot = Some(ManuallyDrop::new(host)))
                }
            }
        });
        if let Err(why) = made {
            self.problem = Some(format!("This computer has no web view for the lobby: {why}"));
        }
        if let Some(after) = self.after.take() {
            // What the match changed (the in-game settings, the console)
            // reaches the page's storage before it reloads.
            if let Some(before) = BEFORE_MATCH.with(|b| b.borrow_mut().take()) {
                let entries = self.mirror.changed_since(&before);
                if !entries.is_empty() {
                    Self::with_host(|h| {
                        h.send(&json!({"v": BRIDGE_VERSION, "type": "storage:changed", "entries": entries}))
                    });
                }
            }
            let kind = if after.demo { "demo:ended" } else { "match:ended" };
            let mut ended = json!({"v": BRIDGE_VERSION, "type": kind, "reason": "returned"});
            if let Some(why) = after.error {
                ended["reason"] = "disconnected".into();
                ended["message"] = why.into();
            }
            Self::with_host(|h| h.send(&ended));
        }
        let layer = EguiLayer::new(&gpu, &window, config.format);
        self.open = Some(Open {
            window,
            gpu,
            surface,
            config,
            layer,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(m) = &mut self.open
            && m.layer.on_window_event(&m.window, &event)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.outcome = Some(LobbyOutcome::Quit);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(m) = &mut self.open {
                    m.config.width = size.width.max(1);
                    m.config.height = size.height.max(1);
                    m.surface.configure(&m.gpu.device, &m.config);
                }
                Self::with_host(|h| h.resize(size));
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(m) = &self.open {
            m.window.request_redraw();
        }
    }
}

/// The lobby page until it starts a match or the player quits. `after`
/// tells the page how the last match ended.
///
/// # Errors
/// The event loop or the window failed.
pub fn run(url: &str, mirror: &StorageMirror, after: Option<AfterMatch>) -> Result<LobbyOutcome, String> {
    MIRROR.with(|m| *m.borrow_mut() = Some(mirror.clone()));
    let mut app = LobbyApp {
        url: url.to_owned(),
        mirror: mirror.clone(),
        demo: None,
        after,
        open: None,
        outcome: None,
        problem: None,
        waiting_since: Instant::now(),
        error: None,
    };
    let ran = run_on_event_loop(&mut app);
    app.open = None;
    ran?;
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok(app.outcome.unwrap_or(LobbyOutcome::Quit))
}

/// The page can show its settings over the match running now: it is
/// loaded, hidden behind the match, and said `settings:ready`.
#[must_use]
pub fn match_settings_available() -> bool {
    SETTINGS_READY.with(Cell::get) && HOST.with(|h| h.borrow().is_some())
}

/// The match menu's Settings key: shows the page over the match and asks it
/// for its settings (`settings:open`), named for the match by `kicker`.
pub fn open_match_settings(kicker: &str) -> bool {
    LobbyApp::with_host(|h| {
        // What the hidden page said during the match is stale.
        drop(h.drain());
        h.show();
        h.send(&json!({"v": BRIDGE_VERSION, "type": "settings:open", "kicker": kicker}));
    })
    .is_some()
}

/// Each frame while the settings are up over a match: the page's settings
/// writes reach the files and the live cvars, Alt+Enter still works, and
/// `settings:closed` (Back or Escape) hides the page again. True once
/// they are closed, or the page is gone.
pub fn poll_match_settings() -> bool {
    let Some(events) = LobbyApp::with_host(|h| h.drain()) else {
        return true;
    };
    let mut closed = false;
    for event in events {
        let HostEvent::Message(message) = event else { continue };
        match text(&message, "type").as_deref() {
            Some("settings:closed") => closed = true,
            // The page reloaded under the settings: they are gone.
            Some("hello") => {
                SETTINGS_READY.with(|r| r.set(false));
                closed = true;
            }
            Some("settings:ready") => SETTINGS_READY.with(|r| r.set(true)),
            _ => {}
        }
        match act(&message) {
            Action::Reply(answer) => {
                LobbyApp::with_host(|h| h.send(&answer));
            }
            Action::Fullscreen => toggle_fullscreen(),
            Action::External(url) => {
                if !open_external(&url) {
                    eprintln!("settings: not opening {url}");
                }
            }
            Action::Storage(key, value) => {
                let kept = MIRROR.with(|m| m.borrow().as_ref().is_some_and(|m| m.apply(&key, value.as_deref())));
                if !kept {
                    eprintln!("settings: not keeping {key}");
                }
            }
            // A match, a demo or quitting is the lobby's to start, not the
            // settings'.
            Action::Match(..) | Action::Demo(..) | Action::Quit | Action::CancelDemo | Action::Nothing => {}
        }
    }
    if closed {
        LobbyApp::with_host(WebviewHost::hide);
    }
    closed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_the_lobby_host_s_client_path() {
        assert_eq!(
            page_url("wss://eu.washoi.net/lobby").as_deref(),
            Some("https://eu.washoi.net/client")
        );
        assert_eq!(
            page_url("ws://localhost:9000").as_deref(),
            Some("http://localhost:9000/client")
        );
        assert_eq!(page_url("https://eu.washoi.net/"), None);
    }

    #[test]
    fn hello_is_answered_with_the_request_id() {
        let Action::Reply(answer) = act(&json!({"v": 1, "type": "hello", "id": "r1", "webVersion": "x"})) else {
            panic!("hello not answered");
        };
        assert_eq!(answer["id"], "r1");
        assert_eq!(answer["type"], "hello");
        assert_eq!(answer["bridgeVersion"], 1);
        assert!(
            answer["capabilities"]
                .as_array()
                .is_some_and(|c| c.contains(&json!("match")))
        );
    }

    #[test]
    fn a_match_request_becomes_the_choice() {
        let play = json!({"v": 1, "type": "match:play", "id": "r2", "serverUrl": "wss://eu.washoi.net:7777",
            "admissionTicket": "t", "name": "Wraith", "team": "alpha"});
        let Action::Match(choice, ok) = act(&play) else {
            panic!("no match")
        };
        assert!(choice.play);
        assert_eq!(choice.name, "Wraith");
        assert_eq!(choice.team.as_deref(), Some("alpha"));
        assert_eq!(ok["type"], "ok");
        assert_eq!(ok["id"], "r2");
        let watch = json!({"v": 1, "type": "match:watch", "id": "r3", "serverUrl": "ws://localhost:7777"});
        let Action::Match(choice, _) = act(&watch) else {
            panic!("no watch")
        };
        assert!(!choice.play);
        assert_eq!(choice.admission_ticket, None);
    }

    #[test]
    fn bad_requests_are_refused_not_followed() {
        let Action::Reply(no) = act(&json!({"v": 1, "type": "match:play", "id": "r4", "serverUrl": "file:///x"}))
        else {
            panic!("accepted");
        };
        assert_eq!(no["type"], "error");
        let Action::Reply(no) = act(&json!({"v": 1, "type": "match:play", "id": "r5", "serverUrl": "ws://h:1"})) else {
            panic!("played without a ticket");
        };
        assert_eq!(no["code"], "bad_request");
        let Action::Reply(unknown) = act(&json!({"v": 1, "type": "console:exec", "id": "r6"})) else {
            panic!("unknown accepted");
        };
        assert_eq!(unknown["code"], "unknown_type");
        assert!(matches!(act(&json!({"v": 1, "type": "noise"})), Action::Nothing));
        assert!(matches!(
            act(&json!({"v": 1, "type": "storage:set", "value": "x"})),
            Action::Nothing
        ));
    }

    #[test]
    fn a_demo_request_names_its_file_and_reel() {
        let play = json!({"v": 1, "type": "demo:play", "id": "r7", "filename": "a.shagdemo",
            "base": "https://eu.washoi.net/data/demos/", "reel": 2});
        let Action::Demo(_, base, file, reel) = act(&play) else {
            panic!("no demo")
        };
        assert_eq!(base, "https://eu.washoi.net/data/demos");
        assert_eq!(file, "a.shagdemo");
        assert_eq!(reel, Some(2.0));
        let local = json!({"v": 1, "type": "demo:play", "id": "r8", "filename": "a", "base": "file:///etc"});
        assert!(matches!(act(&local), Action::Reply(r) if r["code"] == "bad_request"));
        assert!(matches!(
            act(&json!({"v": 1, "type": "demo:cancel"})),
            Action::CancelDemo
        ));
    }

    #[test]
    fn storage_writes_name_their_key() {
        let set = act(&json!({"v": 1, "type": "storage:set", "key": "engine_binds", "value": "{}"}));
        assert!(matches!(set, Action::Storage(k, Some(v)) if k == "engine_binds" && v == "{}"));
        let removed = act(&json!({"v": 1, "type": "storage:set", "key": "shag_player_key", "value": null}));
        assert!(matches!(removed, Action::Storage(k, None) if k == "shag_player_key"));
    }
}
