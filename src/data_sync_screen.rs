//! The first thing a player sees: the game's data brought up to date
//! before the menu, drawn as the lobby's twin like the match loading screen
//! (`LobbyTwin`, `ConnectingOverlay`'s stage and bar), with the download in
//! the bar (the startup half of
//! wash-fps `client/LobbyAssetPreparation.ts` and SHAG's
//! `PackageDownloadStatus.ts`: the phase, the package, megabytes so far of
//! the whole and the speed).
//!
//! The browser fetches the common packages at the lobby and each map's as it
//! loads; the native client keeps every package on disk, so it fetches them
//! all here, once, and afterwards only what a deployment changed
//! (`shagger::loader::pk3::package_store`). Without the network, a data
//! directory a sync completed before is used as it is.
//!
//! A game that can replace itself (the installed Windows game, an AppImage,
//! a `Washoi.app`) first updates here (`update_check::self_update`): with a
//! newer package downloaded and checked, the screen puts it in place and
//! starts it, and this game ends. A failed update is printed and the game
//! plays on.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use shagger::loader::pk3::package_store::{SyncProgress, has_packages, is_synced, sync_packages};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;
use shagger::ui::components::connecting_overlay::{BarFill, OverlayStage, StageState, paint_twin};
use shagger::ui::components::lobby_twin::{TwinRail, TwinStepState, clamp_css};
use shagger::ui::egui_layer::{EguiLayer, egui};
use washoi::client::washoi_style::install_style;
use washoi::update_check::{self, UpdateProgress};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::match_loading_screen::twin_style;
use crate::window::{OpenWindow, next_frame, open_window, present, run_on_event_loop};

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
    /// The game's own update, before the data.
    Updating(UpdateProgress),
    /// A newer package, downloaded and checked: to be put in place.
    Install(update_check::Target, PathBuf),
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
    /// The installer is replacing the game: end it, without the menu.
    Updating,
}

/// Brings `dir` up to `base_url`'s packages and playlists on a thread of
/// its own, reporting into `phase`.
fn start(
    base_url: String,
    dir: PathBuf,
    update: Option<update_check::Target>,
    cancel: Arc<AtomicBool>,
    phase: Arc<Mutex<Phase>>,
) {
    std::thread::spawn(move || {
        let set = |p: Phase| {
            if let Ok(mut slot) = phase.lock() {
                *slot = p;
            }
        };
        if let Some(target) = update
            && let Some(updates) = washoi::cache_dir().map(|d| d.join("updates"))
        {
            match update_check::self_update(&target, &updates, &cancel, &mut |p| set(Phase::Updating(p))) {
                Ok(Some(download)) => {
                    set(Phase::Install(target, download));
                    return;
                }
                Ok(None) => {}
                Err(why) => eprintln!("{why}"),
            }
            set(Phase::Running(SyncProgress {
                package: String::new(),
                received: 0,
                total: 0,
            }));
        }
        // A panic in the sync is a failure the screen can offer a retry
        // for, not a spinner forever.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sync_packages(&base_url, &dir, &PLAYLISTS, &cancel, &mut |p| {
                set(Phase::Running(p.clone()));
            })
        }));
        let Ok(result) = result else {
            set(Phase::Failed("the download stopped unexpectedly".into(), on_disk(&dir)));
            return;
        };
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
    /// The lobby's twin, as the match loading screen draws it.
    screen: DataScreen,
    open: Option<SyncWindow>,
    outcome: Option<SyncOutcome>,
    error: Option<String>,
}

/// `toFixed(1)` megabytes.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

