//! wash-fps `client/HitSpatter.ts`: what a hit knocks out of a body:
//! blood, or sparks off a robot.
//!
//! A hit sprays tiny pieces from where it landed (the head, the body or the
//! legs), away from the shooter, as many as the hit was hard: red cubes from
//! a person, orange and yellow triangles from a robot. A headshot sprays
//! more, with a short puff at the head. Blood that reaches the floor leaves a
//! few flat splats there for a few seconds (`cl_blood_splats 0` turns them
//! off).
//!
//! Kept cheap, as the TypeScript is: pieces shrink away rather than fade, so
//! nothing blends; the floor comes from one trace per hit; and there is a
//! fixed cap on pieces and splats, the oldest giving way. Babylon's thin
//! instances become one draw a piece of a shared unlit mesh per shade.

use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::babylon::meshes::mesh_builder::create_box;
use shagger::babylon::meshes::vertex_data::{VertexData, babylon_local_matrix};
use shagger::core::cvar::{CvarOptions, CvarType, Cvars};
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelMaterial, Skeleton};
use shagger::rendering::glam::{DVec3, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};
use shagger::shared::bsp::spawn_point_table::PLAYER_ORIGIN_HEIGHT;
use shagger::shared::js::js_hypot;

use crate::world::pickups::procedural::procedural_pickup_factory::primitive;

/// Registers `cl_blood_splats`.
pub fn register_hit_spatter_cvars(cvars: &mut Cvars) {
    cvars.register(
        "cl_blood_splats",
        1,
        CvarOptions::describe("Blood leaves splats on the floor where it lands (0 = off, 1 = on)")
            .range(0.0, 1.0)
            .kind(CvarType::Int),
    );
}

/// Where a hit landed, as the damage event names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitZone {
    Head,
    Body,
    Legs,
}

impl HitZone {
    /// The damage event's `hitLocation`; anything else is a body hit.
    #[must_use]
    pub fn from_location(location: Option<&str>) -> Self {
        match location {
            Some("head") => Self::Head,
            Some("legs") => Self::Legs,
            _ => Self::Body,
        }
    }

    /// `ZONE_HEIGHT`: above a player's origin, on a normal body.
    fn height(self) -> f64 {
        match self {
            Self::Head => 30.0,
            Self::Body => 8.0,
            Self::Legs => -14.0,
        }
    }
}

/// `scaledBodyHeight` (`shared/BodyScale.ts`): where a point `normal` above
/// the origin of a normal body is on one `scale` times as big.
#[must_use]
pub fn scaled_body_height(normal: f64, scale: f64) -> f64 {
    if scale == 1.0 {
        normal
    } else {
        normal * scale + PLAYER_ORIGIN_HEIGHT * (scale - 1.0)
    }
}

/// Damage that knocks nothing out of a body (`BLOODLESS_DAMAGE`).
#[must_use]
pub fn bloodless(weapon_id: &str) -> bool {
    matches!(weapon_id, "fall" | "fall_crush" | "world")
}

/// Most pieces in the air at once; a new hit takes the oldest's place.
pub const MAX_PIECES: usize = 256;
/// Most splats on the floor at once.
pub const MAX_SPLATS: usize = 48;
/// How long a splat lies, and over how much of its end it shrinks away.
const SPLAT_LIFE: f64 = 6.0;
const SPLAT_SHRINK: f64 = 1.5;
/// How far below a hit the floor is looked for.
pub const FLOOR_REACH: f64 = 256.0;

/// The colours, one mesh each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shade {
    #[default]
    Blood,
    DarkBlood,
    Orange,
    Yellow,
}

const SHADE_COLORS: [[f32; 3]; 4] = [
    [0.72, 0.03, 0.04],
    [0.42, 0.01, 0.02],
    [1.0, 0.5, 0.08],
    [1.0, 0.86, 0.22],
];
const SPLAT_COLOR: [f32; 3] = [0.36, 0.01, 0.02];

