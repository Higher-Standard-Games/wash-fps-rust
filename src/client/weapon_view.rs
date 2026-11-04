//! The local player's first-person weapon, as wash-fps hands a weapon
//! definition to SHAG's `WeaponViewManager.loadWeapon` (GLB view models):
//! the model from the packages, uploaded with the view's material
//! fix-ups, placed by [`WeaponView`] under the camera and drawn by its own
//! [`ModelRenderer`] with the weapon camera's projection, lights and
//! outlines.

use shagger::core::cvar::with_cvars;
use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::model::tags_loader::{find_tag, parse_tags};
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::rendering::camera::Camera;
use shagger::rendering::dynamic_lights::DynamicLight;
use shagger::rendering::glam::{DVec3, Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer, Outline};
use shagger::rendering::wgpu;
use shagger::shared::js::js_round;
use shagger::ui::font::Font;
use shagger::weapon::ammo_counter_display::{AmmoCounterDisplay, AmmoCounterTuning, parse_ammo_counter};
use shagger::weapon::muzzle::muzzle_flash_renderer::{MuzzleFlashSpawn, MuzzleFlashStyle};
use shagger::weapon::weapon_barrels::{BarrelSpin, babylon_euler, barrel_material_mode};
use shagger::weapon::weapon_view_manager::{
    FlashLight, ShotStyle, ViewModelDef, WEAPON_CAMERA_MAX_Z, WEAPON_CAMERA_MIN_Z, WEAPON_OUTLINE_WIDTH, WeaponView,
    blade_light, view_material_mode, weapon_light, weapon_outline,
};

use crate::client::quad_damage_visual::{
    QUAD_DAMAGE_COLOR, QUAD_VIEW_WEAPON_OUTLINE_MIN, QUAD_VIEW_WEAPON_OUTLINE_RANGE, get_quad_damage_pulse,
};
use crate::shared::entities::definitions::weapons::weapons_table::{WeaponModel, weapon_model};
use crate::world::fps_pickup::{directory, package_path};

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
        melee: weapon.melee,
        charge_time: weapon.charge_time,
    }
}

/// `presentFire`'s reading of a weapon's definition (wash-fps
/// `shared/entities/definitions/weapons/*.ts`: `fireRate`, `recoilScale`,
/// `adsRecoilScale`, `meleeRadius`, `muzzleFlash`).
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
        flash: muzzle_flash_def(weapon).map(|f| FlashLight {
            color: f.color,
            intensity: Some(f.intensity),
            range: Some(f.range),
        }),
    }
}

/// A weapon definition's `muzzleFlash` (wash-fps
/// `shared/entities/definitions/weapons/*.ts`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MuzzleFlashDef {
    pub color: Vec3,
    pub intensity: f64,
    pub range: f64,
    pub duration: f64,
    pub style: MuzzleFlashStyle,
    pub scale: Option<f64>,
}

/// `WEAPONS[weapon].muzzleFlash`.
#[must_use]
pub fn muzzle_flash_def(weapon: &str) -> Option<MuzzleFlashDef> {
    use MuzzleFlashStyle::{Arcs, Burst, Plume};
    let (color, intensity, range, duration, style, scale) = match weapon {
        "blade" => ([0.05, 1.0, 0.9], 25.0, 40.0, 0.16, Burst, None),
        "electrobolt" => ([0.3, 0.5, 1.0], 35.0, 60.0, 0.12, Arcs, Some(2.3)),
        "glauncher" => ([0.6, 1.0, 0.3], 22.0, 40.0, 0.15, Plume, Some(2.2)),
        "machinegun" => ([1.0, 0.9, 0.3], 18.0, 30.0, 0.075, Burst, None),
        "plasmagun" => ([0.3, 1.0, 0.3], 20.0, 40.0, 0.09, Burst, Some(1.4)),
        "riotgun" => ([1.0, 0.7, 0.2], 25.0, 45.0, 0.14, Plume, Some(1.4)),
        "rlauncher" => ([1.0, 0.5, 0.1], 30.0, 50.0, 0.16, Plume, Some(2.4)),
        "voltaic" | "lasergun" => ([1.0, 0.7, 0.1], 12.0, 25.0, 0.03, Burst, None),
        _ => return None,
    };
    Some(MuzzleFlashDef {
        color: Vec3::from_array(color),
        intensity,
        range,
        duration,
        style,
        scale,
    })
}

/// `presentFire`'s procedural flash for `weapon`, spawned at `position`.
/// `None` for the Voltaic: a beam weapon, whose renderer draws its own
/// muzzle corona.
#[must_use]
pub fn muzzle_flash(weapon: &str, position: DVec3) -> Option<MuzzleFlashSpawn> {
    if matches!(weapon, "voltaic" | "lasergun") {
        return None;
    }
    let def = muzzle_flash_def(weapon)?;
    Some(MuzzleFlashSpawn {
        position,
        color: def.color,
        style: def.style,
        scale: def.scale,
        intensity: Some(def.intensity),
        duration: Some(def.duration),
        anchor: None,
    })
}

