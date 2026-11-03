//! The first thing a player sees: the game's data brought up to date
//! before the menu, with a bar for the download (the startup half of
//! wash-fps `client/LobbyAssetPreparation.ts` and SHAG's
//! `PackageDownloadStatus.ts`: the phase, the package, megabytes so far of
//! the whole and the speed).
//!
//! The browser fetches the common packages at the lobby and each map's as it
//! loads; the native client keeps every package on disk, so it fetches them
//! all here, once, and afterwards only what a deployment changed
//! (`shagger::loader::pk3::package_store`). Without the network, a data
//! directory a sync completed before is used as it is.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use shagger::loader::pk3::package_store::{SyncProgress, has_packages, is_synced, sync_packages};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;
use shagger::ui::egui_layer::{EguiLayer, egui};
use washoi::client::lobby_panel::install_style;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::window::{OpenWindow, open_window, run_on_event_loop};

/// The music's playlists, served beside the packages.
const PLAYLISTS: [&str; 3] = ["game.playlist.json", "menu.playlist.json", "endgame.playlist.json"];
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.035,
    g: 0.03,
    b: 0.07,
    a: 1.0,
};

/// How a sync ended, or that it is still running.
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Running(SyncProgress),
    Done,
    /// Why, and what the directory holds: a complete set, some packages, or none.
    Failed(String, OnDisk),
}

/// What a directory holds when a sync cannot finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnDisk {
    Complete,
    Partial,
    Nothing,
}

fn on_disk(dir: &Path) -> OnDisk {
    if is_synced(dir) {
        OnDisk::Complete
    } else if has_packages(dir) {
        OnDisk::Partial
    } else {
        OnDisk::Nothing
    }
}

/// What the player chose on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    /// The data is complete: on to the menu.
    Ready,
    /// The window closed first.
    Quit,
}

/// Brings `dir` up to `base_url`'s packages and playlists on a thread of
/// its own, reporting into `phase`.
fn start(base_url: String, dir: PathBuf, cancel: Arc<AtomicBool>, phase: Arc<Mutex<Phase>>) {
    std::thread::spawn(move || {
        let set = |p: Phase| *phase.lock().expect("sync phase") = p;
        let result = sync_packages(&base_url, &dir, &PLAYLISTS, &cancel, &mut |p| {
            set(Phase::Running(p.clone()));
        });
        let end = match result {
            Ok(report) => {
                println!(
                    "data: {} packages, {} downloaded, {} removed",
                    report.index.packages.len(),
                    report.downloaded.len(),
                    report.removed.len()
                );
                // The music goes without; the game does not need it.
                for name in report.extras_failed {
                    println!("data: {name} could not be fetched");
                }
                Phase::Done
            }
            Err(why) => Phase::Failed(why.to_string(), on_disk(&dir)),
        };
        set(end);
    });
}

struct SyncWindow {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    layer: EguiLayer,
}

struct SyncApp {
    base_url: String,
    dir: PathBuf,
    cancel: Arc<AtomicBool>,
    phase: Arc<Mutex<Phase>>,
    /// Bytes at the start of the speed window, and when.
    speed_from: (u64, Instant),
    speed: f64,
    open: Option<SyncWindow>,
    outcome: Option<SyncOutcome>,
    error: Option<String>,
}

/// `toFixed(1)` megabytes.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

impl SyncApp {
    fn restart(&mut self) {
        self.cancel = Arc::new(AtomicBool::new(false));
        self.phase = Arc::new(Mutex::new(Phase::Running(SyncProgress {
            package: String::new(),
            received: 0,
            total: 0,
        })));
        start(
            self.base_url.clone(),
            self.dir.clone(),
            self.cancel.clone(),
            self.phase.clone(),
        );
    }

