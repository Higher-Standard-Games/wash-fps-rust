//! `washoi menu <ws://lobby>`: the game as a player starts it. The Play page
//! (`client::lobby_panel`, wash-fps `client/LobbyPanel.ts`) in the window
//! until the lobby hands out a game server, then the match in the same
//! window's place, then back to the menu when the match window closes.
//!
//! With `--screenshot`, the page runs offscreen for `--seconds` (`--quick`
//! presses Quick play once it can) and its last frame is saved.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use shagger::input::key_codes::{dom_code, dom_mouse_button};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};
use shagger::rendering::wgpu;
use shagger::ui::egui_layer::{EguiLayer, egui};
use washoi::client::hud::frame_rate_display::FrameRateDisplay;
use washoi::client::lobby_panel::{LobbyChoice, LobbyPanel, Tab, install_style};
use washoi::client::settings::fps_settings_view::FpsSettingsView;
use washoi::client::settings::settings_model::{SETTINGS_PAGES, VIDEO_SECTIONS};
use washoi::console::GameConsole;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::window::{OpenWindow, next_frame, open_window, present, run_on_event_loop};

/// Behind the page: Washoi's night blue.
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.035,
    g: 0.03,
    b: 0.07,
    a: 1.0,
};

struct MenuWindow {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    layer: EguiLayer,
}

struct MenuApp {
    panel: LobbyPanel,
    console: GameConsole,
    open: Option<MenuWindow>,
    choice: Option<LobbyChoice>,
    error: Option<String>,
    /// `show_fps`, which the browser shows over the lobby too.
    frame_rate: FrameRateDisplay,
    last_frame: Option<Instant>,
}

impl MenuApp {
    fn redraw(&mut self) {
        self.panel.poll();
        if let Some(choice) = self.panel.take_choice() {
            self.choice = Some(choice);
            return;
        }
        let Some(m) = &mut self.open else { return };
        let frame = match next_frame(&m.gpu, &m.window, &mut m.surface, &m.config) {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(why) => {
                self.error = Some(why);
                return;
            }
        };
        let view = frame
            .texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let now = Instant::now();
        let dt = self.last_frame.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        self.last_frame = Some(now);
        self.frame_rate
            .tick(dt, shagger::core::cvar::with_cvars(|c| c.get_bool("show_fps")));
        let (panel, console, frame_rate) = (&mut self.panel, &mut self.console, &self.frame_rate);
        let ui = m.layer.run(&m.window, |ui| {
            panel.ui(ui);
            if let Some(text) = frame_rate.text() {
                frame_rate_badge(ui.ctx(), text);
            }
            if std::mem::take(&mut panel.console_pressed) {
                console.console.toggle();
            }
            console.ui(ui.ctx());
        });
        let mut encoder = m
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("menu") });
        clear(&mut encoder, &view);
        m.layer
            .paint(&m.gpu, &mut encoder, &view, [m.config.width, m.config.height], ui);
        m.gpu.queue.submit([encoder.finish()]);
        present(&m.gpu, &m.window, &mut m.surface, &m.config, frame);
    }
}

/// `.fps-frame-rate` over the lobby: the number and "FPS" on the yellow
/// badge in the top-right corner.
fn frame_rate_badge(ctx: &egui::Context, text: &str) {
    let ink = egui::Color32::from_rgb(23, 19, 38);
    egui::Area::new(egui::Id::unique("fps-frame-rate"))
        .anchor(egui::Align2::RIGHT_TOP, [0.0, 0.0])
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(0xff, 0xc5, 0x2c))
                .inner_margin(egui::Margin {
                    left: 22,
                    right: 14,
                    top: 8,
                    bottom: 9,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{text:>4}")).size(17.0).strong().color(ink));
                        ui.label(egui::RichText::new("FPS").size(10.0).strong().color(ink));
                    });
                });
        });
}

fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("menu clear"),
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

