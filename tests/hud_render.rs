//! The HUD of a player who was just fragged: the death screen's veil and
//! card, the damage overlay, the vitals and weapon slots draw over a plain
//! frame. Writes the frame to `HUD_PNG` when that is set, to look at.
//!
//! Needs a GPU adapter, software ones included; it reports a skip without.

use shagger::babylon::post_processes::image_processing_post_process::{
    ImageProcessingConfiguration, ImageProcessingPostProcess,
};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_hdr_with_ui_to_rgba};
use shagger::rendering::wgpu;
use shagger::shared::json;
use shagger::shared::net::game_state::PlayerState;
use washoi::client::fps_hud::{FpsHud, WatchFrame};
use washoi::client::hud::widgets::pickup_prompt::{asks_to_swap, swap_text};
use washoi::client::match_state_manager::MatchStateManager;
use washoi::shared::net::snapshot_events::{Point, SnapshotEvent};

const ZERO: Point = Point { x: 0.0, y: 0.0, z: 0.0 };

#[test]
fn a_fragged_player_sees_the_death_screen() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (width, height) = (1280u32, 720u32);
    let mut hud = FpsHud::new(&gpu, OFFSCREEN_FORMAT).unwrap();
    let roster = vec![
        (
            "player_1".to_owned(),
            PlayerState {
                entity_id: "player_1".into(),
                name: "Washiki!".into(),
                ..PlayerState::default()
            },
        ),
        (
            "player_2".to_owned(),
            PlayerState {
                entity_id: "player_2".into(),
                name: "Ramen".into(),
                score: 3,
                ..PlayerState::default()
            },
        ),
    ];
    let match_state = MatchStateManager::new();
    let positions = vec![
        ("player_1".to_owned(), [0.0, 0.0, 0.0]),
        ("player_2".to_owned(), [300.0, 0.0, 0.0]),
    ];
    let events = vec![
        SnapshotEvent::WeaponState {
            entity_id: "player_1".into(),
            current_weapon: "rlauncher".into(),
            weapons: json::parse(r#"["blade","rlauncher","machinegun"]"#).unwrap(),
            ammo: json::parse(r#"{"rlauncher":0,"machinegun":{"clip":4,"reserve":60}}"#).unwrap(),
            reloading: false,
            last_fire_tick: 0,
        },
        SnapshotEvent::Damage {
            entity_id: "player_1".into(),
            attacker_id: "player_2".into(),
            damage: 60,
            weapon_id: "rlauncher".into(),
            at: ZERO,
            hit_location: None,
            pain_tier: None,
        },
        SnapshotEvent::CombatState {
            entity_id: "player_1".into(),
            health: 0,
            armor: 25,
            dead: true,
        },
        SnapshotEvent::Death {
            entity_id: "player_1".into(),
            killer_id: "player_2".into(),
            weapon_id: "rlauncher".into(),
            damage_type: "explosion".into(),
            final_damage: 60,
            at: ZERO,
            vel: ZERO,
            kb_dir: ZERO,
            kill_weapon: None,
            hit_location: None,
        },
    ];
    let mut frame = WatchFrame {
        roster: &roster,
        match_state: &match_state,
        events: &events,
        view_entity: Some("player_1"),
        playback: false,
        playing: true,
        speed: 0.0,
        positions: &positions,
        yaw: 0.0,
        dt: 1.0 / 60.0,
        air: None,
        watched: None,
    };
    hud.update_watching(&frame, width as f32, height as f32);
    // A second in: the countdown is on, the overlay still fading.
    frame.events = &[];
    for _ in 0..60 {
        hud.update_watching(&frame, width as f32, height as f32);
    }
    hud.prepare(&gpu);
    let post = ImageProcessingPostProcess::new(&gpu, OFFSCREEN_FORMAT);
    let grey = wgpu::Color {
        r: 0.3,
        g: 0.3,
        b: 0.3,
        a: 1.0,
    };
    let pixels = render_hdr_with_ui_to_rgba(
        &gpu,
        width,
        height,
        grey,
        &post,
        &ImageProcessingConfiguration::default(),
        |_| {},
        |_| {},
        |pass| hud.draw(pass),
    );
    if let Some(path) = std::env::var_os("HUD_PNG") {
        image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).unwrap();
    }
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // The corner is under the red veil: redder than it is green.
    let corner = at(4, 4);
    assert!(corner[0] > corner[1] + 10, "corner {corner:?}");
    // The stamp's red slab crosses the middle above the centre.
    let red = (0..width).filter(|&x| {
        let p = at(x, height / 2 - 60);
        p[0] > 150 && p[1] < 90
    });
    assert!(red.count() > 200);
}

