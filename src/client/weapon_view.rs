//! The local player's first-person weapon, as wash-fps hands a weapon
//! definition to SHAG's `WeaponViewManager.loadWeapon` (GLB view models):
//! the model from the packages, uploaded with the view's material
//! fix-ups, placed by [`WeaponView`] under the camera and drawn by its own
//! [`ModelRenderer`] with the weapon camera's projection, lights and
//! outlines.

use shagger::core::cvar::with_cvars;
use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::rendering::camera::Camera;
use shagger::rendering::glam::{DVec3, Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer, Outline};
use shagger::rendering::wgpu;
use shagger::shared::js::js_round;
use shagger::ui::font::Font;
use shagger::weapon::ammo_counter_display::{AmmoCounterDisplay, AmmoCounterTuning, parse_ammo_counter};
use shagger::weapon::muzzle::muzzle_flash_renderer::{MuzzleFlashSpawn, MuzzleFlashStyle};
use shagger::weapon::weapon_view_manager::{
    ShotStyle, ViewModelDef, WEAPON_CAMERA_MAX_Z, WEAPON_CAMERA_MIN_Z, WEAPON_OUTLINE_WIDTH, WeaponView, blade_light,
    view_material_mode, weapon_light, weapon_outline,
};

use crate::client::quad_damage_visual::{
    QUAD_DAMAGE_COLOR, QUAD_VIEW_WEAPON_OUTLINE_MIN, QUAD_VIEW_WEAPON_OUTLINE_RANGE, get_quad_damage_pulse,
};
use crate::shared::entities::definitions::weapons::weapons_table::{WeaponModel, weapon_model};
use crate::world::fps_pickup::package_path;

/// A weapon definition's view-model placement for `WeaponViewManager`.
#[must_use]
pub fn view_model_def(weapon: &WeaponModel) -> ViewModelDef {
    let v = weapon.view;
    ViewModelDef {
        hip_offset: DVec3::from_array(v.hip_offset),
        hip_rotation: DVec3::from_array(v.hip_rotation),
        ads_offset: Some(DVec3::from_array(v.ads_offset)),
        scale: v.scale,
        ads_time: Some(v.ads_time),
        ads_require_ready: v.ads_require_ready,
    }
}

/// `presentFire`'s reading of a weapon's definition (wash-fps
/// `shared/entities/definitions/weapons/*.ts`: `fireRate`, `recoilScale`,
/// `adsRecoilScale`, `meleeRadius`).
#[must_use]
pub fn shot_style(weapon: &str) -> ShotStyle {
    let (fire_rate, recoil_scale, ads_recoil_scale, melee) = match weapon {
        "blade" => (2.0, 1.0, 0.2, true),
        "electrobolt" => (0.8, 4.0, 0.2, false),
        "glauncher" => (1.25, 1.0, 0.2, false),
        "machinegun" => (10.0, 1.6, 0.5, false),
        "plasmagun" => (10.0, 0.4, 0.2, false),
        "riotgun" => (1.25, 4.0, 0.2, false),
        "rlauncher" => (1.25, 3.0, 0.2, false),
        "voltaic" | "lasergun" => (20.0, 0.1, 0.2, false),
        _ => return ShotStyle::default(),
    };
    // `fireIntervalTicks(def) / DEFAULT_PHYSICS_TICK_RATE`.
    let ticks = js_round(TICK_RATE / fire_rate).max(1.0);
    ShotStyle {
        recoil_scale,
        ads_recoil_scale,
        refire_seconds: Some(ticks / TICK_RATE),
        melee,
    }
}

/// A weapon definition's `muzzleFlash` (wash-fps
/// `shared/entities/definitions/weapons/*.ts`), spawned at `position`.
/// `None` for the Voltaic: a beam weapon, whose renderer draws its own
/// muzzle corona (`presentFire`).
#[must_use]
pub fn muzzle_flash(weapon: &str, position: DVec3) -> Option<MuzzleFlashSpawn> {
    use MuzzleFlashStyle::{Arcs, Burst, Plume};
    let (color, duration, style, scale) = match weapon {
        "blade" => ([0.05, 1.0, 0.9], 0.16, Burst, None),
        "electrobolt" => ([0.3, 0.5, 1.0], 0.12, Arcs, Some(2.3)),
        "glauncher" => ([0.6, 1.0, 0.3], 0.15, Plume, Some(2.2)),
        "machinegun" => ([1.0, 0.9, 0.3], 0.075, Burst, None),
        "plasmagun" => ([0.3, 1.0, 0.3], 0.09, Burst, Some(1.4)),
        "riotgun" => ([1.0, 0.7, 0.2], 0.14, Plume, Some(1.4)),
        "rlauncher" => ([1.0, 0.5, 0.1], 0.16, Plume, Some(2.4)),
        _ => return None,
    };
    let intensity = match weapon {
        "blade" | "riotgun" => 25.0,
        "electrobolt" => 35.0,
        "glauncher" => 22.0,
        "machinegun" => 18.0,
        "plasmagun" => 20.0,
        _ => 30.0,
    };
    Some(MuzzleFlashSpawn {
        position,
        color: Vec3::from_array(color),
        style,
        scale,
        intensity: Some(intensity),
        duration: Some(duration),
    })
}

