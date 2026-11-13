//! wash-fps `client/settings/PlayerTeamPreview.ts`: each side's player
//! model on the Gameplay page's team cards, in the side's chosen body,
//! colour and outline, standing idle and swaying a little.
//!
//! The browser draws both sides from one small engine, a camera viewport
//! per card; here each card has a shagger [`ModelStage`] of its own. The
//! bodies come from the packages ([`preview_packages`]) and are read once
//! for both cards; a body that fails to load leaves the card's placeholder
//! (the model's name in the side's outline).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use shagger::animation::player_model_assets::{PLAYER_ANIMATION_ROOT, PLAYER_ANIMATIONS};
use shagger::animation::player_model_manager::{PlayerModel, PlayerModelInstance, PlayerModelVisualStyle};
use shagger::rendering::glam::camera::lh::{proj::directx::orthographic, view::look_at_mat4};
use shagger::rendering::glam::{Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{
    ModelDraw, ModelHandle, ModelLight, ModelMaterialMode, ModelRenderer, Outline,
};
use shagger::rendering::model_stage::{ModelStage, StageScene, StageView};
use shagger::ui::egui_layer::egui;

use crate::client::player_model_selection::{PlayerModelSide, player_model_asset, player_model_choice};
use crate::client::player_model_visual_settings::{PlayerTeamSlot, get_player_model_visual_style};
use crate::client::preview_packages;

/// `createInstance(id, 36)`: the body's height in world units.
const MODEL_SCALE: f32 = 36.0;
/// `orthoTop`: half the camera's view height.
const ORTHO_HALF: f32 = 38.0;

/// The bodies read so far, by choice (`None`: it did not load), shared by
/// both cards.
type Bodies = HashMap<&'static str, Option<Arc<PlayerModel>>>;
static BODIES: Mutex<Option<Bodies>> = Mutex::new(None);

/// `loadModel` and `loadVariant`: a body with its shared clips.
fn body(choice: &'static str) -> Option<Arc<PlayerModel>> {
    let mut bodies = BODIES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    bodies
        .get_or_insert_with(HashMap::new)
        .entry(choice)
        .or_insert_with(|| {
            let (root, file) = player_model_asset(choice);
            let model = PlayerModel::load(
                preview_packages::read,
                root,
                file,
                PLAYER_ANIMATIONS,
                PLAYER_ANIMATION_ROOT,
            );
            match model {
                Ok(m) if m.has_animation("idle") => Some(Arc::new(m)),
                Ok(_) => {
                    eprintln!("[PlayerTeamPreview] {choice}: no idle clip");
                    None
                }
                Err(e) => {
                    eprintln!("[PlayerTeamPreview] {choice}: {e}");
                    None
                }
            }
        })
        .clone()
}

struct Shown {
    choice: &'static str,
    model: Arc<PlayerModel>,
    handle: ModelHandle,
    instance: PlayerModelInstance,
}

struct FigureScene {
    /// The body to show and how (set by the card each frame).
    choice: Option<&'static str>,
    style: Option<PlayerModelVisualStyle>,
    shown: Option<Shown>,
    /// This stage's uploads, by choice.
    uploads: HashMap<&'static str, ModelHandle>,
    /// A choice that did not load.
    failed: Option<&'static str>,
    last: Option<f64>,
}

impl StageScene for FigureScene {
    fn frame(&mut self, gpu: &Gpu, models: &mut ModelRenderer, size: (u32, u32), time: f64) -> Option<StageView> {
        let choice = self.choice?;
        let style = self.style?;
        if self.shown.as_ref().is_none_or(|s| s.choice != choice) {
            self.shown = None;
            if self.failed == Some(choice) {
                return None;
            }
            let Some(model) = body(choice) else {
                self.failed = Some(choice);
                return None;
            };
            let handle = *self
                .uploads
                .entry(choice)
                .or_insert_with(|| models.upload(gpu, &model.model, |_| ModelMaterialMode::Lit));
            let mut instance = model.create_instance();
            model.play_animation(&mut instance, "idle");
            self.shown = Some(Shown {
                choice,
                model,
                handle,
                instance,
            });
            self.last = None;
        }
        let shown = self.shown.as_mut()?;
        let dt = self.last.map_or(0.0, |last| (time - last).clamp(0.0, 0.1));
        self.last = Some(time);
        shown.model.advance(&mut shown.instance, dt);
        shown.model.reset_root_motion(&mut shown.instance);
        // `anchor.rotation.y = sin(now / 3500) * 0.22`.
        let sway = ((time * 1000.0 / 3500.0).sin() * 0.22) as f32;
        let world = Mat4::from_rotation_y(sway) * shown.model.root_matrix(MODEL_SCALE);
        // `applyVisualStyle`: the outline is in world units, the mesh scaled by its fit.
        let outline_width = style.outline_width / shown.model.scale_factor();
        let mut joints = Vec::new();
        let mut palettes: HashMap<usize, u32> = HashMap::new();
        let mut draws = Vec::new();
        for (index, primitive) in shown.model.model.primitives.iter().enumerate() {
            let Some(skin) = &primitive.skin else { continue };
            let offset = *palettes.entry(skin.skin).or_insert_with(|| {
                let at = joints.len() as u32;
                joints.extend(shown.model.joint_matrices(&shown.instance, skin.skin));
                at
            });
            draws.push(ModelDraw {
                tint: style.tint_color,
                skin: Some(offset),
                outline: style.outline_enabled.then_some(Outline {
                    color: style.outline_color,
                    width: outline_width,
                }),
                ..ModelDraw::new(shown.handle, index, world)
            });
        }
        // A `FreeCamera` at (55, 43, 140) aimed at (0, 32, 0), orthographic,
        // 76 units tall and as wide as the card's stage.
        let eye = Vec3::new(55.0, 43.0, 140.0);
        let half_width = ORTHO_HALF * size.0 as f32 / size.1.max(1) as f32;
        let view_proj = orthographic(-half_width, half_width, -ORTHO_HALF, ORTHO_HALF, 1.0, 10_000.0)
            * look_at_mat4(eye, Vec3::new(0.0, 32.0, 0.0), Vec3::Y);
        Some(StageView {
            view_proj,
            eye,
            lights: vec![ModelLight::Hemispheric {
                direction: Vec3::new(0.4, 1.0, 1.0),
                diffuse: Vec3::ONE,
                specular: Vec3::ONE,
                ground: Vec3::ZERO,
                intensity: 1.4,
            }],
            draws,
            joints,
        })
    }
}

/// One card's figure. Made when the Gameplay page shows; dropped with it.
pub struct PlayerTeamPreview {
    side: PlayerModelSide,
    stage: ModelStage<FigureScene>,
}

impl std::fmt::Debug for PlayerTeamPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerTeamPreview").field("side", &self.side).finish()
    }
}

impl PlayerTeamPreview {
    #[must_use]
    pub fn new(side: PlayerModelSide) -> Self {
        Self {
            side,
            stage: ModelStage::new(FigureScene {
                choice: None,
                style: None,
                shown: None,
                uploads: HashMap::new(),
                failed: None,
                last: None,
            }),
        }
    }

    /// `is-live`: the figure is up, so the card's placeholder goes.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.stage.scene().shown.is_some()
    }

    /// `refresh` and the frame: the side's body and style as the settings
    /// stand now, drawn in `rect`.
    pub fn ui(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        let slot = match self.side {
            PlayerModelSide::Ally => PlayerTeamSlot::Ally,
            PlayerModelSide::Enemy => PlayerTeamSlot::Enemy,
        };
        {
            let mut scene = self.stage.scene();
            scene.choice = Some(player_model_choice(self.side));
            scene.style = Some(get_player_model_visual_style(slot, MODEL_SCALE));
        }
        self.stage.ui(ui, rect);
    }
}
