//! wash-fps `client/BombRenderer.ts`: the Search & Destroy bomb in the
//! world.
//!
//! One model, three lives: slung on the carrier's back like a pack, lying on
//! the ground, or planted at a site, flashing in step with the fuse.
//! Everything it draws comes from the server's objective snapshot, so it
//! cannot show a bomb the server does not believe in.
//!
//! The model is the EMP charge from the model pack (`models/bomb_emp.glb`,
//! [`EmpBombModel`]): its purple parts glow, and the lightning disc on each
//! pole ripples. The glow is strongest on a carrier's back, where a purple
//! light also follows it, because who has the bomb is the first thing both
//! teams look for.

use shagger::rendering::dynamic_lights::DynamicLight;
use shagger::rendering::glam::{DMat4, DVec3, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelRenderer};
use shagger::shared::json::Value;

use super::bomb::bomb_floor_decal::BombFloorDecal;
use super::bomb::emp_bomb_model::EmpBombModel;
use shagger::babylon::meshes::vertex_data::babylon_local_matrix;

/// The model spans about 0.92 units; at this scale the bomb is 16 world
/// units across.
const MODEL_SCALE: f64 = 17.4;
/// The bomb's radius at full size, in world units.
const BOMB_RADIUS: f64 = 8.0;

/// Where on the carrier's back it rides, relative to the spine bone, in
/// world units.
const BACK_OFFSET: DVec3 = DVec3::new(0.0, 4.0, 12.0);
const CARRY_SCALE: f64 = 0.75;
/// Shoulder height above the carrier's origin, when there is no bone.
pub const CARRY_FALLBACK_HEIGHT: f64 = 28.0;

/// Idle bob and spin while loose on the ground, so it reads as a pickup.
const LOOSE_SPIN_RATE: f64 = 1.4;
const LOOSE_BOB_AMPLITUDE: f64 = 2.5;
const LOOSE_BOB_RATE: f64 = 2.2;

/// Glow, as multiples of the lit surface: `shell` for the purple parts all
/// over the bomb, `core` for the lightning disc. Above 1, bloom picks them
/// up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
    pub shell: f32,
    pub core: f32,
}

const GLOW_LOOSE: Glow = Glow { shell: 1.6, core: 1.6 };
const GLOW_CARRIED: Glow = Glow { shell: 3.0, core: 2.6 };
const GLOW_PLANTED: Glow = Glow { shell: 1.2, core: 1.2 };
const GLOW_ARMED: Glow = Glow { shell: 1.4, core: 1.4 };
/// The flash at the top of each fuse beep.
const GLOW_BEEP: Glow = Glow { shell: 4.5, core: 4.5 };

/// The purple light that follows a carried bomb.
const CARRY_LIGHT_COLOR: Vec3 = Vec3::new(0.62, 0.18, 1.0);
const CARRY_LIGHT_RANGE: f32 = 110.0;
const CARRY_LIGHT_INTENSITY: f32 = 3.0;

/// The glow on the floor under a bomb that is not carried, as wide as
/// `DEFAULT_BOMB_RULES.armDefuseRadius`: purple while it lies loose, amber
/// once planted, red and pulsing once armed, turning faster as the fuse
/// runs down.
const FLOOR_RADIUS: f64 = 100.0;
const FLOOR_COLOR_LOOSE: Vec3 = Vec3::new(0.62, 0.2, 1.0);
const FLOOR_COLOR_PLANTED: Vec3 = Vec3::new(1.0, 0.5, 0.1);
const FLOOR_COLOR_ARMED: Vec3 = Vec3::new(1.0, 0.12, 0.08);
const FLOOR_STRENGTH_LOOSE: f64 = 1.0;
const FLOOR_STRENGTH_PLANTED: f64 = 1.0;
const FLOOR_STRENGTH_ARMED: f64 = 0.75;
/// Radians a second; armed adds up to `FLOOR_ARMED_SPIN_MAX` more as the
/// beeps quicken.
const FLOOR_SPIN: f64 = 0.5;
const FLOOR_ARMED_SPIN_MAX: f64 = 2.5;
/// Pulses per second while armed.
const FLOOR_PULSE_HZ: f64 = 1.6;

/// The floor glow this frame (`updateFloor`): where, its colour, strength
/// and spin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloorLook {
    pub at: DVec3,
    pub color: Vec3,
    pub strength: f64,
    pub spin: f64,
}