/// `DEFAULT_PHYSICS_TICK_RATE`.
const TICK_RATE: f64 = 125.0;

/// `reloadTime` of a weapon with a clip (`reloadModel: "clip"`), which
/// the view model's reload animation plays over.
#[must_use]
pub fn clip_reload_time(weapon: &str) -> Option<f64> {
    match weapon {
        "machinegun" => Some(1.5),
        "plasmagun" => Some(1.3),
        "riotgun" => Some(0.832),
        "voltaic" | "lasergun" => Some(0.5),
        _ => None,
    }
}

/// The view model and its renderer.
pub struct WeaponViewModel {
    pub renderer: ModelRenderer,
    pub view: WeaponView,
    handle: ModelHandle,
    model: GltfModel,
    draws: Vec<ModelDraw>,
    /// `setPowerupOutlineOverride`: the outline a powerup puts on the gun
    /// in place of the black one, its width before the root's scale.
    pub powerup_outline: Option<Outline>,
    /// The on-gun counter and its `tag_ammo`, for a model with both and an
    /// `ammoCounter` line in its `.cfg`.
    counter: Option<(AmmoCounterDisplay, Mat4)>,
    /// `setAmmoCount`: the clip the counter shows.
    pub ammo: f64,
    /// `muzzleSocket`: the model's `tag_flash` (or `tag_muzzle`), in the
    /// glTF scene's space.
    muzzle: Option<Vec3>,
}

/// `FPSPlayer`'s quad outline on the gun: the quad colour, its width
/// pulsing 0.24 to 0.48 with the body's glow.
#[must_use]
pub fn quad_view_weapon_outline(elapsed: f64) -> Outline {
    let pulse = get_quad_damage_pulse(elapsed);
    Outline {
        color: QUAD_DAMAGE_COLOR,
        width: (QUAD_VIEW_WEAPON_OUTLINE_MIN + QUAD_VIEW_WEAPON_OUTLINE_RANGE * pulse).max(0.001) as f32,
    }
}

impl WeaponViewModel {
    /// `loadWeapon` for a definition id (`lasergun` is the voltaic).
    ///
    /// # Errors
    /// An unknown id, or a model missing from the packages or unreadable.
    pub fn load(gpu: &Gpu, pk3: &mut Pk3Loader, id: &str, color_format: wgpu::TextureFormat) -> Result<Self, String> {
        let weapon = weapon_model(id).ok_or_else(|| format!("no weapon {id}"))?;
        let package = package_path(weapon.model_path);
        let bytes = pk3
            .get_file(&package)
            .ok_or_else(|| format!("{package}: in no package"))?;
        let model = GltfModel::from_glb(&bytes).map_err(|e| format!("{package}: {e}"))?;
        let mut renderer = ModelRenderer::new(gpu, color_format);
        let name = weapon.id;
        let handle = renderer.upload(gpu, &model, |m| view_material_mode(name, m));
        // `setupAmmoCounter`: the cfg beside the model.
        let counter = model.attachment_points.get("tag_ammo").and_then(|tag| {
            let cfg = pk3.get_file(&package.replace(".glb", ".cfg"))?;
            let config = parse_ammo_counter(&String::from_utf8_lossy(&cfg))?;
            let font = Font::from_bytes(include_bytes!("../../assets/fonts/Orbitron-Black.ttf").to_vec()).ok()?;
            let display = AmmoCounterDisplay::new(gpu, &mut renderer, config, &font);
            Some((display, Mat4::from_rotation_translation(tag.rotation, tag.position)))
        });
        // `findWeaponTag(glbRoot, ["tag_flash", "tag_muzzle", "muzzle"])`;
        // only `tag_` nodes are attachment points.
        let muzzle = ["tag_flash", "tag_muzzle"]
            .iter()
            .find_map(|tag| model.attachment_points.get(*tag))
            .map(|tag| tag.position);
        Ok(Self {
            renderer,
            view: WeaponView::new(name, Some(view_model_def(weapon))),
            muzzle,
            handle,
            model,
            draws: Vec::new(),
            powerup_outline: None,
            counter,
            ammo: 0.0,
        })
    }

