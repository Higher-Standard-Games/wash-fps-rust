//! Washoi's movement against wash-fps's own `MovementProfile` (shagger
//! `tools/golden/movement.mjs`): scripted players on synthetic maps, stepped
//! as `V2NetworkClient` predicts (sanitize, lifts, knockback drag, simulate).
//! Each step's position, velocity and a hash of every result field and the
//! whole saved profile state must match.

use shagger::loader::bsp::bsp_loader::load_collision;
use shagger::network::systems::movement::movement_profile::{MovementInput, ViewAngles};
use shagger::physics::sweep_backend_adapter::SweepBackendAdapter;
use shagger::physics::sweep_collision_backend::SweepCollisionBackend;
use shagger::shared::bsp::lift_system::LiftActor;
use shagger::shared::bsp::trigger_table::extract_trigger_volumes;
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::js::{js_cos, js_sin};
use shagger::shared::json::{self, Value};
use shagger::shared::server::simulation::input_sanitizer::{
    MovementFields, SanitizedMovement, sanitize_movement_or_zero,
};
use shagger::shared::server::simulation::types::Vec3;
use washoi::simulation::knockback::apply_knockback_drag;
use washoi::simulation::movement_config::AdvancedMovementResult;
use washoi::simulation::movement_profile::MovementProfile;

const DT: f64 = 0.008;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn num(x: f64) -> String {
    if x.is_nan() {
        "nan".into()
    } else {
        format!("{:016x}", x.to_bits())
    }
}

fn read_num(v: &Value) -> f64 {
    match v {
        Value::String(s) if s == "nan" => f64::NAN,
        Value::String(s) => f64::from_bits(u64::from_str_radix(s, 16).unwrap()),
        Value::Number(n) => *n,
        other => panic!("not a number: {other:?}"),
    }
}

fn read_vec(v: &[Value]) -> Vec3 {
    Vec3::new(read_num(&v[0]), read_num(&v[1]), read_num(&v[2]))
}

/// The harness's wish direction: forward (sin yaw, cos yaw), right (cos yaw, -sin yaw).
fn wish(buttons: u32, yaw: f64, scale: f64) -> [f64; 3] {
    let f = f64::from(u8::from(buttons & 1 != 0)) - f64::from(u8::from(buttons & 2 != 0));
    let s = f64::from(u8::from(buttons & 8 != 0)) - f64::from(u8::from(buttons & 4 != 0));
    let mut x = js_sin(yaw) * f + js_cos(yaw) * s;
    let mut z = js_cos(yaw) * f - js_sin(yaw) * s;
    let len = (x * x + z * z).sqrt();
    if len > 0.0 {
        x = (x / len) * scale;
        z = (z / len) * scale;
    }
    [x, 0.0, z]
}

/// The canonical line `movement.mjs` hashes.
struct Line(Vec<String>);

impl Line {
    fn n(&mut self, x: f64) {
        self.0.push(num(x));
    }
    fn b(&mut self, x: bool) {
        self.0.push(if x { "1" } else { "0" }.into());
    }
    fn s(&mut self, x: &str) {
        self.0.push(x.into());
    }
    fn v(&mut self, v: Vec3) {
        self.0.push(format!("{} {} {}", num(v.x), num(v.y), num(v.z)));
    }
    fn u(&mut self) {
        self.0.push("u".into());
    }
    fn on<T>(&mut self, x: Option<T>, f: impl FnOnce(&mut Self, T)) {
        match x {
            Some(x) => f(self, x),
            None => self.u(),
        }
    }
}