/// The bomb this frame (`update`): the model's root when it is drawn (its
/// scale included), its glow, and the floor glow when it is not carried.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BombPose {
    pub root: Option<DMat4>,
    pub glow: Glow,
    pub floor: Option<FloorLook>,
}

impl BombPose {
    /// Where the carry light goes: on a drawn, carried bomb.
    #[must_use]
    pub fn carry_light(&self, carried: bool) -> Option<DVec3> {
        self.root.filter(|_| carried).map(|root| root.w_axis.truncate())
    }
}

/// The bomb's state as `BombObjectiveSnapshot` sends it.
#[derive(Debug, Clone, PartialEq)]
pub struct BombSnapshot {
    pub phase: String,
    pub position: DVec3,
    pub carrier: Option<String>,
    pub beep_interval: Option<f64>,
}

impl BombSnapshot {
    /// The objective when it is the bomb (`MatchStateManager.bomb`).
    #[must_use]
    pub fn from_objective(bomb: &Value) -> Option<Self> {
        let number = |k: &str| bomb.get(k).and_then(Value::as_f64);
        Some(Self {
            phase: bomb.get("phase")?.as_str()?.to_owned(),
            position: DVec3::new(number("x")?, number("y")?, number("z")?),
            carrier: bomb.get("carrier").and_then(Value::as_str).map(str::to_owned),
            beep_interval: number("beepInterval"),
        })
    }
}

/// `updateFloor`: the glow on the floor wherever the bomb sits, hidden
/// while carried (the radius means nothing on a moving back).
#[must_use]
pub fn floor_look(bomb: &BombSnapshot, elapsed: f64) -> Option<FloorLook> {
    let (color, strength, spin) = match bomb.phase.as_str() {
        "carried" => return None,
        "armed" => {
            let pulse = 0.5 + 0.5 * (elapsed * FLOOR_PULSE_HZ * std::f64::consts::PI * 2.0).sin();
            // 5 s between beeps at the start of the fuse, 0.2 s at the end
            let urgency = match bomb.beep_interval {
                Some(interval) if interval > 0.0 => (0.2 / interval).min(1.0),
                _ => 0.0,
            };
            (
                FLOOR_COLOR_ARMED,
                FLOOR_STRENGTH_ARMED * (0.6 + 0.6 * pulse),
                FLOOR_SPIN + FLOOR_ARMED_SPIN_MAX * urgency,
            )
        }
        "planted" => (FLOOR_COLOR_PLANTED, FLOOR_STRENGTH_PLANTED, FLOOR_SPIN),
        _ => (FLOOR_COLOR_LOOSE, FLOOR_STRENGTH_LOOSE, FLOOR_SPIN),
    };
    Some(FloorLook {
        at: bomb.position,
        color,
        strength,
        spin,
    })
}

/// `updateGlow`: how brightly the bomb glows. Armed, it flashes at the top
/// of each beep interval, the same number the audio uses, so the flash and
/// the sound are one signal.
#[must_use]
pub fn glow_look(phase: &str, beep_interval: Option<f64>, elapsed: f64) -> Glow {
    match phase {
        "carried" => GLOW_CARRIED,
        "planted" => GLOW_PLANTED,
        "armed" => match beep_interval {
            // A short flash rather than a sine: a blink reads as an event.
            Some(interval) if interval > 0.0 && (elapsed % interval) / interval < 0.25 => GLOW_BEEP,
            _ => GLOW_ARMED,
        },
        _ => GLOW_LOOSE,
    }
}