/// The blade tip's flash: a smaller copy of the blade's own, riding
/// `anchor` (`presentFire`, a swing rather than a throw).
#[must_use]
pub fn blade_tip_flash(position: DVec3, anchor: u32) -> Option<MuzzleFlashSpawn> {
    let mut flash = muzzle_flash("blade", position)?;
    flash.scale = Some(flash.scale.unwrap_or(1.0) * 0.4);
    flash.anchor = Some(anchor);
    Some(flash)
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
    /// The barrel sub-models (`WeaponBarrels.load`), their motion in
    /// [`WeaponView::barrels`].
    barrels: Vec<ViewBarrel>,
    /// `tag_blade_tip`: the blade's farthest point along +X, in the glTF
    /// scene's space.
    blade_tip: Option<Vec3>,
    /// `r_dynamiclight`: the muzzle flash lights the gun.
    pub dynamic_lights: bool,
}

/// `loadGlbSource`'s `tag_blade_tip`: the vertex farthest along +X of the
/// normalized model, in glTF scene space.
fn blade_tip(model: &GltfModel) -> Option<Vec3> {
    model
        .primitives
        .iter()
        .flat_map(|p| {
            p.positions
                .iter()
                .map(|v| p.node_matrix.transform_point3(Vec3::from_array(*v)))
        })
        .reduce(|tip, point| if point.x > tip.x { point } else { tip })
}

/// A barrel sub-model on the view model: where `WeaponBarrels.load` put
/// its root, in the weapon's glTF scene space.
struct ViewBarrel {
    handle: ModelHandle,
    model: GltfModel,
    /// The socket it hangs on: the weapon's `tag_barrel` node, or the
    /// weapon's root when `tags.txt` places it.
    parent: Mat4,
    /// The root's position and Euler rotation before it turns.
    rest: Vec3,
    rotation: Vec3,
}

/// `WeaponBarrels.load`'s barrel material set-up (`loadSource`): energy
/// and glass layers as `prepareBarrelMaterials` draws them, the rest lit
/// from both sides.
fn view_barrel_material_mode(material: &shagger::loader::model::gltf_model::ModelMaterial) -> ModelMaterialMode {
    match barrel_material_mode(material) {
        ModelMaterialMode::Lit => ModelMaterialMode::LitTwoSided,
        mode => mode,
    }
}