fn canonical(
    input: &SanitizedMovement,
    r: &AdvancedMovementResult,
    p: &MovementProfile,
    lift_offsets: &[f64],
    kb_age: Option<f64>,
) -> String {
    let mut l = Line(Vec::new());
    l.n(input.direction[0]);
    l.n(input.direction[2]);
    l.n(input.pitch);
    l.n(input.yaw);
    l.n(f64::from(input.buttons));
    l.v(r.position);
    l.v(r.velocity);
    l.b(r.grounded);
    l.b(r.is_sliding);
    l.b(r.is_crouching);
    l.s(r.wall_jump_state.name());
    l.s(r.ledge_grab_state.name());
    l.n(r.actual_horiz_speed);
    l.n(r.crouch_view_offset);
    l.n(r.slide_phase);
    l.n(r.ledge_grab_phase);
    l.on(r.jumppad_launch, Line::b);
    l.on(r.teleported, Line::b);
    l.on(r.teleport_yaw, Line::n);
    l.on(r.teleport_from, Line::v);
    l.on(r.trigger_id, |l, id| l.n(id as f64));
    l.on(r.water_level, |l, w| l.n(f64::from(w)));
    l.on(r.liquid_contents, |l, c| l.n(f64::from(c)));
    l.on(r.liquid_kind, |l, k| match k {
        Some(k) => l.s(k.name()),
        None => l.s("n"),
    });
    l.on(r.on_ladder, Line::b);
    l.on(r.ladder_climb_delta, Line::n);

    l.b(p.is_grounded);
    l.b(p.was_grounded_last_frame);
    l.b(p.jump_consumed);
    l.n(f64::from(p.water_level));
    l.n(p.water_jump_until_ms);
    l.b(p.prone_in_water);
    l.b(p.in_ladder_volume);
    l.b(p.climbing);
    l.n(p.ladder_detach_until_ms);
    l.n(p.sim_now_ms);
    l.n(p.actual_horiz_speed);
    l.n(p.smoothed_y);
    let w = &p.wall_jump;
    l.s(w.state.name());
    l.v(w.normal);
    l.v(w.position);
    l.v(w.pre_velocity);
    l.b(w.key_released);
    l.n(w.charge_start_ms);
    l.n(w.grace_until_ms);
    l.n(w.ground_grace_until_ms);
    l.n(w.cooldowns.len() as f64);
    for c in &w.cooldowns {
        l.n(c.start_ms);
        l.n(c.until_ms);
    }
    let s = &p.slide;
    l.b(s.is_sliding);
    l.b(s.is_crouching);
    for x in [
        s.crouch_view_offset,
        s.slide_phase,
        s.slide_start_ms,
        s.cooldown_until_ms,
        s.slide_end_ms,
        s.slide_end_phase,
        s.crouch_anchor_ms,
        s.crouch_from_offset,
        s.crouch_target_offset,
    ] {
        l.n(x);
    }
    let g = &p.ledge_grab;
    l.s(g.state.name());
    l.n(g.phase);
    l.n(g.state_start_ms);
    l.n(g.cooldown_until_ms);
    l.v(g.target_pos);
    l.v(g.start_pos);
    l.n(lift_offsets.len() as f64);
    for &o in lift_offsets {
        l.n(o);
    }
    match kb_age {
        Some(a) => l.n(a),
        None => l.s("n"),
    }
    l.0.join(",")
}

fn fnv(text: &str) -> String {
    let h = text
        .bytes()
        .fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    format!("{h:08x}")
}