/// `update`'s pose for a bomb `elapsed` seconds into its life. `anchor` is
/// the carrier's spine (or shoulder height) and yaw, when they are drawn.
#[must_use]
pub fn bomb_pose(bomb: &BombSnapshot, elapsed: f64, anchor: Option<(DVec3, f64)>) -> Option<BombPose> {
    if bomb.phase == "defused" || bomb.phase == "detonated" {
        return None;
    }
    let root = if bomb.phase == "carried" && bomb.carrier.is_some() {
        // `carry`: behind them by their own facing, turned with them so one
        // lightning disc faces whoever is chasing; nobody to hang it on
        // (out of view) hides it.
        anchor.map(|(at, yaw)| {
            let back = BACK_OFFSET.z;
            let position = DVec3::new(at.x - yaw.sin() * back, at.y + BACK_OFFSET.y, at.z - yaw.cos() * back);
            babylon_local_matrix(
                position,
                DVec3::new(0.0, yaw, 0.0),
                DVec3::splat(CARRY_SCALE * MODEL_SCALE),
            )
        })
    } else {
        // `place`: still and upright when planted, turning and bobbing
        // loose; the bob rests on the floor at the bottom of each swing.
        let planted = bomb.phase == "planted" || bomb.phase == "armed";
        let bob = if planted {
            0.0
        } else {
            (1.0 + (elapsed * LOOSE_BOB_RATE).sin()) * LOOSE_BOB_AMPLITUDE
        };
        let spin = if planted { 0.0 } else { elapsed * LOOSE_SPIN_RATE };
        Some(babylon_local_matrix(
            bomb.position + DVec3::new(0.0, BOMB_RADIUS + bob, 0.0),
            DVec3::new(0.0, spin, 0.0),
            DVec3::splat(MODEL_SCALE),
        ))
    };
    Some(BombPose {
        root,
        glow: glow_look(&bomb.phase, bomb.beep_interval, elapsed),
        floor: floor_look(bomb, elapsed),
    })
}

/// `BombRenderer`: the model (when it loaded), the floor glow and the
/// bomb's clock.
pub struct BombRenderer {
    /// The model, once loaded. Without it only the floor glow draws.
    model: Option<EmpBombModel>,
    floor: BombFloorDecal,
    /// Seconds for the loose bomb's bob, the flashes and the ripple.
    elapsed: f64,
    pose: Option<BombPose>,
    /// The purple light on a carrier, while there is one.
    carry_light: Option<DynamicLight>,
}

impl BombRenderer {
    /// The renderer, `file` reading the model from the packages.
    pub fn new(gpu: &Gpu, models: &mut ModelRenderer, file: impl FnOnce(&str) -> Option<Vec<u8>>) -> Self {
        let model = EmpBombModel::load(gpu, models, file)
            .map_err(|why| eprintln!("bomb model: {why}"))
            .ok();
        // Its own draw, not under the bomb's matrix: the bomb bobs and rides
        // a carrier's spine, and a floor glow doing either would stop
        // reading as the ground around it.
        let floor = BombFloorDecal::new(gpu, models, FLOOR_RADIUS);
        Self {
            model,
            floor,
            elapsed: 0.0,
            pose: None,
            carry_light: None,
        }
    }

    /// `update`: the bomb this frame from the objective, `anchor` finding a
    /// drawn carrier's spine and yaw. `dt` is seconds.
    pub fn update(&mut self, dt: f64, bomb: Option<&Value>, anchor: impl FnOnce(&str) -> Option<(DVec3, f64)>) {
        let bomb = bomb.and_then(BombSnapshot::from_objective);
        let Some(bomb) = bomb.filter(|b| b.phase != "defused" && b.phase != "detonated") else {
            // `hide`: the floor glow and the light go with it.
            self.pose = None;
            self.floor.hide();
            self.carry_light = None;
            return;
        };
        self.elapsed += dt;
        let anchor = match (&bomb.carrier, bomb.phase.as_str()) {
            (Some(carrier), "carried") => anchor(carrier),
            _ => None,
        };
        self.pose = bomb_pose(&bomb, self.elapsed, anchor);
        match self.pose.and_then(|p| p.floor) {
            Some(look) => self.floor.show(look.at, look.color, look.strength, look.spin, dt),
            None => self.floor.hide(),
        }
        // `updateCarryLight`: the frame drops it with `r_dynamiclight 0`.
        self.carry_light = self
            .pose
            .and_then(|p| p.carry_light(bomb.phase == "carried"))
            .map(|at| {
                let color = CARRY_LIGHT_COLOR * CARRY_LIGHT_INTENSITY;
                DynamicLight {
                    position: at.as_vec3(),
                    diffuse: color,
                    specular: color,
                    range: CARRY_LIGHT_RANGE,
                }
            });
    }

    /// The purple light on the carrier this frame.
    #[must_use]
    pub fn carry_light(&self) -> Option<DynamicLight> {
        self.carry_light
    }