/// Quad Damage's countdown stacks above the vitals: a gold slab, hot red in
/// its last five seconds. Writes the gold frame to `HUD_QUAD_PNG` when set.
#[test]
fn a_quad_carrier_sees_the_countdown() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (width, height) = (1280u32, 720u32);
    let mut hud = FpsHud::new(&gpu, OFFSCREEN_FORMAT).unwrap();
    let roster = vec![(
        "player_1".to_owned(),
        PlayerState {
            entity_id: "player_1".into(),
            name: "Washiki!".into(),
            ..PlayerState::default()
        },
    )];
    let match_state = MatchStateManager::new();
    let positions = vec![("player_1".to_owned(), [0.0, 0.0, 0.0])];
    let snapshot = |seconds: u16| {
        vec![
            SnapshotEvent::CombatState {
                entity_id: "player_1".into(),
                health: 100,
                armor: 0,
                dead: false,
            },
            SnapshotEvent::PowerupState {
                entity_id: "player_1".into(),
                kind: "quad".into(),
                seconds,
            },
        ]
    };
    let post = ImageProcessingPostProcess::new(&gpu, OFFSCREEN_FORMAT);
    let mut render = |events: &[SnapshotEvent]| {
        let frame = WatchFrame {
            roster: &roster,
            match_state: &match_state,
            events,
            view_entity: Some("player_1"),
            playback: false,
            playing: true,
            speed: 0.0,
            positions: &positions,
            yaw: 0.0,
            dt: 1.0 / 60.0,
            air: None,
            watched: None,
        };
        hud.update_watching(&frame, width as f32, height as f32);
        hud.prepare(&gpu);
        render_hdr_with_ui_to_rgba(
            &gpu,
            width,
            height,
            wgpu::Color {
                r: 0.3,
                g: 0.3,
                b: 0.3,
                a: 1.0,
            },
            &post,
            &ImageProcessingConfiguration::default(),
            |_| {},
            |_| {},
            |pass| hud.draw(pass),
        )
    };
    // Pixels in the left quarter between the middle and the vitals.
    let count = |pixels: &[u8], hit: &dyn Fn([u8; 3]) -> bool| {
        (height / 2..height * 7 / 10)
            .flat_map(|y| (0..width / 4).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let i = ((y * width + x) * 4) as usize;
                hit([pixels[i], pixels[i + 1], pixels[i + 2]])
            })
            .count()
    };
    let gold = |p: [u8; 3]| p[0] > 200 && p[1] > 150 && p[2] < 90;
    let hot = |p: [u8; 3]| p[0] > 180 && p[1] < 90 && p[2] < 90;
    let pixels = render(&snapshot(12));
    if let Some(path) = std::env::var_os("HUD_QUAD_PNG") {
        image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).unwrap();
    }
    assert!(count(&pixels, &gold) > 1500, "gold {}", count(&pixels, &gold));
    let pixels = render(&snapshot(4));
    assert!(count(&pixels, &hot) > 1500, "red {}", count(&pixels, &hot));
    // A snapshot without it ends it.
    let pixels = render(&snapshot(4)[..1]);
    assert!(count(&pixels, &hot) < 100);
}

/// Dead in a round, watching a teammate: the "Spectating" bar names them
/// above the bottom bar, and the HUD shows their vitals and weapon. Writes
/// the frame to `HUD_SPECTATE_PNG` when set.
#[test]
fn a_dead_player_watching_a_teammate_sees_whom() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (width, height) = (1280u32, 720u32);
    let mut hud = FpsHud::new(&gpu, OFFSCREEN_FORMAT).unwrap();
    let player = |id: &str, name: &str| {
        (
            id.to_owned(),
            PlayerState {
                entity_id: id.into(),
                name: name.into(),
                ..PlayerState::default()
            },
        )
    };
    let roster = vec![player("player_1", "Washiki!"), player("player_3", "Noodlesoup")];
    let mut match_state = MatchStateManager::new();
    match_state.process_event(
        &json::parse(r#"{"type":"match_state","phase":2,"round":1,"playerTeams":{"1":"red","3":"red"}}"#).unwrap(),
    );
    let positions = vec![("player_3".to_owned(), [0.0, 0.0, 0.0])];
    let events = vec![
        SnapshotEvent::CombatState {
            entity_id: "player_1".into(),
            health: 0,
            armor: 0,
            dead: true,
        },
        SnapshotEvent::CombatState {
            entity_id: "player_3".into(),
            health: 77,
            armor: 10,
            dead: false,
        },
        SnapshotEvent::WeaponState {
            entity_id: "player_3".into(),
            current_weapon: "machinegun".into(),
            weapons: json::parse(r#"["blade","machinegun"]"#).unwrap(),
            ammo: json::parse(r#"{"machinegun":{"clip":20,"reserve":60}}"#).unwrap(),
            reloading: false,
            last_fire_tick: 0,
        },
    ];
    let frame = WatchFrame {
        roster: &roster,
        match_state: &match_state,
        events: &events,
        view_entity: Some("player_1"),
        playback: false,
        playing: true,
        speed: 0.0,
        positions: &positions,
        yaw: 0.0,
        dt: 1.0 / 60.0,
        air: None,
        watched: Some("player_3"),
    };
    hud.update_watching(&frame, width as f32, height as f32);
    assert!(hud.state.local_dead);
    assert_eq!(hud.state.spectating, "player_3");
    assert_eq!(hud.state.spectating_name, "Noodlesoup");
    hud.prepare(&gpu);
    let post = ImageProcessingPostProcess::new(&gpu, OFFSCREEN_FORMAT);
    let pixels = render_hdr_with_ui_to_rgba(
        &gpu,
        width,
        height,
        wgpu::Color {
            r: 0.3,
            g: 0.3,
            b: 0.3,
            a: 1.0,
        },
        &post,
        &ImageProcessingConfiguration::default(),
        |_| {},
        |_| {},
        |pass| hud.draw(pass),
    );
    if let Some(path) = std::env::var_os("HUD_SPECTATE_PNG") {
        image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).unwrap();
    }
    // The bar's cyan edge, left of centre, above the bottom bar.
    let cyan = (height - 320..height - 230)
        .flat_map(|y| (0..width / 2).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let i = ((y * width + x) * 4) as usize;
            let (r, g, b) = (pixels[i], pixels[i + 1], pixels[i + 2]);
            r < 90 && g > 180 && b > 180
        })
        .count();
    assert!(cyan > 100, "{cyan}");
}