    /// Builds the gun's pipelines now rather than on its first frame.
    pub fn precompile(&mut self, gpu: &Gpu) -> usize {
        self.renderer.precompile(gpu)
    }

    /// Places the weapon under `camera` and writes the weapon camera: the
    /// main camera's view with the weapon FOV, near 0.1 and far 1000, lit
    /// by the weapon light (and the blade's).
    pub fn prepare(&mut self, gpu: &Gpu, camera: &Camera, aspect: f32) {
        let view = camera.view();
        let camera_world = view.inverse();
        let root = self.view.model_matrix(camera_world);
        self.draws.clear();
        let name = self.view.weapon.as_str();
        for (i, primitive) in self.model.primitives.iter().enumerate() {
            let mode = primitive
                .material
                .and_then(|m| self.model.materials.get(m))
                .map_or(ModelMaterialMode::Lit, |m| view_material_mode(name, m));
            // `applyWeaponOutlines`: the powerup's colour and width when
            // one is on, divided by the root's scale like the black one.
            let outline = weapon_outline(&mode, self.view.scale).map(|black| match self.powerup_outline {
                Some(powerup) => Outline {
                    color: powerup.color,
                    width: black.width / WEAPON_OUTLINE_WIDTH * powerup.width,
                },
                None => black,
            });
            self.draws.push(ModelDraw {
                outline,
                ..ModelDraw::new(self.handle, i, root * primitive.node_matrix)
            });
        }
        if let Some((counter, tag)) = &mut self.counter {
            // `displayedAmmo | 0`.
            counter.set_ammo(gpu, &mut self.renderer, self.ammo as i64);
            let tuning = with_cvars(|c| AmmoCounterTuning::read(c));
            counter.draws(root * *tag, &tuning, &mut self.draws);
        }
        let mut lights = vec![weapon_light()];
        if self.view.weapon == "blade" {
            let (_, rotation, _) = camera_world.to_scale_rotation_translation();
            lights.push(blade_light(rotation));
        }
        self.renderer.set_lights(lights);
        let weapon_camera = Camera {
            fov_y: self.view.fov_radians(),
            near: WEAPON_CAMERA_MIN_Z,
            far: WEAPON_CAMERA_MAX_Z,
            ..*camera
        };
        let view_proj = weapon_camera.projection(aspect) * view;
        self.renderer
            .prepare(gpu, view_proj, camera.position, &self.draws, &[] as &[Mat4]);
    }

    /// `getMuzzleWorldPosition(distance)` under `camera`: on the screen
    /// ray through the muzzle tag, `distance` units out.
    #[must_use]
    pub fn muzzle_world_position(&self, camera: &Camera, distance: f64) -> DVec3 {
        self.view
            .muzzle_world_position(camera.view().inverse(), camera.fov_y, self.muzzle, distance)
    }

    /// Records the weapon into a pass whose depth was cleared.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.renderer.draw_opaque(pass);
        self.renderer.draw_blended(pass);
    }
}

#[cfg(test)]
mod shot_tests {
    use super::*;

    #[test]
    fn each_gun_has_its_flash_but_the_beam() {
        let at = DVec3::new(1.0, 2.0, 3.0);
        let mg = muzzle_flash("machinegun", at).unwrap();
        assert_eq!(mg.position, at);
        assert_eq!(mg.duration, Some(0.075));
        assert_eq!(mg.style, MuzzleFlashStyle::Burst);
        let rl = muzzle_flash("rlauncher", at).unwrap();
        assert_eq!(
            (rl.style, rl.scale, rl.intensity),
            (MuzzleFlashStyle::Plume, Some(2.4), Some(30.0))
        );
        assert!(muzzle_flash("voltaic", at).is_none());
    }

    #[test]
    fn refire_follows_the_tick_rate() {
        // 63 ticks at 125 Hz between swings.
        let blade = shot_style("blade");
        assert!(blade.melee);
        assert_eq!(blade.refire_seconds, Some(63.0 / 125.0));
        assert_eq!(shot_style("machinegun").refire_seconds, Some(13.0 / 125.0));
        assert_eq!(shot_style("voltaic").refire_seconds, Some(6.0 / 125.0));
        assert_eq!(clip_reload_time("rlauncher"), None);
    }
}
