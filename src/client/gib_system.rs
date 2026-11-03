//! Port of wash-fps `client/GibSystem.ts`: a body breaking up digitally
//! into tumbling shards on a gib death (a rail, the bomb, a big explosion).
//! Twenty flying shards (the first five "hero" shards bounce off the map
//! with real traces, the rest off the death height) and eight body-part
//! drops; each bounces once, tumbles, drifts and fades from 6.5 to 8 s.
//!
//! The TypeScript pools a mesh per shard with a StandardMaterial per shade;
//! here every shard is a draw of one tetrahedron, uploaded twice (opaque,
//! and blended for the fade), tinted and lit per draw.

use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelMaterial, ModelPrimitive, Skeleton};
use shagger::rendering::glam::{DVec3, EulerRot, Mat4, Quat, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};

pub const GIB_COUNT: usize = 20;
pub const GIB_LIFETIME: f64 = 8.0;
pub const GIB_FADE_START: f64 = 6.5;
pub const GIB_SIZE: f64 = 6.0;
pub const GRAVITY: f64 = -360.0;
pub const UPWARD_IMPULSE_MIN: f64 = 200.0;
pub const UPWARD_IMPULSE_MAX: f64 = 400.0;
pub const RANDOM_SPREAD: f64 = 180.0;
pub const TUMBLE_SPEED_MIN: f64 = 3.0;
pub const TUMBLE_SPEED_MAX: f64 = 12.0;
pub const MAX_ACTIVE_GIBS: usize = 200;
pub const TRACED_GIB_COUNT: usize = 5;
pub const TRACE_GRACE_PERIOD: f64 = 0.15;
pub const DROP_GIB_COUNT: usize = 8;
pub const DROP_GIB_SIZE: f64 = 8.0;
pub const DROP_GRAVITY: f64 = -500.0;

/// `BODY_PART_OFFSETS`: head, shoulders, elbows, torso, hips.
pub const BODY_PART_OFFSETS: [[f64; 3]; 8] = [
    [0.0, 60.0, 0.0],
    [-15.0, 45.0, 0.0],
    [15.0, 45.0, 0.0],
    [-20.0, 30.0, 0.0],
    [20.0, 30.0, 0.0],
    [0.0, 20.0, 0.0],
    [-8.0, 5.0, 0.0],
    [8.0, 5.0, 0.0],
];

/// `GibShade`: a shard's diffuse and emissive colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GibShade {
    pub diffuse: Vec3,
    pub emissive: Vec3,
}

/// `GIB_COLORS`: pink, teal and grey, for a body of unknown colour.
pub const GIB_COLORS: [GibShade; 3] = [
    GibShade {
        diffuse: Vec3::new(1.0, 0.2, 0.6),
        emissive: Vec3::new(0.5, 0.05, 0.2),
    },
    GibShade {
        diffuse: Vec3::new(0.0, 0.87, 0.75),
        emissive: Vec3::new(0.0, 0.3, 0.25),
    },
    GibShade {
        diffuse: Vec3::new(0.53, 0.56, 0.63),
        emissive: Vec3::new(0.05, 0.05, 0.08),
    },
];

/// `gibPalette`: the player's hue at full brightness, a lighter tint and a
/// darker shade (which the body-part drops take).
#[must_use]
pub fn gib_palette(color: Vec3) -> [GibShade; 3] {
    let peak = color.x.max(color.y).max(color.z);
    let scale = if peak.is_finite() && peak > 1.0 {
        1.0 / peak
    } else {
        1.0
    };
    let base = (color * scale).max(Vec3::ZERO);
    let shade = |diffuse: Vec3, glow: f32| GibShade {
        diffuse,
        emissive: diffuse * glow,
    };
    [
        shade(base, 0.45),
        shade(base.lerp(Vec3::ONE, 0.5), 0.4),
        shade(base * 0.4, 0.15),
    ]
}

/// One shard (`Gib`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gib {
    pub pos: DVec3,
    pub vel: DVec3,
    pub rot: DVec3,
    pub tumble: DVec3,
    pub scale: f64,
    pub shade: GibShade,
    pub age: f64,
    pub bounced: bool,
    pub floor_y: f64,
    pub traced: bool,
    pub is_drop: bool,
}

impl Gib {
    /// `mesh.visibility`: whole until the fade, then to nothing.
    #[must_use]
    pub fn visibility(&self) -> f64 {
        if self.age > GIB_FADE_START {
            1.0 - (self.age - GIB_FADE_START) / (GIB_LIFETIME - GIB_FADE_START)
        } else {
            1.0
        }
    }