/// `WeaponBarrels.load`: each barrel of `weapon`, its model from the
/// packages, placed on the weapon `model`. A `tag_barrel` rule in the
/// barrels' `tags.txt` places them from the weapon's root (a bone's
/// position plus the rule's offset and rotation); otherwise they hang on
/// the weapon's own `tag_barrel` node. Each adds its offset.
fn load_view_barrels(
    gpu: &Gpu,
    pk3: &mut Pk3Loader,
    renderer: &mut ModelRenderer,
    weapon: &WeaponModel,
    model: &GltfModel,
) -> (Vec<ViewBarrel>, Vec<BarrelSpin>) {
    let mut barrels = Vec::new();
    let mut spins = Vec::new();
    let Some(first) = weapon.barrels.first() else {
        return (barrels, spins);
    };
    let tags = pk3
        .get_file(&format!("{}tags.txt", directory(&package_path(first.model_path))))
        .map(|bytes| parse_tags(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default();
    let barrel_tag = find_tag(&tags, "tag_barrel");
    let skeleton = &model.skeleton;
    let globals = skeleton.global_matrices(&skeleton.rest());
    // `findViewBarrelSocket`: a tags.txt rule wins over the node.
    let socket = if barrel_tag.is_none() {
        skeleton.find("tag_barrel").and_then(|i| globals.get(i).copied())
    } else {
        None
    };
    // The rule's bone, when the weapon has it.
    let bone = barrel_tag
        .filter(|t| socket.is_none() && !t.bone_name.is_empty())
        .and_then(|t| skeleton.find(&t.bone_name))
        .and_then(|i| skeleton.nodes.get(i))
        .map_or(Vec3::ZERO, |n| n.rest.translation);
    for def in weapon.barrels {
        let package = package_path(def.model_path);
        let Some(barrel_model) = pk3.get_file(&package).and_then(|b| GltfModel::from_glb(&b).ok()) else {
            eprintln!("{package}: no barrel model");
            continue;
        };
        let handle = renderer.upload(gpu, &barrel_model, view_barrel_material_mode);
        let tag_offset = barrel_tag.map_or(Vec3::ZERO, |t| t.offset);
        let rest = bone + tag_offset + def.offset.map_or(Vec3::ZERO, Vec3::from);
        let rotation = match barrel_tag {
            Some(t) if socket.is_none() => t.rotation,
            _ => Vec3::ZERO,
        };
        barrels.push(ViewBarrel {
            handle,
            model: barrel_model,
            parent: socket.unwrap_or(Mat4::IDENTITY),
            rest,
            rotation,
        });
        spins.push(BarrelSpin::new(&def.motion));
    }
    (barrels, spins)
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
        let (barrels, spins) = load_view_barrels(gpu, pk3, &mut renderer, weapon, &model);
        let mut view = WeaponView::new(name, Some(view_model_def(weapon)));
        view.barrels = spins;
        Ok(Self {
            renderer,
            view,
            barrels,
            blade_tip: if name == "blade" { blade_tip(&model) } else { None },
            dynamic_lights: true,
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
        // `applyVisibility`: stowed for a climb, the weapon is not drawn.
        let drawn = self.view.is_drawn();
        for (i, primitive) in self.model.primitives.iter().enumerate().filter(|_| drawn) {
            let mode = primitive
                .material
                .and_then(|m| self.model.materials.get(m))
                .map_or(ModelMaterialMode::Lit, |m| view_material_mode(name, m));
            // `applyWeaponOutlines`: the powerup's colour and width when
            // one is on, else the blade's charge (`WeaponCharge`), divided
            // by the root's scale like the black one. The quad wins.
            let over = self.powerup_outline.or(self.view.charge_outline);
            let outline = weapon_outline(&mode, self.view.scale).map(|black| match over {
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
        // The barrels, turned and slid as `WeaponBarrels.update` leaves
        // them, under the loader's handedness flip; no outline.
        for (barrel, spin) in self.barrels.iter().zip(&self.view.barrels).filter(|_| drawn) {
            let (position, rotation) = spin.pose(barrel.rest, barrel.rotation);
            let local = barrel.parent
                * Mat4::from_scale_rotation_translation(Vec3::new(1.0, 1.0, -1.0), babylon_euler(rotation), position);
            for (i, primitive) in barrel.model.primitives.iter().enumerate() {
                self.draws
                    .push(ModelDraw::new(barrel.handle, i, root * local * primitive.node_matrix));
            }
        }
        if let Some((counter, tag)) = self.counter.as_mut().filter(|_| drawn) {
            // `displayedAmmo | 0`.
            counter.set_ammo(gpu, &mut self.renderer, self.ammo as i64);
            let tuning = with_cvars(|c| AmmoCounterTuning::read(c));
            counter.draws(root * *tag, &tuning, &mut self.draws);
        }
        let mut lights = vec![weapon_light()];
        if self.view.weapon == "blade" {
            let (_, rotation, _) = camera_world.to_scale_rotation_translation();
            // `WeaponLighting`: the map's light at the camera tints it.
            let [r, g, b] = self.view.lighting.color;
            lights.push(blade_light(rotation, Vec3::new(r as f32, g as f32, b as f32)));
        }
        self.renderer.set_lights(lights);
        // `muzzleFlashLight`: weapon layer only, at the live muzzle socket.
        let flash = self
            .muzzle
            .zip(self.view.muzzle_flash_light())
            .filter(|_| self.dynamic_lights)
            .map(|(socket, light)| {
                let intensity = light.intensity as f32;
                DynamicLight {
                    position: (root * socket.extend(1.0)).truncate(),
                    diffuse: light.diffuse * intensity,
                    specular: light.specular * intensity,
                    range: light.range as f32,
                }
            });
        self.renderer.set_dynamic_lights(flash.as_slice());
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

    /// `getBladeTipWorldPosition(jitterX, jitterY)` under `camera`: 5
    /// units out along the screen ray through the blade's tip, moved
    /// `jitter` pixels on a screen `viewport_height` pixels tall. `None`
    /// for anything but the blade.
    #[must_use]
    pub fn blade_tip_world_position(&self, camera: &Camera, jitter: (f64, f64), viewport_height: f64) -> Option<DVec3> {
        self.view.socket_world_position(
            camera.view().inverse(),
            camera.fov_y,
            self.blade_tip?,
            5.0,
            jitter,
            viewport_height,
        )
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
    fn the_voltaic_lights_the_gun_without_a_flash_and_the_blade_tip_is_smaller() {
        let at = DVec3::ZERO;
        let voltaic = shot_style("voltaic").flash.unwrap();
        assert_eq!(voltaic.intensity, Some(12.0));
        assert_eq!(voltaic.range, Some(25.0));
        let rl = shot_style("rlauncher").flash.unwrap();
        assert_eq!((rl.intensity, rl.range), (Some(30.0), Some(50.0)));
        let tip = blade_tip_flash(at, 3).unwrap();
        assert_eq!(tip.anchor, Some(3));
        assert!((tip.scale.unwrap() - 0.4).abs() < 1e-12);
        assert_eq!(muzzle_flash("blade", at).unwrap().anchor, None);
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
