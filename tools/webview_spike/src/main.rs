//! W0 spike for `docs/research/WEBVIEW_LOBBY.md` (section 18): one winit
//! window with a wgpu surface (a stand-in for the match) and a wry child
//! webview (the lobby) over it. The webview is shown and hidden on a
//! schedule, the bridge says hello both ways, and the page reports how its
//! timers and a WebSocket fare while hidden. Everything lands in a JSON
//! report.
//!
//! ```text
//! webview_spike [--url URL] [--ws ws://HOST:PORT] [--seconds N]
//!               [--hide-at S] [--show-at S] [--shots DIR] [--report FILE]
//! ```
//!
//! Without `--url` the page is the built-in test page. `--ws` is an echo
//! server the test page pings every five seconds. `--shots` saves the whole
//! screen (Linux, ImageMagick `import`) one second after each change.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// The built-in lobby stand-in. `WS_URL` is replaced before loading.
const TEST_PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>spike</title>
<style>
  html, body { margin: 0; height: 100%; background: #1b1030; color: #f6ecd2;
    font: 24px/1.4 system-ui, sans-serif; }
  main { padding: 48px; }
  h1 { color: #ffc21a; margin: 0 0 16px; }
  #log { font: 14px/1.3 monospace; white-space: pre; opacity: .8; }
  input { font-size: 20px; }
</style></head>
<body><main>
  <h1>WEBVIEW LOBBY STAND-IN</h1>
  <div>shell: <b id="shell">?</b> · visibility: <b id="vis">?</b> · ticks: <b id="ticks">0</b></div>
  <p><input id="typing" placeholder="type here to test focus"></p>
  <div id="log"></div>
</main>
<script>
const native = window.__washoiNative;
const post = (m) => window.ipc.postMessage(JSON.stringify(Object.assign({ v: 1 }, m)));
const log = (t) => { const el = document.getElementById("log"); el.textContent = (t + "\n" + el.textContent).slice(0, 2000); };
document.getElementById("shell").textContent = native ? native.shell + " bridge " + native.bridgeVersion : "browser";
native.receive = (m) => { log("native → page: " + JSON.stringify(m)); };
let ticks = 0, last = performance.now(), worstGap = 0;
setInterval(() => {
  const now = performance.now();
  worstGap = Math.max(worstGap, now - last);
  last = now;
  ticks++;
  document.getElementById("ticks").textContent = ticks;
  if (ticks % 5 === 0) post({ type: "timer", ticks, worstGapMs: Math.round(worstGap), hidden: document.hidden });
  if (ticks % 5 === 0) worstGap = 0;
}, 1000);
const vis = () => {
  document.getElementById("vis").textContent = document.visibilityState;
  post({ type: "visibility", state: document.visibilityState });
};
document.addEventListener("visibilitychange", vis);
vis();
document.getElementById("typing").addEventListener("keydown", (e) => post({ type: "page:key", code: e.code }));
const wsUrl = "WS_URL";
if (wsUrl.startsWith("ws")) {
  let sent = 0, got = 0;
  const ws = new WebSocket(wsUrl);
  ws.onopen = () => { post({ type: "ws", state: "open" });
    setInterval(() => { if (ws.readyState === 1) { ws.send("ping " + (++sent)); } }, 5000); };
  ws.onmessage = () => { got++; post({ type: "ws", state: "pong", sent, got, hidden: document.hidden }); };
  ws.onclose = (e) => post({ type: "ws", state: "closed", code: e.code, sent, got, hidden: document.hidden });
  ws.onerror = () => post({ type: "ws", state: "error" });
}
post({ type: "hello", webVersion: "spike", bridgeVersion: 1, href: location.href });
</script></body></html>"#;

struct Options {
    url: Option<String>,
    ws: String,
    seconds: f64,
    hide_at: Vec<f64>,
    show_at: Vec<f64>,
    shots: Option<std::path::PathBuf>,
    report: Option<std::path::PathBuf>,
}

fn options() -> Options {
    let mut o = Options {
        url: None,
        ws: String::new(),
        seconds: 20.0,
        hide_at: Vec::new(),
        show_at: Vec::new(),
        shots: None,
        report: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{a} needs a value"));
        match a.as_str() {
            "--url" => o.url = Some(value()),
            "--ws" => o.ws = value(),
            "--seconds" => o.seconds = value().parse().expect("--seconds"),
            "--hide-at" => o.hide_at.push(value().parse().expect("--hide-at")),
            "--show-at" => o.show_at.push(value().parse().expect("--show-at")),
            "--shots" => o.shots = Some(value().into()),
            "--report" => o.report = Some(value().into()),
            _ => panic!("unknown argument {a}"),
        }
    }
    o
}

/// What the run saw, written as JSON at the end.
#[derive(Default)]
struct Report {
    events: Vec<serde_json::Value>,
    frames: u64,
    winit_keys_while_hidden: u32,
    winit_keys_while_shown: u32,
    focus_changes: Vec<(f64, bool)>,
    rss_kb: Vec<(f64, u64, u64)>,
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

struct App {
    o: Options,
    start: Instant,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    webview: Option<wry::WebView>,
    visible: bool,
    report: Arc<Mutex<Report>>,
    pending_shots: Vec<(Instant, String)>,
    replies: Arc<Mutex<Vec<String>>>,
}

impl App {
    fn t(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn init_script() -> String {
        format!(
            "window.__washoiNative = {{ shell: 'washoi-native', bridgeVersion: 1, clientVersion: 'spike', platform: '{}', capabilities: ['match'], storage: {{}}, receive(m) {{}} }}; document.documentElement.dataset.shell = 'native';",
            std::env::consts::OS
        )
    }

    fn build_webview(&mut self, window: &Window) {
        let report = self.report.clone();
        let replies = self.replies.clone();
        let start = self.start;
        let mut builder = wry::WebViewBuilder::new()
            .with_initialization_script_for_main_only(Self::init_script(), true)
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                let t = start.elapsed().as_secs_f64();
                let body = req.body();
                let mut msg: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
                println!("{t:7.2}s page → native ({}): {body}", req.uri());
                if msg.get("type").and_then(|v| v.as_str()) == Some("hello") {
                    replies
                        .lock()
                        .unwrap()
                        .push(r#"{"v":1,"type":"hello","clientVersion":"spike","bridgeVersion":1}"#.to_owned());
                }
                if let Some(obj) = msg.as_object_mut() {
                    obj.insert("t".into(), t.into());
                    obj.insert("origin".into(), req.uri().to_string().into());
                }
                report.lock().unwrap().events.push(msg);
            })
            .with_navigation_handler(|url| {
                println!("navigation: {url}");
                true
            })
            .with_bounds(bounds(window.inner_size()));
        builder = match &self.o.url {
            Some(url) => builder.with_url(url),
            None => builder.with_html(TEST_PAGE.replace("WS_URL", &self.o.ws)),
        };
        match builder.build_as_child(window) {
            Ok(w) => self.webview = Some(w),
            Err(e) => panic!("webview: {e}"),
        }
        self.visible = true;
    }

    fn set_visible(&mut self, visible: bool) {
        let Some(w) = &self.webview else { return };
        w.set_visible(visible).expect("set_visible");
        if visible {
            let _ = w.focus();
        } else {
            let _ = w.focus_parent();
            if let Some(win) = &self.window {
                win.focus_window();
            }
        }
        self.visible = visible;
        let t = self.t();
        println!("{t:7.2}s webview {}", if visible { "shown" } else { "hidden" });
        self.report
            .lock()
            .unwrap()
            .events
            .push(serde_json::json!({"type": "native:visible", "visible": visible, "t": t}));
        self.shot(if visible { "shown" } else { "hidden" });
    }

    fn shot(&mut self, label: &str) {
        if self.o.shots.is_some() {
            let name = format!("{:05.1}-{label}", self.t());
            self.pending_shots.push((Instant::now() + Duration::from_secs(1), name));
        }
    }

    fn draw(&mut self) {
        let Some(g) = &self.gpu else { return };
        let (wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) =
            g.surface.get_current_texture()
        else {
            return;
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = g
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let t = self.start.elapsed().as_secs_f64();
        {
            let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.4 + 0.3 * (t * 2.0).sin(),
                            b: 0.2,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        g.queue.submit([enc.finish()]);
        g.queue.present(frame);
        self.report.lock().unwrap().frames += 1;
    }

    fn finish(&mut self) {
        let mut r = self.report.lock().unwrap();
        let mut json = serde_json::json!({
            "os": std::env::consts::OS,
            "seconds": self.t(),
            "frames": r.frames,
            "winit_keys_while_hidden": r.winit_keys_while_hidden,
            "winit_keys_while_shown": r.winit_keys_while_shown,
            "focus_changes": r.focus_changes,
            "rss_kb": r.rss_kb,
            "events": std::mem::take(&mut r.events),
        });
        if let Ok(exe) = std::env::current_exe()
            && let Ok(m) = std::fs::metadata(exe)
        {
            json["binary_bytes"] = m.len().into();
        }
        let text = serde_json::to_string_pretty(&json).unwrap();
        match &self.o.report {
            Some(p) => std::fs::write(p, text).expect("report"),
            None => println!("{text}"),
        }
    }
}

fn bounds(size: PhysicalSize<u32>) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::PhysicalPosition::new(0, 0).into(),
        size: wry::dpi::PhysicalSize::new(size.width, size.height).into(),
    }
}

/// Resident memory of this process and of every process below it (the
/// webview's own processes), in kB. Linux only; elsewhere zeros.
fn rss_kb() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        let read = |pid: &str| -> Option<(u64, u32)> {
            let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
            let field = |name: &str| {
                status
                    .lines()
                    .find(|l| l.starts_with(name))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse().ok())
            };
            Some((field("VmRSS:").unwrap_or(0), field("PPid:").unwrap_or(0) as u32))
        };
        let me = std::process::id();
        let mut parents = std::collections::HashMap::new();
        let mut rss = std::collections::HashMap::new();
        for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Ok(pid) = name.parse::<u32>()
                && let Some((kb, ppid)) = read(&name)
            {
                parents.insert(pid, ppid);
                rss.insert(pid, kb);
            }
        }
        let below = |mut pid: u32| {
            while let Some(&p) = parents.get(&pid) {
                if p == me {
                    return true;
                }
                if p <= 1 {
                    return false;
                }
                pid = p;
            }
            false
        };
        let own = rss.get(&me).copied().unwrap_or(0);
        let children = rss.iter().filter(|(p, _)| below(**p)).map(|(_, kb)| kb).sum();
        (own, children)
    }
    #[cfg(not(target_os = "linux"))]
    {
        // The webview's processes are not our children everywhere
        // (WebView2 runs under its own browser process), so they are summed
        // by name: msedgewebview2 on Windows, WebKit's on macOS.
        #[cfg(windows)]
        let (cmd, args) = (
            "powershell",
            vec![
                "-NoProfile",
                "-Command",
                "$o=(Get-Process -Id $PID).Parent; $me=(Get-Process -Name webview_spike).WorkingSet64 | Measure-Object -Sum; $w=(Get-Process -Name msedgewebview2 -ErrorAction SilentlyContinue).WorkingSet64 | Measure-Object -Sum; \"$([int64]($me.Sum/1024)) $([int64]($w.Sum/1024))\"",
            ],
        );
        #[cfg(target_os = "macos")]
        let (cmd, args) = (
            "sh",
            vec![
                "-c",
                "ps -axo rss=,comm= | awk '/webview_spike/{a+=$1} /WebKit|WebContent|Networking|GPU/{b+=$1} END{print a+0, b+0}'",
            ],
        );
        let out = std::process::Command::new(cmd).args(args).output();
        let text = out
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let mut it = text.split_whitespace().filter_map(|v| v.parse().ok());
        (it.next().unwrap_or(0), it.next().unwrap_or(0))
    }
}