#[derive(Debug, Clone, Default)]
pub struct Piece {
    alive: bool,
    pub shade: Shade,
    pub pos: DVec3,
    pub vel: DVec3,
    gravity: f64,
    age: f64,
    life: f64,
    size: f64,
    spin: f64,
    /// The floor under its hit, or `None` when none was found.
    floor: Option<f64>,
    /// Leaves a splat where it lands.
    splats: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Splat {
    alive: bool,
    pub pos: DVec3,
    size: f64,
    turn: f64,
    age: f64,
}

/// `HitSpatter`.
#[derive(Debug)]
pub struct HitSpatter {
    pieces: Vec<Piece>,
    splats: Vec<Splat>,
    next_piece: usize,
    next_splat: usize,
    /// `Math.random`'s stand-in (xorshift64*).
    seed: u64,
}

impl Default for HitSpatter {
    fn default() -> Self {
        Self {
            pieces: Vec::new(),
            splats: Vec::new(),
            next_piece: 0,
            next_splat: 0,
            seed: 0x2f6b_93d1_c4a5_0e17,
        }
    }
}

/// What a hit is: where, how hard, which way, and on what body.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    /// The player's position.
    pub origin: DVec3,
    pub zone: HitZone,
    pub damage: f64,
    /// From the shooter to the player; `None` sprays any way.
    pub away: Option<(f64, f64)>,
    /// Sparks instead of blood.
    pub robot: bool,
    /// The body's size (a campaign boss is bigger).
    pub scale: f64,
}