    /// The bomb's draws this frame.
    pub fn draws(&self, draws: &mut Vec<ModelDraw>) {
        let Some(pose) = self.pose else { return };
        if let (Some(root), Some(model)) = (pose.root, &self.model) {
            model.draws(
                root.as_mat4(),
                pose.glow.shell,
                pose.glow.core,
                self.elapsed as f32,
                draws,
            );
        }
        self.floor.draws(draws);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bomb(phase: &str) -> BombSnapshot {
        BombSnapshot {
            phase: phase.into(),
            position: DVec3::new(100.0, 20.0, -50.0),
            carrier: (phase == "carried").then(|| "p1".into()),
            beep_interval: (phase == "armed").then_some(1.0),
        }
    }

    #[test]
    fn a_loose_bomb_bobs_above_its_floor_glow() {
        let pose = bomb_pose(&bomb("dropped"), 0.0, None).unwrap();
        let root = pose.root.unwrap();
        // Its radius plus the bob's midpoint above the floor.
        assert_eq!(root.w_axis.truncate(), DVec3::new(100.0, 30.5, -50.0));
        assert!((root.x_axis.truncate().length() - MODEL_SCALE).abs() < 1e-9);
        let floor = pose.floor.unwrap();
        assert_eq!(floor.at, DVec3::new(100.0, 20.0, -50.0));
        assert_eq!((floor.color, floor.strength, floor.spin), (FLOOR_COLOR_LOOSE, 1.0, 0.5));
        assert_eq!(pose.glow, GLOW_LOOSE);
    }

    #[test]
    fn a_carried_bomb_rides_behind_its_carrier_or_hides() {
        let carried = bomb("carried");
        let pose = bomb_pose(&carried, 1.0, Some((DVec3::new(0.0, 40.0, 0.0), 0.0))).unwrap();
        let root = pose.root.unwrap();
        // Behind: yaw 0 faces +Z, so the bomb is 12 units toward -Z, 4 up.
        assert!((root.w_axis.truncate() - DVec3::new(0.0, 44.0, -12.0)).length() < 1e-9);
        assert!((root.x_axis.truncate().length() - 0.75 * MODEL_SCALE).abs() < 1e-9);
        assert!(pose.floor.is_none());
        assert_eq!(pose.glow, GLOW_CARRIED);
        assert!(pose.carry_light(true).is_some());
        let hidden = bomb_pose(&carried, 1.0, None).unwrap();
        assert!(hidden.root.is_none());
        assert!(hidden.carry_light(true).is_none());
    }

    #[test]
    fn the_armed_bomb_flashes_with_the_beep() {
        assert_eq!(glow_look("armed", Some(1.0), 0.1), GLOW_BEEP);
        assert_eq!(glow_look("armed", Some(1.0), 0.5), GLOW_ARMED);
        assert_eq!(glow_look("armed", None, 0.1), GLOW_ARMED);
        assert_eq!(glow_look("planted", None, 0.5), GLOW_PLANTED);
        assert!(bomb_pose(&bomb("defused"), 0.0, None).is_none());
    }

    #[test]
    fn the_armed_floor_glow_pulses_red_and_turns_faster_as_the_fuse_runs() {
        let mut armed = bomb("armed");
        let look = floor_look(&armed, 0.0).unwrap();
        assert_eq!(look.color, FLOOR_COLOR_ARMED);
        assert!((look.strength - 0.75 * 0.9).abs() < 1e-12);
        assert!((look.spin - (0.5 + 2.5 * 0.2)).abs() < 1e-12);
        armed.beep_interval = Some(0.2);
        assert!((floor_look(&armed, 0.0).unwrap().spin - 3.0).abs() < 1e-12);
        assert_eq!(floor_look(&bomb("planted"), 0.0).unwrap().color, FLOOR_COLOR_PLANTED);
    }

    #[test]
    fn the_objective_reads_as_a_snapshot() {
        let v = shagger::shared::json::parse(
            r#"{"kind":"bomb","phase":"armed","x":1,"y":2,"z":3,"carrier":null,"beepInterval":0.5}"#,
        )
        .unwrap();
        let b = BombSnapshot::from_objective(&v).unwrap();
        assert_eq!(b.phase, "armed");
        assert_eq!(b.carrier, None);
        assert_eq!(b.beep_interval, Some(0.5));
    }
}