/// The whole screen to `path`: ImageMagick on Linux, `screencapture` on
/// macOS, .NET's screen copy on Windows.
fn screenshot(path: &std::path::Path) -> bool {
    let p = path.display().to_string();
    let mut c;
    #[cfg(target_os = "linux")]
    {
        c = std::process::Command::new("import");
        c.args(["-window", "root", &p]);
    }
    #[cfg(target_os = "macos")]
    {
        c = std::process::Command::new("screencapture");
        c.args(["-x", &p]);
    }
    #[cfg(windows)]
    {
        c = std::process::Command::new("powershell");
        c.args([
            "-NoProfile",
            "-Command",
            &format!(
                "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $i=New-Object System.Drawing.Bitmap $b.Width,$b.Height; $g=[System.Drawing.Graphics]::FromImage($i); $g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); $i.Save('{p}')"
            ),
        ]);
    }
    c.status().is_ok_and(|s| s.success())
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            el.create_window(
                Window::default_attributes()
                    .with_title("webview spike")
                    .with_inner_size(LogicalSize::new(1280.0, 720.0)),
            )
            .expect("window"),
        );
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("adapter");
        println!("adapter: {:?}", adapter.get_info().name);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("config");
        surface.configure(&device, &config);
        self.gpu = Some(Gpu {
            surface,
            device,
            queue,
            config,
        });
        self.build_webview(&window);
        self.window = Some(window);
        self.shot("start");
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                if let Some(g) = &mut self.gpu {
                    g.config.width = size.width.max(1);
                    g.config.height = size.height.max(1);
                    g.surface.configure(&g.device, &g.config);
                }
                if let Some(w) = &self.webview {
                    let _ = w.set_bounds(bounds(size));
                }
            }
            WindowEvent::Focused(f) => {
                let t = self.t();
                self.report.lock().unwrap().focus_changes.push((t, f));
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let mut r = self.report.lock().unwrap();
                if self.visible {
                    r.winit_keys_while_shown += 1;
                } else {
                    r.winit_keys_while_hidden += 1;
                }
                println!(
                    "{:7.2}s winit key {:?}",
                    self.start.elapsed().as_secs_f64(),
                    event.physical_key
                );
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        let t = self.t();
        let replies: Vec<String> = std::mem::take(&mut *self.replies.lock().unwrap());
        if let Some(w) = &self.webview {
            for r in replies {
                let _ = w.evaluate_script(&format!("window.__washoiNative.receive({r})"));
            }
        }
        if let Some(i) = self.o.hide_at.iter().position(|&s| t >= s) {
            self.o.hide_at.remove(i);
            self.set_visible(false);
        }
        if let Some(i) = self.o.show_at.iter().position(|&s| t >= s) {
            self.o.show_at.remove(i);
            self.set_visible(true);
        }
        let due: Vec<String> = {
            let now = Instant::now();
            let (due, keep): (Vec<_>, Vec<_>) = self.pending_shots.drain(..).partition(|(at, _)| *at <= now);
            self.pending_shots = keep;
            due.into_iter().map(|(_, n)| n).collect()
        };
        for name in due {
            if let Some(dir) = &self.o.shots {
                let path = dir.join(format!("{name}.png"));
                let ok = screenshot(&path);
                println!(
                    "{t:7.2}s shot {} {}",
                    path.display(),
                    if ok { "saved" } else { "failed" }
                );
            }
        }
        {
            let mut r = self.report.lock().unwrap();
            let need = r.rss_kb.last().is_none_or(|(at, _, _)| t - at >= 5.0);
            if need {
                let (own, kids) = rss_kb();
                r.rss_kb.push((t, own, kids));
            }
        }
        if t >= self.o.seconds {
            self.finish();
            el.exit();
            return;
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
    }
}

fn main() {
    let o = options();
    #[cfg(target_os = "linux")]
    gtk::init().expect("gtk");
    let mut builder = EventLoop::builder();
    #[cfg(target_os = "linux")]
    {
        // Child webviews need X11; on a Wayland desktop this runs on XWayland.
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let event_loop = builder.build().expect("event loop");
    let mut app = App {
        o,
        start: Instant::now(),
        window: None,
        gpu: None,
        webview: None,
        visible: false,
        report: Arc::default(),
        pending_shots: Vec::new(),
        replies: Arc::default(),
    };
    event_loop.run_app(&mut app).expect("run");
    let mut s = String::new();
    let _ = write!(s, "done");
    println!("{s}");
}
