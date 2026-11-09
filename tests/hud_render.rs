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