/// The weapon swap prompt: the gold WEAPON cell, then the dark slab with
/// the line, 340 px up and centred. Writes the frame to `HUD_PROMPT_PNG`
/// when set.
#[test]
fn a_full_inventory_is_asked_to_swap() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (width, height) = (1280u32, 720u32);
    let mut hud = FpsHud::new(&gpu, OFFSCREEN_FORMAT).unwrap();
    let roster = vec![(
        "player_1".to_owned(),
        PlayerState {
            entity_id: "player_1".into(),
            name: "Washiki!".into(),
            ..PlayerState::default()
        },
    )];
    let match_state = MatchStateManager::new();
    let positions = vec![("player_1".to_owned(), [0.0, 0.0, 0.0])];
    let events = vec![SnapshotEvent::WeaponState {
        entity_id: "player_1".into(),
        current_weapon: "riotgun".into(),
        weapons: json::parse(r#"["blade","riotgun","plasmagun"]"#).unwrap(),
        ammo: json::parse(r#"{"riotgun":{"clip":8,"reserve":24},"plasmagun":{"clip":40,"reserve":80}}"#).unwrap(),
        reloading: false,
        last_fire_tick: 0,
    }];
    let frame = WatchFrame {
        roster: &roster,
        match_state: &match_state,
        events: &events,
        view_entity: Some("player_1"),
        playback: false,
        playing: true,
        speed: 0.0,
        positions: &positions,
        yaw: 0.0,
        dt: 1.0 / 60.0,
        air: None,
        watched: None,
    };
    hud.update_watching(&frame, width as f32, height as f32);
    let state = &hud.state;
    assert!(asks_to_swap("rlauncher", &state.weapons, &state.current_weapon));
    hud.pickup_prompt.set("pickup", Some(swap_text("F", "rlauncher")));
    hud.update_watching(&frame, width as f32, height as f32);
    hud.prepare(&gpu);
    let post = ImageProcessingPostProcess::new(&gpu, OFFSCREEN_FORMAT);
    let grey = wgpu::Color {
        r: 0.3,
        g: 0.3,
        b: 0.3,
        a: 1.0,
    };
    let pixels = render_hdr_with_ui_to_rgba(
        &gpu,
        width,
        height,
        grey,
        &post,
        &ImageProcessingConfiguration::default(),
        |_| {},
        |_| {},
        |pass| hud.draw(pass),
    );
    if let Some(path) = std::env::var_os("HUD_PROMPT_PNG") {
        image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).unwrap();
    }
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // Some row over the middle of the screen crosses the gold cell and
    // then the dark slab (the vitals and slots sit lower, the top row
    // higher).
    let crosses = |y: u32| {
        let gold = (0..width)
            .filter(|&x| {
                let p = at(x, y);
                p[0] > 180 && p[1] > 110 && p[2] < 90
            })
            .count();
        let dark = (0..width)
            .filter(|&x| {
                let p = at(x, y);
                p[0] < 40 && p[1] < 40 && p[2] < 50
            })
            .count();
        gold > 40 && dark > 200
    };
    assert!((height / 3..height / 2).any(crosses));
}