impl ApplicationHandler for MenuApp {
    /// The loop is ending, perhaps with the process (Cmd+Q): the settings
    /// upload and the lobby goodbye happen here, as when the menu closes.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.panel.leave();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.open.is_some() {
            return;
        }
        match open_window(event_loop) {
            Ok(OpenWindow {
                window,
                gpu,
                surface,
                config,
            }) => {
                window.set_title("Washoi");
                let layer = EguiLayer::new(&gpu, &window, config.format);
                install_style(&layer.ctx);
                self.open = Some(MenuWindow {
                    window,
                    gpu,
                    surface,
                    config,
                    layer,
                });
            }
            Err(why) => {
                self.error = Some(why);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // A backquote typed into a lobby text field is the field's; with the
        // console open it closes it, as Escape does.
        let typing = self
            .open
            .as_ref()
            .is_some_and(|m| m.layer.ctx.egui_wants_keyboard_input());
        if let WindowEvent::KeyboardInput { event: key, .. } = &event
            && (key.physical_key == PhysicalKey::Code(KeyCode::Backquote) && (self.console.console.open || !typing)
                || key.physical_key == PhysicalKey::Code(KeyCode::Escape) && self.console.console.open)
        {
            if key.state == ElementState::Pressed && !key.repeat {
                self.console.console.toggle();
            }
            return;
        }
        // A key, button or wheel turn for the action waiting on the
        // controls page goes there, not to the menu.
        if let (Some(code), Some(settings), Some(m)) = (bind_code(&event), &mut self.panel.settings, &self.open) {
            let now = m.layer.ctx.input(|i| i.time);
            if settings.controls.take_input(&code, now) {
                return;
            }
        }
        if let Some(m) = &mut self.open
            && m.layer.on_window_event(&m.window, &event)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(m) = &mut self.open {
                    m.config.width = size.width.max(1);
                    m.config.height = size.height.max(1);
                    m.surface.configure(&m.gpu.device, &m.config);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.choice.is_some() || self.console.ctx.quit || self.error.is_some() {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(m) = &self.open {
            m.window.request_redraw();
        }
    }
}

/// A pressed key, button or wheel turn as the controls page names it
/// (`InputCode`).
fn bind_code(event: &WindowEvent) -> Option<String> {
    match event {
        WindowEvent::KeyboardInput { event: key, .. } if key.state == ElementState::Pressed && !key.repeat => {
            match key.physical_key {
                PhysicalKey::Code(code) => Some(dom_code(code)),
                PhysicalKey::Unidentified(_) => None,
            }
        }
        WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button,
            ..
        } => dom_mouse_button(*button).map(|b| format!("mouse:{}", b.name())),
        WindowEvent::MouseWheel { delta, .. } => {
            let y = match delta {
                MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                MouseScrollDelta::PixelDelta(p) => p.y,
            };
            // winit's positive turns up, the browser's `deltaY` down.
            (y != 0.0).then(|| if y > 0.0 { "wheel:up" } else { "wheel:down" }.to_owned())
        }
        _ => None,
    }
}

/// The menu in its window until the lobby sends this player somewhere;
/// `None` when the window closes first.
pub fn choose(
    url: &str,
    key_file: Option<std::path::PathBuf>,
    match_error: Option<String>,
) -> Result<Option<LobbyChoice>, String> {
    let mut panel = LobbyPanel::connect(url, key_file);
    panel.match_error = match_error;
    let mut app = MenuApp {
        console: GameConsole::new(washoi::config_dir()),
        panel,
        open: None,
        choice: None,
        error: None,
        frame_rate: FrameRateDisplay::default(),
        last_frame: None,
    };
    let ran = run_on_event_loop(&mut app);
    if let Some(m) = &app.open {
        crate::window::remember_placement(&m.window);
    }
    app.open = None;
    if app.choice.is_none() {
        // Closed or `quit`: the settings still go up.
        app.panel.leave();
    }
    ran?;
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok(app.choice)
}

/// What an offscreen run of the menu does.
pub struct Shot {
    /// Press Quick play once it can.
    pub quick: bool,
    /// Open this settings page (`controls`, `gameplay`, `video`, `audio`),
    /// and with `video/effects` a video section.
    pub settings: Option<String>,
    /// Open this rail page (`play`, `leaderboard`, `demos`, `profile`) once
    /// logged in; `demos:latest` opens the Latest tab, `demos:play` also
    /// plays the newest recording, and `demos:reel` its longest highlight reel.
    pub tab: Option<String>,
    /// Open the console and run these commands (`;` between them).
    pub console: Option<String>,
    pub seconds: f64,
    pub size: (u32, u32),
}

/// The page offscreen for `seconds`, its last frame saved to `path`.
pub fn screenshot(
    url: &str,
    key_file: Option<std::path::PathBuf>,
    shot: &Shot,
    path: &Path,
) -> Result<Option<LobbyChoice>, String> {
    let (width, height) = shot.size;
    let (quick, seconds) = (shot.quick, shot.seconds);
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let mut layer = EguiLayer::offscreen(&gpu, OFFSCREEN_FORMAT);
    install_style(&layer.ctx);
    let mut panel = LobbyPanel::connect(url, key_file);
    if let Some(page) = &shot.settings {
        // `video/effects` opens a video section too.
        let (page, section) = page
            .split_once('/')
            .map_or((page.as_str(), None), |(p, s)| (p, Some(s)));
        let id = SETTINGS_PAGES
            .iter()
            .find(|(_, title, _)| title.eq_ignore_ascii_case(page))
            .map(|p| p.0)
            .ok_or_else(|| format!("no settings page {page}"))?;
        let mut view = FpsSettingsView::open(id);
        if let Some(section) = section {
            view.video_section = Some(
                VIDEO_SECTIONS
                    .iter()
                    .find(|(_, name)| name.eq_ignore_ascii_case(section))
                    .map(|s| s.0)
                    .ok_or_else(|| format!("no video section {section}"))?,
            );
        }
        panel.settings = Some(view);
    }
    let mut console = GameConsole::new(None);
    if let Some(lines) = &shot.console {
        console.console.open = true;
        for line in lines.split(';') {
            console.run(line.trim());
        }
        for (_, text) in &console.console.lines {
            println!("console: {text}");
        }
    }
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width as f32, height as f32),
        )),
        ..Default::default()
    };
    let started = Instant::now();
    let mut pressed = false;
    let mut status = String::new();
    let mut pixels = Vec::new();
    let mut choice = None;
    while started.elapsed() < Duration::from_secs_f64(seconds) {
        panel.poll();
        if quick && !pressed && panel.quick_play_enabled() {
            println!("menu: quick play");
            panel.start_quick_play();
            pressed = true;
        }
        if let Some(tab) = &shot.tab
            && panel.tab == Tab::Play
            && !panel.name.is_empty()
        {
            panel.open_tab(match tab.as_str() {
                "leaderboard" => Tab::Leaderboard,
                "profile" => Tab::Profile,
                "demos" | "demos:latest" | "demos:play" | "demos:reel" => Tab::Demos,
                _ => Tab::Play,
            });
            if tab.starts_with("demos:") {
                panel.demos.show_latest(&mut panel.client);
            }
        }
        // `demos:play` presses Play on the newest recording once it is listed.
        if shot.tab.as_deref() == Some("demos:play") && !pressed && panel.demos.play_newest() {
            println!("menu: playing the newest demo");
            pressed = true;
        }
        if shot.tab.as_deref() == Some("demos:reel") && !pressed && panel.demos.play_newest_reel() {
            println!("menu: playing the newest demo's highlight reel");
            pressed = true;
        }
        if let Some(room) = panel.in_room() {
            let now = format!("{} {}: {}", room.gametype_label, room.lobby_id, room.status());
            if now != status {
                println!("menu: {now}");
                status = now;
            }
        }
        let frame = layer.run_input(input(), |ui| {
            panel.ui(ui);
            console.ui(ui.ctx());
        });
        pixels = render_encoded_to_rgba(&gpu, width, height, BACKGROUND, |encoder, view| {
            layer.paint(&gpu, encoder, view, [width, height], frame);
        });
        if let Some(c) = panel.take_choice() {
            choice = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(33));
    }
    if let Some(why) = &panel.closed {
        println!("menu: lobby closed: {why}");
    }
    image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    println!("menu: saved {}", path.display());
    Ok(choice)
}
