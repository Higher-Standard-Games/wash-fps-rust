//! wash-fps `client/hud/widgets/WeaponPreview.ts`: one weapon, lit and
//! turning, inside a UI panel (the loadout picker's stand).
//!
//! The browser runs it on its own engine and canvas rather than the game
//! scene, so it looks the same every round whatever the map's light; here
//! it is a shagger [`ModelStage`], which keeps its own renderer the same
//! way. The model comes from the packages ([`preview_packages`]); a weapon
//! with no model leaves the stand empty, which costs the player nothing
//! more than a missing picture.

use std::collections::HashMap;

use shagger::loader::model::gltf_model::GltfModel;
use shagger::rendering::glam::{Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelLight, ModelMaterialMode, ModelRenderer};
use shagger::rendering::model_stage::{ModelStage, StageScene, StageView, arc_rotate_eye};
use shagger::ui::egui_layer::egui;

use crate::client::preview_packages;
use crate::shared::entities::definitions::weapons::weapons_table::weapon_model;
use crate::world::fps_pickup::package_path;

/// Radians per second the weapon turns on its stand.
const SPIN_RATE: f64 = 0.6;
/// Framing padding: how much of the panel the model fills.
const FRAME_MARGIN: f32 = 1.45;

/// A loaded weapon: its upload, where each primitive sits under the glTF
/// root, and the camera's framing (`frame`).
struct Loaded {
    handle: ModelHandle,
    nodes: Vec<Mat4>,
    centre: Vec3,
    radius: f32,
}

#[derive(Default)]
struct WeaponScene {
    /// The model path to show.
    wanted: Option<&'static str>,
    /// What each path loaded to (`None`: it has no model).
    loaded: HashMap<&'static str, Option<Loaded>>,
    /// The path on the stand, its turn so far (radians) and the stage time
    /// of the last frame drawn.
    shown: Option<(&'static str, f64, f64)>,
}

/// The glTF loader's `__root__` once `show` has reset its position and
/// rotation: only the handedness flip remains.
fn gltf_root() -> Mat4 {
    Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0))
}

fn load(gpu: &Gpu, models: &mut ModelRenderer, path: &str) -> Option<Loaded> {
    let bytes = preview_packages::read(&package_path(path))?;
    let model = GltfModel::from_glb(&bytes).ok()?;
    let handle = models.upload(gpu, &model, |_| ModelMaterialMode::Lit);
    let nodes: Vec<Mat4> = model.primitives.iter().map(|p| p.node_matrix).collect();
    // `getHierarchyBoundingVectors`, in the pose `frame` measures.
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (p, node) in model.primitives.iter().zip(&nodes) {
        let world = gltf_root() * *node;
        for v in &p.positions {
            let at = world.transform_point3(Vec3::from(*v));
            min = min.min(at);
            max = max.max(at);
        }
    }
    if min.x > max.x {
        return None;
    }
    let size = max - min;
    Some(Loaded {
        handle,
        nodes,
        centre: min + size * 0.5,
        radius: (size.max_element() * FRAME_MARGIN).max(0.01),
    })
}

impl StageScene for WeaponScene {
    fn frame(&mut self, gpu: &Gpu, models: &mut ModelRenderer, size: (u32, u32), time: f64) -> Option<StageView> {
        let path = self.wanted?;
        let loaded = self
            .loaded
            .entry(path)
            .or_insert_with(|| load(gpu, models, path))
            .as_ref()?;
        // `rotation.y += SPIN_RATE * deltaTime`, from the frame the model is
        // up; a long frame (the model's load) moves it at most a tenth of a
        // second's worth.
        let angle = match &mut self.shown {
            Some((p, angle, last)) if *p == path => {
                *angle += (time - *last).clamp(0.0, 0.1) * SPIN_RATE;
                *last = time;
                *angle
            }
            _ => {
                self.shown = Some((path, 0.0, time));
                0.0
            }
        };
        let turn = Mat4::from_rotation_y(angle as f32);
        let draws = loaded
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| ModelDraw::new(loaded.handle, i, turn * gltf_root() * *node))
            .collect();
        // `ArcRotateCamera(-PI / 2, PI / 2.35, radius, centre)`, Babylon's
        // default field of view and clip planes.
        let eye = arc_rotate_eye(
            -std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI / 2.35,
            loaded.radius,
            loaded.centre,
        );
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        let view_proj = shagger::rendering::glam::camera::lh::proj::directx::perspective(0.8, aspect, 1.0, 10_000.0)
            * shagger::rendering::glam::camera::lh::view::look_at_mat4(eye, loaded.centre, Vec3::Y);
        Some(StageView {
            view_proj,
            eye,
            lights: vec![ModelLight::Hemispheric {
                direction: Vec3::new(0.4, 1.0, -0.6),
                diffuse: Vec3::ONE,
                specular: Vec3::ONE,
                ground: Vec3::ZERO,
                intensity: 1.15,
            }],
            draws,
            joints: Vec::new(),
        })
    }
}

/// The turning weapon. Made when the picker opens; dropped when it closes.
pub struct WeaponPreview {
    stage: ModelStage<WeaponScene>,
}

impl std::fmt::Debug for WeaponPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WeaponPreview")
    }
}

impl Default for WeaponPreview {
    fn default() -> Self {
        Self {
            stage: ModelStage::new(WeaponScene::default()),
        }
    }
}

impl WeaponPreview {
    /// `show`: the weapon `id` (its model) on the stand.
    pub fn show(&mut self, id: &str) {
        self.stage.scene().wanted = weapon_model(id).map(|m| m.model_path);
    }

    /// Whether the shown weapon has a model to draw (once looked for).
    #[must_use]
    pub fn has_model(&self) -> bool {
        let scene = self.stage.scene();
        scene
            .wanted
            .is_none_or(|path| scene.loaded.get(path).is_none_or(Option::is_some))
    }

    /// Draws the last frame in `rect` and asks for the next.
    pub fn ui(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        self.stage.ui(ui, rect);
    }
}