    fn redraw(&mut self) {
        let phase = self.phase.lock().expect("sync phase").clone();
        if phase == Phase::Done {
            self.outcome = Some(SyncOutcome::Ready);
            return;
        }
        if let Phase::Running(p) = &phase {
            // `bytesPerSecond` over about two seconds.
            let (from, at) = self.speed_from;
            let elapsed = at.elapsed().as_secs_f64();
            if elapsed >= 2.0 || p.received < from {
                self.speed = p.received.saturating_sub(from) as f64 / elapsed.max(0.25);
                self.speed_from = (p.received, Instant::now());
            }
        }
        let Some(m) = &mut self.open else { return };
        let frame = match m.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                m.surface.configure(&m.gpu.device, &m.config);
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut choice = None;
        let speed = self.speed;
        let ui = m.layer.run(&m.window, |ui| {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.vertical_centered(|ui| {
                    ui.set_max_width(520.0);
                    // The bar's track, which the menu's style leaves as dark as the page.
                    ui.visuals_mut().extreme_bg_color = egui::Color32::from_rgb(40, 36, 58);
                    ui.heading("WASHOI");
                    ui.add_space(16.0);
                    match &phase {
                        Phase::Running(p) if p.total == 0 => {
                            ui.label("Checking the game data…");
                            ui.add(egui::ProgressBar::new(0.0).desired_width(520.0).animate(true));
                        }
                        Phase::Running(p) => {
                            ui.label(format!("Downloading {}", p.package));
                            ui.add(
                                egui::ProgressBar::new(p.received as f32 / p.total as f32)
                                    .desired_width(520.0)
                                    .text(format!("{} / {}", megabytes(p.received), megabytes(p.total))),
                            );
                            ui.label(format!("{}/s", megabytes(speed as u64)));
                        }
                        Phase::Failed(why, held) => {
                            ui.label("The game data could not be downloaded.");
                            ui.small(why);
                            if *held == OnDisk::Partial {
                                ui.small("The data on disk is incomplete: some maps or sounds may be missing.");
                            }
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui.button("Try again").clicked() {
                                    choice = Some(false);
                                }
                                if *held != OnDisk::Nothing && ui.button("Play with the data on disk").clicked() {
                                    choice = Some(true);
                                }
                            });
                        }
                        Phase::Done => {}
                    }
                });
            });
        });
        let mut encoder = m.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("data sync"),
        });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("data sync clear"),
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
        m.layer
            .paint(&m.gpu, &mut encoder, &view, [m.config.width, m.config.height], ui);
        m.gpu.queue.submit([encoder.finish()]);
        m.window.pre_present_notify();
        m.gpu.queue.present(frame);
        match choice {
            Some(true) => self.outcome = Some(SyncOutcome::Ready),
            Some(false) => self.restart(),
            None => {}
        }
    }
}

impl ApplicationHandler for SyncApp {
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
                self.open = Some(SyncWindow {
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
        if let Some(m) = &mut self.open
            && m.layer.on_window_event(&m.window, &event)
        {
            return;
        }
        let escape = matches!(&event, WindowEvent::KeyboardInput { event: key, .. }
            if key.state == ElementState::Pressed && key.physical_key == PhysicalKey::Code(KeyCode::Escape));
        match event {
            WindowEvent::CloseRequested => {
                self.outcome = Some(SyncOutcome::Quit);
                event_loop.exit();
            }
            _ if escape => {
                self.outcome = Some(SyncOutcome::Quit);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(m) = &mut self.open {
                    m.config.width = size.width.max(1);
                    m.config.height = size.height.max(1);
                    m.surface.configure(&m.gpu.device, &m.config);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.outcome.is_some() {
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

/// The screen until the data in `dir` matches `base_url`'s (or the player
/// takes what is on disk); `Quit` when the window closes first.
///
/// # Errors
/// No window could open.
pub fn run(base_url: &str, dir: &Path) -> Result<SyncOutcome, String> {
    let mut app = SyncApp {
        base_url: base_url.to_owned(),
        dir: dir.to_path_buf(),
        cancel: Arc::new(AtomicBool::new(false)),
        phase: Arc::new(Mutex::new(Phase::Done)),
        speed_from: (0, Instant::now()),
        speed: 0.0,
        open: None,
        outcome: None,
        error: None,
    };
    app.restart();
    let ran = run_on_event_loop(&mut app);
    app.cancel.store(true, Ordering::Relaxed);
    app.open = None;
    ran?;
    if let Some(why) = app.error {
        return Err(why);
    }
    Ok(app.outcome.unwrap_or(SyncOutcome::Quit))
}

/// The same without a window, printing the progress: for `washoi sync`
/// and machines without a display.
///
/// # Errors
/// Why the sync stopped.
pub fn run_headless(base_url: &str, dir: &Path) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let phase = Arc::new(Mutex::new(Phase::Done));
    *phase.lock().expect("sync phase") = Phase::Running(SyncProgress {
        package: String::new(),
        received: 0,
        total: 0,
    });
    start(base_url.to_owned(), dir.to_path_buf(), cancel, phase.clone());
    let mut last = String::new();
    loop {
        let now = phase.lock().expect("sync phase").clone();
        match now {
            Phase::Done => return Ok(()),
            Phase::Failed(why, _) => return Err(why),
            Phase::Running(p) if p.package != last => {
                println!(
                    "data: {} ({} of {})",
                    p.package,
                    megabytes(p.received),
                    megabytes(p.total)
                );
                last = p.package;
            }
            Phase::Running(_) => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