#[test]
#[allow(clippy::too_many_lines)]
fn movement_matches_wash_fps() {
    let doc = json::parse(include_str!("golden/movement.json")).unwrap();
    assert_eq!(
        MovementProfile::default().determinism_fingerprint(),
        doc.get("fingerprint").and_then(Value::as_str).unwrap(),
        "movement config fingerprint"
    );
    let mut steps = 0;
    for (m, v) in doc.get("vectors").and_then(Value::as_array).unwrap().iter().enumerate() {
        let bsp = load_collision(&hex(v.get("bytes").and_then(Value::as_str).unwrap())).unwrap();
        let triggers = extract_trigger_volumes(&bsp.entities, &bsp.models);
        let expected = v.get("triggers").and_then(Value::as_array).unwrap();
        assert_eq!(triggers.len(), expected.len(), "map {m}: trigger count");
        for (t, e) in triggers.iter().zip(expected) {
            let e = e.as_array().unwrap();
            assert_eq!(t.id as f64, e[0].as_f64().unwrap(), "map {m}: trigger id");
            assert_eq!(t.kind.name(), e[1].as_str().unwrap(), "map {m}: trigger type");
            assert_eq!(
                (t.min, t.max, t.dest),
                (read_vec(&e[2..5]), read_vec(&e[5..8]), read_vec(&e[8..11]))
            );
            assert_eq!(
                t.dest_yaw.map(f64::to_bits),
                (e[11] != Value::Null).then(|| read_num(&e[11]).to_bits())
            );
        }

        for (run_index, run) in v.get("runs").and_then(Value::as_array).unwrap().iter().enumerate() {
            let backend = SweepCollisionBackend::new(bsp.clone());
            let mut world = SharedCollisionWorld::new(SweepBackendAdapter { backend });
            let mut profile = MovementProfile::default();
            profile.set_trigger_volumes(&triggers);
            let mut grounded = run.get("grounded").and_then(Value::as_bool).unwrap();
            profile.set_grounded(grounded);
            let mut position = read_vec(run.get("start").and_then(Value::as_array).unwrap());
            let mut velocity = read_vec(run.get("startVelocity").and_then(Value::as_array).unwrap());
            let clock = run.get("clock").and_then(Value::as_str).unwrap();
            let base = run.get("base").and_then(Value::as_f64).unwrap();
            let mut kb_age: Option<f64> = None;
            let inputs = run.get("inputs").and_then(Value::as_array).unwrap();
            let expected = run.get("steps").and_then(Value::as_array).unwrap();
            for (step, (raw, want)) in inputs.iter().zip(expected).enumerate() {
                let raw = raw.as_array().unwrap();
                let buttons = raw[0].as_f64().unwrap() as u32;
                let (yaw, pitch, scale) = (read_num(&raw[1]), read_num(&raw[2]), read_num(&raw[3]));
                let op = raw[4].as_str().unwrap();
                let op_arg = raw[5].as_bool().unwrap();
                match op {
                    "setGrounded" => profile.set_grounded(op_arg),
                    "reset" => profile.reset_physics_state(op_arg),
                    "bonus" => profile.speed_bonus = if op_arg { 25.0 } else { 0.0 },
                    "knock" => {
                        velocity.x += if op_arg { 400.0 } else { -250.0 };
                        velocity.y += 300.0;
                        kb_age = Some(if op_arg { 0.0 } else { 60.0 });
                    }
                    _ => {}
                }
                let clean = sanitize_movement_or_zero(MovementFields {
                    direction: Some(wish(buttons, yaw, scale)),
                    pitch: Some(pitch),
                    yaw: Some(yaw),
                    buttons: Some(f64::from(buttons)),
                });
                let seq = base + step as f64;
                let input = MovementInput {
                    direction: Vec3::new(clean.direction[0], clean.direction[1], clean.direction[2]),
                    view_angles: ViewAngles {
                        pitch: clean.pitch,
                        yaw: clean.yaw,
                    },
                    buttons: clean.buttons,
                    sequence: (clock != "none").then_some(seq),
                    sim_step: (clock == "simStep").then_some(seq),
                };
                let mut actors = [LiftActor {
                    position,
                    velocity,
                    id: Some(1),
                    passive: false,
                }];
                world.backend.backend.step_lifts(DT, &mut actors);
                (position, velocity) = (actors[0].position, actors[0].velocity);
                if let Some(age) = kb_age {
                    kb_age = apply_knockback_drag(&mut velocity, grounded, age + 1.0, DT);
                }
                let r = profile.simulate(position, velocity, Some(&input), DT, Some(&mut world));
                position = r.position;
                velocity = r.velocity;
                grounded = r.grounded;
                let offsets = world
                    .backend
                    .backend
                    .lifts
                    .as_ref()
                    .map(|l| l.offsets())
                    .unwrap_or_default();
                let line = canonical(&clean, &r, &profile, &offsets, kb_age);
                let want = want.as_array().unwrap();
                let got_pv = [position.x, position.y, position.z, velocity.x, velocity.y, velocity.z].map(num);
                let want_pv: Vec<&str> = want[..6].iter().map(|x| x.as_str().unwrap()).collect();
                let hash = want[6].as_str().unwrap();
                assert!(
                    got_pv.iter().zip(&want_pv).all(|(a, b)| a == b) && fnv(&line) == hash,
                    "map {m} run {run_index} step {step}: position/velocity {got_pv:?}, wash-fps {want_pv:?}; \
                     hash {} vs {hash}\nline: {line}\n(DUMP={m},{run_index},{step} prints the TypeScript line)",
                    fnv(&line)
                );
                steps += 1;
            }
        }
    }
    assert!(steps > 20_000, "{steps} steps");
}