impl HitSpatter {
    fn random(&mut self) -> f64 {
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        (self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn between(&mut self, min: f64, max: f64) -> f64 {
        min + self.random() * (max - min)
    }

    /// Where a hit at `origin` in `zone` sprays from.
    #[must_use]
    pub fn hit_point(hit: &Hit) -> DVec3 {
        hit.origin + DVec3::Y * scaled_body_height(hit.zone.height(), hit.scale)
    }

    /// `spawn`: `floor` is the floor straight under the hit point (one
    /// trace for all its pieces), `splats_on` the `cl_blood_splats` setting.
    pub fn spawn(&mut self, hit: &Hit, floor: Option<f64>, splats_on: bool) {
        if hit.damage <= 0.0 {
            return;
        }
        let at = Self::hit_point(hit);
        let scale = hit.scale;
        let robot = hit.robot;
        // Unit direction away from the shooter; any way at all without one.
        let (mut dx, mut dz) = hit.away.unwrap_or((0.0, 0.0));
        let length = js_hypot(&[dx, dz]);
        if length < 1e-3 {
            let turn = self.random() * std::f64::consts::TAU;
            dx = turn.cos();
            dz = turn.sin();
        } else {
            dx /= length;
            dz /= length;
        }
        let headshot = hit.zone == HitZone::Head;
        let mut count = (2.0 + hit.damage / 5.0).round().clamp(3.0, 14.0) as usize;
        if headshot {
            count = (count as f64 * 1.5).round() as usize;
        }
        let splatting = !robot && floor.is_some() && splats_on;
        let mut splats_left = if splatting { 3.min(count.div_ceil(4)) } else { 0 };
        for i in 0..count {
            let speed = if robot {
                self.between(160.0, 380.0)
            } else {
                self.between(90.0, 240.0)
            };
            let side = self.between(-1.0, 1.0) * if robot { 160.0 } else { 110.0 };
            let vy = if robot {
                self.between(60.0, 260.0)
            } else {
                self.between(40.0, 220.0)
            };
            let life = if robot {
                self.between(0.2, 0.4)
            } else {
                self.between(0.45, 0.75)
            };
            let size = if robot {
                self.between(2.2, 3.6)
            } else {
                self.between(1.2, 2.4)
            } * scale;
            let spin = self.between(-12.0, 12.0);
            let shade = match (robot, i % 2 == 1, i % 3 == 0) {
                (true, true, _) => Shade::Yellow,
                (true, false, _) => Shade::Orange,
                (false, _, false) => Shade::Blood,
                (false, _, true) => Shade::DarkBlood,
            };
            let splats = splats_left > 0;
            splats_left = splats_left.saturating_sub(1);
            *self.take_piece() = Piece {
                alive: true,
                shade,
                pos: at,
                // Along the shot, out to either side, and up.
                vel: DVec3::new(dx * speed - dz * side, vy, dz * speed + dx * side),
                gravity: if robot { -450.0 } else { -800.0 },
                age: 0.0,
                life,
                size,
                spin,
                floor,
                splats,
            };
        }
        // A headshot puffs: a few big, slow pieces gone almost at once.
        if headshot {
            for _ in 0..5 {
                let turn = self.random() * std::f64::consts::TAU;
                let speed = self.between(20.0, 60.0);
                let vy = self.between(10.0, 50.0);
                let size = if robot {
                    self.between(5.0, 7.0)
                } else {
                    self.between(4.0, 5.5)
                } * scale;
                let spin = self.between(-4.0, 4.0);
                *self.take_piece() = Piece {
                    alive: true,
                    shade: if robot { Shade::Yellow } else { Shade::Blood },
                    pos: at,
                    vel: DVec3::new(turn.cos() * speed, vy, turn.sin() * speed),
                    gravity: 0.0,
                    age: 0.0,
                    life: 0.22,
                    size,
                    spin,
                    floor: None,
                    splats: false,
                };
            }
        }
    }

    /// `update`: every piece on by `dt` seconds, splatting what lands.
    pub fn update(&mut self, dt: f64) {
        let step = dt.max(0.0);
        let mut landed = Vec::new();
        for piece in &mut self.pieces {
            if !piece.alive {
                continue;
            }
            piece.age += step;
            if piece.age >= piece.life {
                piece.alive = false;
                continue;
            }
            piece.vel.y += piece.gravity * step;
            let next_y = piece.pos.y + piece.vel.y * step;
            // Landed: blood that leaves a mark marks the floor; nothing falls
            // through it.
            if let Some(floor) = piece.floor
                && piece.vel.y < 0.0
                && next_y <= floor
            {
                if piece.splats {
                    landed.push((DVec3::new(piece.pos.x, floor, piece.pos.z), piece.size));
                }
                piece.alive = false;
                continue;
            }
            piece.pos.x += piece.vel.x * step;
            piece.pos.y = next_y;
            piece.pos.z += piece.vel.z * step;
        }
        for (at, size) in landed {
            self.add_splat(at, size);
        }
        for splat in &mut self.splats {
            if splat.alive {
                splat.age += step;
                splat.alive = splat.age < SPLAT_LIFE;
            }
        }
    }

    /// `clear`: a seek, a new map.
    pub fn clear(&mut self) {
        self.pieces.iter_mut().for_each(|p| p.alive = false);
        self.splats.iter_mut().for_each(|s| s.alive = false);
    }

    /// `takePiece`: a free piece, or the oldest one when all are flying.
    fn take_piece(&mut self) -> &mut Piece {
        if self.pieces.len() < MAX_PIECES {
            self.pieces.push(Piece::default());
            return self.pieces.last_mut().unwrap_or_else(|| unreachable!());
        }
        let free = (0..MAX_PIECES)
            .map(|i| (self.next_piece + i) % MAX_PIECES)
            .find(|&i| !self.pieces[i].alive);
        let at = free.unwrap_or_else(|| {
            let at = self.next_piece;
            self.next_piece = (self.next_piece + 1) % MAX_PIECES;
            at
        });
        &mut self.pieces[at]
    }

    fn add_splat(&mut self, at: DVec3, size: f64) {
        let size = size * self.between(2.5, 4.5);
        let turn = self.random() * std::f64::consts::PI;
        let splat = Splat {
            alive: true,
            // Just above the floor, so it does not flicker into it.
            pos: at + DVec3::Y * 0.4,
            size,
            turn,
            age: 0.0,
        };
        if self.splats.len() < MAX_SPLATS {
            self.splats.push(splat);
        } else {
            self.splats[self.next_splat] = splat;
            self.next_splat = (self.next_splat + 1) % MAX_SPLATS;
        }
    }

    /// The live pieces, each with its world matrix: shrinking away over its
    /// life and tumbling.
    pub fn piece_poses(&self) -> impl Iterator<Item = (Shade, shagger::rendering::glam::Mat4)> + '_ {
        self.pieces.iter().filter(|p| p.alive).map(|p| {
            let size = p.size * (1.0 - p.age / p.life).sqrt();
            let turn = p.spin * p.age;
            let world = babylon_local_matrix(p.pos, DVec3::new(turn * 0.7, turn, turn * 0.3), DVec3::splat(size));
            (p.shade, world.as_mat4())
        })
    }

    /// The splats: spreading out at once, shrinking away at the end.
    pub fn splat_poses(&self) -> impl Iterator<Item = shagger::rendering::glam::Mat4> + '_ {
        self.splats.iter().filter(|s| s.alive).map(|s| {
            let grow = (s.age / 0.08).min(1.0);
            let shrink = ((SPLAT_LIFE - s.age) / SPLAT_SHRINK).min(1.0);
            let w = s.size * grow * shrink;
            babylon_local_matrix(s.pos, DVec3::new(0.0, s.turn, 0.0), DVec3::new(w, 1.0, w)).as_mat4()
        })
    }

