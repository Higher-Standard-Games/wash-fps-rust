//! The local player's first-person weapon, as wash-fps hands a weapon
//! definition to SHAG's `WeaponViewManager.loadWeapon` (GLB view models):
//! the model from the packages, uploaded with the view's material
//! fix-ups, placed by [`WeaponView`] under the camera and drawn by its own
//! [`ModelRenderer`] with the weapon camera's projection, lights and
//! outlines.

use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::rendering::camera::Camera;
use shagger::rendering::glam::{DVec3, Mat4};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer, Outline};
use shagger::rendering::wgpu;
use shagger::weapon::weapon_view_manager::{
    ViewModelDef, WEAPON_CAMERA_MAX_Z, WEAPON_CAMERA_MIN_Z, WEAPON_OUTLINE_WIDTH, WeaponView, blade_light,
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
        Ok(Self {
            renderer,
            view: WeaponView::new(name, Some(view_model_def(weapon))),
            handle,
            model,
            draws: Vec::new(),
            powerup_outline: None,
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

    /// Records the weapon into a pass whose depth was cleared.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.renderer.draw_opaque(pass);
        self.renderer.draw_blended(pass);
    }
}