impl SyncApp {
    /// The sync from the start, after the game's own update when `update`.
    fn restart(&mut self, update: Option<update_check::Target>) {
        self.cancel = Arc::new(AtomicBool::new(false));
        self.phase = Arc::new(Mutex::new(if update.is_some() {
            Phase::Updating(UpdateProgress::Checking)
        } else {
            Phase::Running(SyncProgress {
                package: String::new(),
                received: 0,
                total: 0,
            })
        }));
        start(
            self.base_url.clone(),
            self.dir.clone(),
            update,
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
        if let Phase::Install(target, download) = &phase {
            match update_check::apply(target, download) {
                Ok(()) => {
                    println!("update: started the new game from {}", download.display());
                    self.outcome = Some(SyncOutcome::Updating);
                    return;
                }
                // Played on: the data sync follows on the next try.
                Err(why) => {
                    eprintln!("{why}");
                    self.restart(None);
                    return;
                }
            }
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
        let mut choice = None;
        let speed = self.speed;
        let screen = &mut self.screen;
        screen.dress(&phase, speed);
        let ui = m.layer.run(&m.window, |ui| {
            let now = ui.ctx().input(|i| i.time);
            choice = screen.paint(ui, now);
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
        present(&m.gpu, &m.window, &mut m.surface, &m.config, frame);
        match choice {
            Some(true) => self.outcome = Some(SyncOutcome::Ready),
            Some(false) => self.restart(update_check::self_update_target()),
            None => {}
        }
    }
}

/// The data screen as the lobby's twin (`LobbyTwin`, the match loading
/// screen's look): the rail with the start-up's steps, and on the stage the
/// phase as the title, the package in the status plate, and the transfer
/// bar (`PackageDownloadStatus`'s phase, package, megabytes and speed).
struct DataScreen {
    rail: TwinRail,
    stage: OverlayStage,
    /// The phase last dressed, to log a change once.
    last: String,
    /// What the directory holds, after a failure.
    on_disk: OnDisk,
}

impl DataScreen {
    fn new(base_url: &str) -> Self {
        let mut rail = TwinRail::new("Starting up", &["Update", "Game data", "Lobby"], "Game data");
        let host = base_url.split("://").nth(1).unwrap_or(base_url);
        rail.set_fact("source", "Source", host.split('/').next().unwrap_or(host));
        let stage = OverlayStage::new(
            "GAME DATA",
            vec!["CHECKING".into(), "DATA".into()],
            "",
            "DOWNLOAD STATUS",
        );
        Self {
            rail,
            stage,
            last: String::new(),
            on_disk: OnDisk::Nothing,
        }
    }

    /// Sets the rail and the stage for `phase`.
    fn dress(&mut self, phase: &Phase, speed: f64) {
        use TwinStepState::{Active, Done, Error, Pending};
        let percent = |received: u64, total: u64| (received as f64 * 100.0 / total.max(1) as f64).floor() as u32;
        let stage = &mut self.stage;
        stage.error = None;
        stage.footer = String::new();
        let (kicker, title, lead, status, bar, state, steps, log): (_, [&str; 2], String, String, _, _, _, String) =
            match phase {
                Phase::Updating(UpdateProgress::Checking) => (
                    "GAME UPDATE",
                    ["CHECKING", "UPDATES"],
                    "Looking for a newer Washoi before the game data.".into(),
                    "CHECKING".into(),
                    BarFill::Sweep,
                    StageState::Preparing,
                    [Active, Pending, Pending],
                    "Checking for updates".into(),
                ),
                Phase::Updating(UpdateProgress::Downloading {
                    version,
                    received,
                    total,
                }) => {
                    stage.footer = format!("{} of {}", megabytes(*received), megabytes(*total));
                    (
                        "GAME UPDATE",
                        ["UPDATING", "WASHOI"],
                        format!("Washoi {version} is downloading. The game starts again once it is in place."),
                        format!("UPDATE {}%", percent(*received, *total)),
                        BarFill::Fill(*received as f32 / (*total).max(1) as f32),
                        StageState::Loading,
                        [Active, Pending, Pending],
                        format!("Updating to {version}"),
                    )
                }
                Phase::Install(..) => (
                    "GAME UPDATE",
                    ["INSTALLING", "UPDATE"],
                    "Putting the new game in place.".into(),
                    "INSTALLING".into(),
                    BarFill::Fill(1.0),
                    StageState::SettingUp,
                    [Active, Pending, Pending],
                    "Installing the update".into(),
                ),
                Phase::Running(p) if p.total == 0 => (
                    "GAME DATA",
                    ["CHECKING", "DATA"],
                    "Comparing the game data on disk with the servers.".into(),
                    "CHECKING".into(),
                    BarFill::Sweep,
                    StageState::Preparing,
                    [Done, Active, Pending],
                    "Checking the game data".into(),
                ),
                Phase::Running(p) => {
                    stage.footer = format!(
                        "{} of {} · {}/s",
                        megabytes(p.received),
                        megabytes(p.total),
                        megabytes(speed as u64)
                    );
                    (
                        "GAME DATA",
                        ["DOWNLOADING", "DATA"],
                        format!(
                            "Fetching {}. Only what changed since the last start comes down.",
                            p.package
                        ),
                        format!("DOWNLOADING {}%", percent(p.received, p.total)),
                        BarFill::Fill(p.received as f32 / p.total.max(1) as f32),
                        StageState::Loading,
                        [Done, Active, Pending],
                        format!("Downloading {}", p.package),
                    )
                }
                Phase::Failed(why, held) => {
                    self.on_disk = *held;
                    let stop = if why.ends_with(['.', '!', '?']) { "" } else { "." };
                    let mut error = format!("The game data could not be downloaded: {why}{stop}");
                    if *held == OnDisk::Partial {
                        error.push_str(" The data on disk is incomplete: some maps or sounds may be missing.");
                    }
                    stage.error = Some(error);
                    (
                        "GAME DATA",
                        ["DOWNLOAD", "FAILED"],
                        "Try again, or play with the data already on disk.".into(),
                        "FAILED".into(),
                        BarFill::Fill(0.0),
                        StageState::Failed,
                        [Done, Error, Pending],
                        why.clone(),
                    )
                }
                Phase::Done => (
                    "GAME DATA",
                    ["DATA", "READY"],
                    "Opening the lobby.".into(),
                    "READY".into(),
                    BarFill::Fill(1.0),
                    StageState::Ready,
                    [Done, Done, Active],
                    "Ready".into(),
                ),
            };
        kicker.clone_into(&mut stage.kicker);
        stage.set_title(title.iter().map(|l| (*l).to_owned()).collect());
        stage.lead = lead;
        stage.status = status;
        stage.bar = bar;
        stage.state = state;
        self.rail.set_steps(&steps);
        if let Phase::Running(p) = phase
            && p.total > 0
        {
            self.rail.set_fact("package", "Package", &p.package);
        }
        if log != self.last {
            self.rail.log(&log);
            self.last = log;
        }
    }

    /// Paints the screen; on a failure, the choice: `Some(true)` to play
    /// with the data on disk, `Some(false)` to try again.
    fn paint(&mut self, ui: &mut egui::Ui, now: f64) -> Option<bool> {
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);
        paint_twin(&painter, rect, &twin_style(), &self.rail, &mut self.stage, now);
        if self.stage.state != StageState::Failed {
            return None;
        }
        // The keys, where the stage's content starts.
        let (vw, vh) = (rect.width() / 100.0, rect.height() / 100.0);
        let at = egui::pos2(
            rect.min.x + TwinRail::width(rect.width()) + clamp_css(40.0, 5.0 * vw, 96.0) + 44.0,
            rect.min.y + clamp_css(28.0, 5.0 * vh, 64.0),
        );
        let can_play = self.on_disk != OnDisk::Nothing;
        let mut choice = None;
        egui::Area::new(egui::Id::unique("data-sync-keys"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Try again").clicked() {
                        choice = Some(false);
                    }
                    if can_play && ui.button("Play with the data on disk").clicked() {
                        choice = Some(true);
                    }
                });
            });
        choice
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
                if self.outcome.is_some() || self.error.is_some() {
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
        screen: DataScreen::new(base_url),
        open: None,
        outcome: None,
        error: None,
    };
    app.restart(update_check::self_update_target());
    let ran = run_on_event_loop(&mut app);
    app.cancel.store(true, Ordering::Relaxed);
    if let Some(m) = &app.open {
        crate::window::remember_placement(&m.window);
    }
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
    start(base_url.to_owned(), dir.to_path_buf(), None, cancel, phase.clone());
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
            Phase::Running(_) | Phase::Updating(_) | Phase::Install(..) => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};