    #[must_use]
    pub fn live_pieces(&self) -> usize {
        self.pieces.iter().filter(|p| p.alive).count()
    }

    #[must_use]
    pub fn live_splats(&self) -> usize {
        self.splats.iter().filter(|s| s.alive).count()
    }
}

/// `shapeData`: a unit cube, a spark shard (a flat triangle seen from both
/// sides), or a flat square splat facing up.
fn shape(kind: usize) -> VertexData {
    match kind {
        0 => create_box(1.0, 1.0, 1.0),
        1 => VertexData {
            positions: vec![0.0, 0.7, 0.0, -0.5, -0.35, 0.0, 0.5, -0.35, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            uvs: vec![0.0; 6],
            indices: vec![0, 1, 2],
        },
        _ => VertexData {
            positions: vec![-0.5, 0.0, -0.5, 0.5, 0.0, -0.5, 0.5, 0.0, 0.5, -0.5, 0.0, 0.5],
            normals: vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            uvs: vec![0.0; 8],
            indices: vec![0, 2, 1, 0, 3, 2],
        },
    }
}

/// The pieces' and splats' draws: one unlit mesh a shade, its own colour
/// in any light, both sides.
pub struct HitSpatterRenderer {
    model: ModelHandle,
}

/// Primitive order: the four shades, then the splat.
const SPLAT_PRIMITIVE: usize = 4;

impl HitSpatterRenderer {
    pub fn new(gpu: &Gpu, models: &mut ModelRenderer) -> Self {
        let colors: Vec<[f32; 3]> = SHADE_COLORS.iter().copied().chain([SPLAT_COLOR]).collect();
        let shapes = [0, 0, 1, 1, 2];
        let primitives = shapes
            .iter()
            .enumerate()
            .map(|(i, &kind)| primitive(&format!("hitspatter-{i}"), &shape(kind), i, i))
            .collect();
        let model = GltfModel {
            primitives,
            materials: (0..colors.len())
                .map(|i| ModelMaterial {
                    name: format!("hitspatter-{i}"),
                    base_color: [1.0; 4],
                    base_color_image: None,
                    metallic: 0.0,
                    roughness: 1.0,
                    metallic_roughness_image: None,
                    normal_image: None,
                    emissive: [0.0; 3],
                    emissive_image: None,
                    alpha_mode: AlphaMode::Opaque,
                    alpha_cutoff: 0.5,
                    double_sided: true,
                })
                .collect(),
            images: Vec::new(),
            attachment_points: Default::default(),
            skeleton: Skeleton::default(),
        };
        let model = models.upload(gpu, &model, |material| {
            let i = material
                .name
                .rsplit('-')
                .next()
                .and_then(|i| i.parse::<usize>().ok())
                .unwrap_or(0);
            let color = colors.get(i).copied().unwrap_or(SPLAT_COLOR);
            ModelMaterialMode::Standard(StandardMaterialParams {
                emissive_color: Vec3::from(color),
                diffuse_color: Vec3::ZERO,
                specular_color: Vec3::ZERO,
                disable_lighting: true,
                back_face_culling: false,
                ..StandardMaterialParams::default()
            })
        });
        Self { model }
    }

    pub fn draws(&self, spatter: &HitSpatter, draws: &mut Vec<ModelDraw>) {
        for (shade, world) in spatter.piece_poses() {
            draws.push(ModelDraw::new(self.model, shade as usize, world));
        }
        for world in spatter.splat_poses() {
            draws.push(ModelDraw::new(self.model, SPLAT_PRIMITIVE, world));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(zone: HitZone, damage: f64, robot: bool) -> Hit {
        Hit {
            origin: DVec3::new(0.0, 100.0, 0.0),
            zone,
            damage,
            away: Some((1.0, 0.0)),
            robot,
            scale: 1.0,
        }
    }

    #[test]
    fn a_harder_hit_sprays_more_and_a_headshot_puffs() {
        let mut s = HitSpatter::default();
        s.spawn(&hit(HitZone::Body, 5.0, false), None, true);
        assert_eq!(s.live_pieces(), 3);
        s.clear();
        s.spawn(&hit(HitZone::Body, 100.0, false), None, true);
        assert_eq!(s.live_pieces(), 14);
        let mut h = HitSpatter::default();
        // round(2 + 50 / 5) = 12, ×1.5 = 18, and the puff's 5.
        h.spawn(&hit(HitZone::Head, 50.0, false), None, true);
        assert_eq!(h.live_pieces(), 18 + 5);
        // Nothing for no damage.
        h.clear();
        h.spawn(&hit(HitZone::Body, 0.0, false), None, true);
        assert_eq!(h.live_pieces(), 0);
    }

    #[test]
    fn pieces_fly_away_from_the_shooter_and_blood_splats_the_floor() {
        let mut s = HitSpatter::default();
        let h = hit(HitZone::Body, 20.0, false);
        s.spawn(&h, Some(60.0), true);
        assert!(s.pieces.iter().all(|p| p.vel.x > 0.0));
        assert!((s.pieces[0].pos.y - 108.0).abs() < 1e-9);
        for _ in 0..120 {
            s.update(1.0 / 60.0);
        }
        assert_eq!(s.live_pieces(), 0);
        // round(2 + 20 / 5) = 6 pieces: up to two splat, if they land in time.
        assert!(s.live_splats() <= 2);
        for _ in 0..(60 * 7) {
            s.update(1.0 / 60.0);
        }
        assert_eq!(s.live_splats(), 0);
    }

    #[test]
    fn robots_spark_and_never_splat() {
        let mut s = HitSpatter::default();
        s.spawn(&hit(HitZone::Legs, 40.0, true), Some(90.0), true);
        assert!(
            s.pieces
                .iter()
                .all(|p| matches!(p.shade, Shade::Orange | Shade::Yellow))
        );
        assert!(s.pieces.iter().all(|p| !p.splats));
    }

    #[test]
    fn a_bigger_body_is_hit_higher() {
        assert_eq!(scaled_body_height(30.0, 1.0), 30.0);
        assert!((scaled_body_height(30.0, 2.0) - (60.0 + PLAYER_ORIGIN_HEIGHT)).abs() < 1e-9);
    }

    #[test]
    fn the_cap_reuses_the_oldest_pieces() {
        let mut s = HitSpatter::default();
        for _ in 0..40 {
            s.spawn(&hit(HitZone::Head, 100.0, false), None, true);
        }
        assert_eq!(s.pieces.len(), MAX_PIECES);
    }
}