/// A spectator flying free: the SPECTATOR card with the keys, bottom left
/// above where the HUD would be. Writes the frame to `HUD_FREEFLY_PNG`
/// when set.
#[test]
fn a_freefly_spectator_sees_the_keys() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (width, height) = (1280u32, 720u32);
    let mut hud = FpsHud::new(&gpu, OFFSCREEN_FORMAT).unwrap();
    let roster = vec![(
        "player_3".to_owned(),
        PlayerState {
            entity_id: "player_3".into(),
            name: "Noodlesoup".into(),
            ..PlayerState::default()
        },
    )];
    let match_state = MatchStateManager::new();
    let frame = WatchFrame {
        roster: &roster,
        match_state: &match_state,
        events: &[],
        view_entity: None,
        playback: false,
        playing: false,
        speed: 0.0,
        positions: &[],
        yaw: 0.0,
        dt: 1.0 / 60.0,
        air: None,
        watched: None,
    };
    hud.update_watching(&frame, width as f32, height as f32);
    assert!(hud.state.watcher);
    assert!(hud.state.spectating.is_empty());
    hud.prepare(&gpu);
    let post = ImageProcessingPostProcess::new(&gpu, OFFSCREEN_FORMAT);
    let pixels = render_hdr_with_ui_to_rgba(
        &gpu,
        width,
        height,
        wgpu::Color {
            r: 0.3,
            g: 0.3,
            b: 0.3,
            a: 1.0,
        },
        &post,
        &ImageProcessingConfiguration::default(),
        |_| {},
        |_| {},
        |pass| hud.draw(pass),
    );
    if let Some(path) = std::env::var_os("HUD_FREEFLY_PNG") {
        image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).unwrap();
    }
    // The card's cyan edge and its cream key chips, in the lower left.
    let count = |test: &dyn Fn(u8, u8, u8) -> bool| {
        (height / 2..height)
            .flat_map(|y| (0..width / 3).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let i = ((y * width + x) * 4) as usize;
                test(pixels[i], pixels[i + 1], pixels[i + 2])
            })
            .count()
    };
    let cyan = count(&|r, g, b| r < 90 && g > 180 && b > 180);
    let cream = count(&|r, g, b| r > 220 && g > 200 && b > 170);
    assert!(cyan > 100, "{cyan}");
    assert!(cream > 300, "{cream}");
}

/// A new player's hint, bottom right above the weapons: the gold HINT cell
/// and the line with the player's keys as gold caps. Writes the frame to
/// `HUD_HINT_PNG` when set.
#[test]
fn a_hint_shows_above_the_weapons() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (width, height) = (1280u32, 720u32);
    let mut hud = FpsHud::new(&gpu, OFFSCREEN_FORMAT).unwrap();
    let roster = vec![(
        "player_3".to_owned(),
        PlayerState {
            entity_id: "player_3".into(),
            name: "Noodlesoup".into(),
            ..PlayerState::default()
        },
    )];
    let match_state = MatchStateManager::new();
    let mut input = shagger::input::input_system::InputSystem::new(None);
    washoi::client::fps_actions::register_fps_actions(&mut input);
    hud.hints.set_keys(&input, false);
    let mut frame = WatchFrame {
        roster: &roster,
        match_state: &match_state,
        events: &[],
        view_entity: Some("player_3"),
        playback: false,
        playing: true,
        speed: 0.0,
        positions: &[],
        yaw: 0.0,
        dt: 1.0 / 60.0,
        air: None,
        watched: None,
    };
    hud.update_watching(&frame, width as f32, height as f32);
    hud.hints
        .present(washoi::client::hud::hint_coach::HintId::Strafe, "machinegun");
    // Past the fade in.
    frame.dt = 0.3;
    hud.update_watching(&frame, width as f32, height as f32);
    assert_eq!(
        hud.hints.text().as_deref(),
        Some("In the air, hold [A] or [D] and turn the mouse to gain speed!")
    );
    hud.prepare(&gpu);
    let post = ImageProcessingPostProcess::new(&gpu, OFFSCREEN_FORMAT);
    let pixels = render_hdr_with_ui_to_rgba(
        &gpu,
        width,
        height,
        wgpu::Color {
            r: 0.3,
            g: 0.3,
            b: 0.3,
            a: 1.0,
        },
        &post,
        &ImageProcessingConfiguration::default(),
        |_| {},
        |_| {},
        |pass| hud.draw(pass),
    );
    if let Some(path) = std::env::var_os("HUD_HINT_PNG") {
        image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).unwrap();
    }
    // Gold (the cell and the caps) in the lower right, above the weapons.
    let gold = (height / 2..height - 200)
        .flat_map(|y| (width / 2..width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let i = ((y * width + x) * 4) as usize;
            let (r, g, b) = (pixels[i], pixels[i + 1], pixels[i + 2]);
            r > 200 && g > 140 && b < 90
        })
        .count();
    assert!(gold > 2000, "gold pixels: {gold}");
}