    /// The screen for `phase`, `seconds` in, as RGBA at 1280x720.
    fn render(phase: &Phase, seconds: f64) -> Option<Vec<u8>> {
        let gpu = Gpu::headless().ok()?;
        let (width, height) = (1280u32, 720u32);
        let mut layer = EguiLayer::offscreen(&gpu, OFFSCREEN_FORMAT);
        install_style(&layer.ctx);
        let mut screen = DataScreen::new("https://eu.washoi.net/data");
        let mut pixels = Vec::new();
        for t in [0.0, seconds] {
            screen.dress(phase, 6_800_000.0);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width as f32, height as f32),
                )),
                time: Some(t),
                ..Default::default()
            };
            let frame = layer.run_input(input, |ui| {
                screen.paint(ui, t);
            });
            pixels = render_encoded_to_rgba(&gpu, width, height, BACKGROUND, |encoder, target| {
                layer.paint(&gpu, encoder, target, [width, height], frame);
            });
        }
        Some(pixels)
    }

    #[test]
    fn a_download_draws_the_twin_and_its_bar() {
        let phase = Phase::Running(SyncProgress {
            package: "map_washoi_dm12.pk3".into(),
            received: 35_800_000,
            total: 346_900_000,
        });
        let mut screen = DataScreen::new("https://eu.washoi.net/data");
        screen.dress(&phase, 6_800_000.0);
        assert_eq!(screen.stage.status, "DOWNLOADING 10%");
        assert_eq!(screen.stage.footer, "35.8 MB of 346.9 MB · 6.8 MB/s");
        assert_eq!(screen.stage.title, ["DOWNLOADING", "DATA"]);
        let Some(pixels) = render(&phase, 1.5) else {
            eprintln!("skipped: no GPU adapter");
            return;
        };
        if let Ok(path) = std::env::var("DATA_SYNC_PNG") {
            image::save_buffer(&path, &pixels, 1280, 720, image::ColorType::Rgba8).unwrap();
        }
    }

    #[test]
    fn a_failure_offers_both_ways_on() {
        let phase = Phase::Failed("the server did not answer".into(), OnDisk::Partial);
        let mut screen = DataScreen::new("https://eu.washoi.net/data");
        screen.dress(&phase, 0.0);
        assert_eq!(screen.stage.state, StageState::Failed);
        assert!(screen.stage.error.as_deref().is_some_and(|e| e.contains("incomplete")));
        let Some(pixels) = render(&phase, 1.5) else {
            return;
        };
        if let Ok(path) = std::env::var("DATA_SYNC_FAILED_PNG") {
            image::save_buffer(&path, &pixels, 1280, 720, image::ColorType::Rgba8).unwrap();
        }
    }
}