    /// The shard's world matrix: Babylon's `rotation` (yaw, pitch, roll
    /// about Y, X and Z) and its scaling.
    #[must_use]
    pub fn world(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::splat(self.scale as f32),
            Quat::from_euler(EulerRot::YXZ, self.rot.y as f32, self.rot.x as f32, self.rot.z as f32),
            self.pos.as_vec3(),
        )
    }
}

/// A bounce off the map: where, and the surface's normal.
pub type GibTrace<'a> = &'a mut dyn FnMut(DVec3, DVec3) -> Option<(DVec3, DVec3)>;

/// `GibSystem`.
#[derive(Debug, Clone)]
pub struct GibSystem {
    gibs: Vec<Gib>,
    /// `Math.random`'s stand-in (xorshift64*).
    seed: u64,
}

impl Default for GibSystem {
    fn default() -> Self {
        Self {
            gibs: Vec::new(),
            seed: 0x51_7cc1_b727_220a,
        }
    }
}

impl GibSystem {
    fn random(&mut self) -> f64 {
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        (self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `spawn`: a body at `at` moving at `vel` breaks up, in the colour it
    /// was drawn with (`color`), or the default palette when unknown.
    /// Nothing past `MAX_ACTIVE_GIBS` live shards.
    pub fn spawn(&mut self, at: DVec3, vel: DVec3, color: Option<Vec3>) {
        if self.gibs.len() >= MAX_ACTIVE_GIBS {
            return;
        }
        let shades = color.map_or(GIB_COLORS, gib_palette);
        for i in 0..GIB_COUNT {
            let pos = DVec3::new(
                at.x + (self.random() - 0.5) * 10.0,
                at.y + 20.0 + self.random() * 10.0,
                at.z + (self.random() - 0.5) * 10.0,
            );
            let upward = UPWARD_IMPULSE_MIN + self.random() * (UPWARD_IMPULSE_MAX - UPWARD_IMPULSE_MIN);
            let spread = DVec3::new(
                (self.random() - 0.5) * 2.0 * RANDOM_SPREAD,
                self.random() * RANDOM_SPREAD * 0.5,
                (self.random() - 0.5) * 2.0 * RANDOM_SPREAD,
            );
            let vel = DVec3::new(vel.x + spread.x, vel.y + upward + spread.y, vel.z + spread.z);
            let gib = self.init_common(pos, vel, 1.0, at.y, i < TRACED_GIB_COUNT, false, &shades);
            self.gibs.push(gib);
        }
        for i in 0..DROP_GIB_COUNT {
            let offset = BODY_PART_OFFSETS[i % BODY_PART_OFFSETS.len()];
            let pos = DVec3::new(
                at.x + offset[0] + (self.random() - 0.5) * 4.0,
                at.y + offset[1],
                at.z + offset[2] + (self.random() - 0.5) * 4.0,
            );
            let vel = DVec3::new(
                offset[0] * 3.0 + (self.random() - 0.5) * 40.0,
                20.0 + self.random() * 30.0,
                offset[2] * 3.0 + (self.random() - 0.5) * 40.0,
            );
            let scale = (0.9 + self.random() * 0.4) * (DROP_GIB_SIZE / GIB_SIZE);
            let gib = self.init_common(pos, vel, scale, at.y, false, true, &shades);
            self.gibs.push(gib);
        }
    }

    /// `initGibCommon`: spin, tumble, shade and size.
    #[allow(clippy::too_many_arguments)]
    fn init_common(
        &mut self,
        pos: DVec3,
        vel: DVec3,
        drop_scale: f64,
        floor_y: f64,
        traced: bool,
        is_drop: bool,
        shades: &[GibShade; 3],
    ) -> Gib {
        let tau = std::f64::consts::TAU;
        let rot = DVec3::new(self.random() * tau, self.random() * tau, self.random() * tau);
        let (t_min, t_max) = if is_drop {
            (1.0, 6.0)
        } else {
            (TUMBLE_SPEED_MIN, TUMBLE_SPEED_MAX)
        };
        let mut tumble = [0.0; 3];
        for t in &mut tumble {
            let speed = t_min + self.random() * (t_max - t_min);
            *t = if self.random() > 0.5 { speed } else { -speed };
        }
        let (shade, scale) = if is_drop {
            (shades[2], drop_scale)
        } else {
            let pick = ((self.random() * 3.0) as usize).min(2);
            (shades[pick], 0.7 + self.random() * 0.6)
        };
        Gib {
            pos,
            vel,
            rot,
            tumble: DVec3::from(tumble),
            scale,
            shade,
            age: 0.0,
            // Drops skip the bounce and fall through.
            bounced: is_drop,
            floor_y,
            traced,
            is_drop,
        }
    }

    /// `update`: fall, bounce once, drift, tumble, fade. `trace` finds
    /// where a hero shard's path meets the map, when there is a map.
    pub fn update(&mut self, dt: f64, mut trace: Option<GibTrace<'_>>) {
        for gib in &mut self.gibs {
            gib.age += dt;
            if gib.age >= GIB_LIFETIME {
                continue;
            }
            gib.vel.y += if gib.is_drop { DROP_GRAVITY } else { GRAVITY } * dt;
            gib.pos += gib.vel * dt;
            if !gib.bounced {
                let mut bounced = false;
                if gib.traced
                    && gib.age > TRACE_GRACE_PERIOD
                    && let Some(trace) = trace.as_mut()
                {
                    let speed = gib.vel.length();
                    if speed > 10.0 {
                        // `traceBounce`: this frame's path and a shard's reach.
                        let reach = speed * dt + GIB_SIZE;
                        let end = gib.pos + gib.vel / speed * reach;
                        if let Some((point, n)) = trace(gib.pos, end) {
                            let dot = gib.vel.dot(n);
                            gib.vel = (gib.vel - 2.0 * dot * n) * 0.3;
                            gib.pos = point + n * GIB_SIZE * 0.3;
                            bounced = true;
                        }
                    }
                }
                if !bounced && gib.vel.y < 0.0 && gib.pos.y <= gib.floor_y + GIB_SIZE * 0.3 {
                    gib.pos.y = gib.floor_y + GIB_SIZE * 0.3;
                    gib.vel.y = -gib.vel.y * 0.3;
                    gib.vel.x *= 0.5;
                    gib.vel.z *= 0.5;
                    bounced = true;
                }
                if bounced {
                    gib.tumble *= 0.4;
                    gib.bounced = true;
                }
            }
            gib.vel.x *= 1.0 - 0.3 * dt;
            gib.vel.z *= 1.0 - 0.3 * dt;
            gib.rot += gib.tumble * dt;
        }
        self.gibs.retain(|g| g.age < GIB_LIFETIME);
    }

    /// `clear`: seeks drop every shard.
    pub fn clear(&mut self) {
        self.gibs.clear();
    }

    #[must_use]
    pub fn gibs(&self) -> &[Gib] {
        &self.gibs
    }
}

/// `createTriangleMesh`: an irregular tetrahedron, faces unshared for flat
/// shading.
#[must_use]
pub fn shard_primitive() -> ModelPrimitive {
    let s = GIB_SIZE as f32;
    let v0 = [0.0, s * 0.7, 0.0];
    let v1 = [-s * 0.5, -s * 0.2, s * 0.3];
    let v2 = [s * 0.5, -s * 0.2, s * 0.3];
    let v3 = [0.0, -s * 0.3, -s * 0.4];
    let positions = vec![v0, v1, v2, v0, v3, v1, v0, v2, v3, v1, v3, v2];
    let mut normals = Vec::with_capacity(12);
    for face in positions.as_chunks::<3>().0 {
        let (a, b, c) = (Vec3::from(face[0]), Vec3::from(face[1]), Vec3::from(face[2]));
        let n = (b - a).cross(c - a).normalize_or_zero();
        normals.extend([n.to_array(); 3]);
    }
    ModelPrimitive {
        name: "gib_template".to_owned(),
        node: 0,
        node_scale: Vec3::ONE,
        node_matrix: Mat4::IDENTITY,
        positions,
        normals,
        uvs: vec![[0.0; 2]; 12],
        // Babylon's winding, flipped as the procedural pickups' are.
        indices: vec![2, 1, 0, 5, 4, 3, 8, 7, 6, 11, 10, 9],
        material: Some(0),
        bounds_center: Vec3::ZERO,
        creation_order: 0,
        skin: None,
    }
}

/// The shards' draws: one tetrahedron, opaque and fading.
pub struct GibRenderer {
    opaque: ModelHandle,
    fading: ModelHandle,
}

impl GibRenderer {
    /// Uploads the shard with `createMaterials`' material: lit, no
    /// specular, both sides.
    pub fn new(gpu: &Gpu, models: &mut ModelRenderer) -> Self {
        let model = GltfModel {
            primitives: vec![shard_primitive()],
            materials: vec![ModelMaterial {
                name: "gib_mat".to_owned(),
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
            }],
            images: Vec::new(),
            attachment_points: Default::default(),
            skeleton: Skeleton::default(),
        };
        let params = StandardMaterialParams {
            specular_color: Vec3::ZERO,
            back_face_culling: false,
            ..StandardMaterialParams::default()
        };
        let opaque = models.upload(gpu, &model, |_| ModelMaterialMode::Standard(params));
        let fading = models.upload(gpu, &model, |_| {
            ModelMaterialMode::Standard(StandardMaterialParams {
                alpha_blend: true,
                ..params
            })
        });
        Self { opaque, fading }
    }

    /// Every live shard's draw: its shade as the diffuse (`tint`) and
    /// emissive, faded once `visibility` drops.
    pub fn draws(&self, gibs: &GibSystem, draws: &mut Vec<ModelDraw>) {
        for gib in gibs.gibs() {
            let visibility = gib.visibility();
            let fading = visibility < 1.0;
            draws.push(ModelDraw {
                tint: Some(gib.shade.diffuse),
                emissive: Some(gib.shade.emissive),
                alpha: fading.then_some(visibility as f32),
                ..ModelDraw::new(if fading { self.fading } else { self.opaque }, 0, gib.world())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_breaks_into_twenty_eight_shards() {
        let mut gibs = GibSystem::default();
        gibs.spawn(DVec3::new(0.0, 100.0, 0.0), DVec3::new(50.0, 0.0, 0.0), None);
        assert_eq!(gibs.gibs().len(), GIB_COUNT + DROP_GIB_COUNT);
        let flying = &gibs.gibs()[..GIB_COUNT];
        assert_eq!(flying.iter().filter(|g| g.traced).count(), TRACED_GIB_COUNT);
        for g in flying {
            assert!(g.pos.y >= 120.0 && g.pos.y <= 130.0);
            assert!(g.vel.y >= 200.0 && g.vel.y <= 490.0);
            assert!((g.vel.x - 50.0).abs() <= RANDOM_SPREAD);
            assert!(g.tumble.abs().min_element() >= TUMBLE_SPEED_MIN);
            assert!(GIB_COLORS.contains(&g.shade));
            assert!(!g.bounced);
        }
        let head = gibs.gibs()[GIB_COUNT];
        assert_eq!(head.pos.y, 160.0);
        assert!(head.is_drop && head.bounced);
        assert_eq!(head.shade, GIB_COLORS[2]);
    }

    #[test]
    fn shards_bounce_once_off_the_death_height_then_fade() {
        let mut gibs = GibSystem::default();
        gibs.spawn(DVec3::ZERO, DVec3::ZERO, None);
        // The highest throw (490 u/s up, from 30 over the floor) is down
        // in under three seconds.
        for _ in 0..200 {
            gibs.update(1.0 / 60.0, None);
        }
        // Every flying shard has come down onto the floor and bounced.
        assert!(gibs.gibs()[..GIB_COUNT].iter().all(|g| g.bounced));
        for _ in 0..220 {
            gibs.update(1.0 / 60.0, None);
        }
        let g = gibs.gibs()[0];
        assert!(g.visibility() < 1.0 && g.visibility() > 0.0);
        for _ in 0..70 {
            gibs.update(1.0 / 60.0, None);
        }
        assert!(gibs.gibs().is_empty());
    }

    #[test]
    fn hero_shards_bounce_off_the_map() {
        let mut gibs = GibSystem::default();
        gibs.spawn(DVec3::ZERO, DVec3::ZERO, None);
        // A ceiling at 140: anything crossing it reflects downward.
        let mut ceiling = |from: DVec3, to: DVec3| {
            (to.y >= 140.0 && from.y < 140.0).then(|| (DVec3::new(from.x, 140.0, from.z), DVec3::NEG_Y))
        };
        for _ in 0..30 {
            gibs.update(1.0 / 60.0, Some(&mut ceiling));
        }
        let heroes = &gibs.gibs()[..TRACED_GIB_COUNT];
        assert!(heroes.iter().any(|g| g.bounced && g.vel.y < 0.0));
        assert!(gibs.gibs()[TRACED_GIB_COUNT..GIB_COUNT].iter().all(|g| !g.bounced));
    }

    #[test]
    fn a_coloured_body_keeps_its_hue() {
        let p = gib_palette(Vec3::new(0.1, 1.15, 1.85));
        assert!((p[0].diffuse.z - 1.0).abs() < 1e-6);
        assert!((p[0].emissive - p[0].diffuse * 0.45).length() < 1e-6);
        assert!((p[2].diffuse - p[0].diffuse * 0.4).length() < 1e-6);
        let mut gibs = GibSystem::default();
        for _ in 0..8 {
            gibs.spawn(DVec3::ZERO, DVec3::ZERO, None);
        }
        assert_eq!(gibs.gibs().len(), 8 * 28);
        gibs.spawn(DVec3::ZERO, DVec3::ZERO, None);
        assert_eq!(gibs.gibs().len(), 8 * 28, "past 200 live shards, none");
    }
}
