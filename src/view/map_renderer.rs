//! The map's renderer: what a frame of a map draws, and how its effects,
//! players and pickups get there (wash-fps `client/GameEvents.ts`, the
//! scene side of `Game.ts`). Split out of `view.rs`; the same module tree.

use super::*;

/// `MAX_REMOTE_CHARGE_GLOW_LIGHTS` (wash-fps `GameEvents.ts`).
const MAX_REMOTE_CHARGE_GLOW_LIGHTS: usize = 6;

/// The map's sky, world and pickup models, and which world batches the
/// camera can see.
pub struct MapRenderer {
    pub world: WorldRenderer,
    pub models: ModelRenderer,
    /// Draws that stay put: weapon pickups and players.
    pub model_draws: Vec<ModelDraw>,
    /// Health, armour, ammo and the quad, posed each frame.
    pub items: ProceduralPickups,
    /// The armors and megahealth from their packaged models, bobbing.
    pub item_models: ItemModels,
    /// Each weapon pickup's id and its draws, at the head of `model_draws`.
    pub(super) weapon_pickup_spans: Vec<(u32, std::ops::Range<usize>)>,
    /// The server's word on which pickups are up, by id
    /// (`reconcileActiveSet`); a pickup it has not named is drawn.
    pub pickup_active: Vec<bool>,
    /// Each map weapon's id, weapon and resting spot, for the swap prompt.
    pub(super) weapon_pickup_spots: Vec<(u32, &'static str, DVec3)>,
    /// Seconds since the items were built (`visualElapsed`).
    pub item_time: f64,
    /// Weapons on the floor (`DroppedWeaponRenderer`), drawn with
    /// `hand_weapons`' models.
    pub dropped_weapons: DroppedWeaponRenderer,
    /// The map's PVS from the camera, for things that are not world batches:
    /// pickups and weapons on the floor (`BspManager.getPvsCuller`).
    pub(super) item_pvs: Option<PvsCuller>,
    /// This frame's draws.
    pub(super) frame_draws: Vec<ModelDraw>,
    /// Projectile models and where they are held.
    pub projectiles: Vec<(ProjectileModel, Mat4)>,
    /// Their trails, with the definitions' light colours.
    pub trails: Vec<(ProjectileTrail, Option<Vec3>)>,
    /// Held explosions, drawn at their ages (the clock stays at 0).
    pub explosions: ExplosionBatchRenderer,
    /// Bullet streaks, pellets, rails, Voltaic beams, muzzle flashes and
    /// impact sparks: held ones, and the shots of a demo or match.
    pub hitscan: HitscanEffects,
    /// What the shots of a demo or match stop on.
    pub(super) shot_world: Option<SweepCollisionBackend>,
    /// The joint palettes `model_draws` index, then the live players'.
    pub joints: Vec<Mat4>,
    /// How many of `joints` belong to `model_draws`.
    pub(super) static_joints: usize,
    /// The player model, when players are drawn.
    pub remote_players: Option<RemotePlayerRenderer>,
    /// This frame's live players and projectiles.
    pub(super) live_draws: Vec<ModelDraw>,
    /// Each live player's box and their run of `live_draws`, for the
    /// water's reflection, and this frame's reflected draws.
    pub(super) live_actors: Vec<(ReflectionActor, std::ops::Range<usize>)>,
    /// The same for the players posed at load, in `model_draws`.
    pub(super) static_actors: Vec<(ReflectionActor, std::ops::Range<usize>)>,
    pub(super) actor_draws: Vec<ModelDraw>,
    /// The frame's dynamic lights and reflection actors, kept for their
    /// storage.
    pub(super) frame_lights: Vec<DynamicLight>,
    pub(super) frame_actors: Vec<ReflectionActor>,
    /// Every projectile type's model, for projectiles that fly.
    pub(super) live_projectile_models: Vec<(&'static ProjectileDef, ProjectileModel)>,
    /// The flying projectiles' trails, by entity, and finished ones kept
    /// for the next projectile of their type.
    pub(super) live_trails: HashMap<String, (&'static ProjectileDef, ProjectileTrail)>,
    pub(super) spare_trails: Vec<(&'static str, ProjectileTrail)>,
    /// The flying projectiles' pool lights, by entity.
    pub(super) live_projectile_lights: HashMap<String, PooledLight>,
    /// When each projectile was last in a snapshot, on the lights' clock: a
    /// projectile seen before is mid-flight, not a new shot.
    pub(super) projectile_seen: HashMap<String, f64>,
    /// Our new shots' muzzle convergence (`renderOffset`): the offset from
    /// the true position to the barrel, and when it was fired.
    pub(super) projectile_offsets: HashMap<String, (DVec3, f64)>,
    /// The pool's clock for them.
    pub(super) live_light_clock: f64,
    /// The effects' clock (seconds) for a demo or match; held effects
    /// (`--explosion` and the like) stay at 0.
    pub effect_time: f64,
    /// Explosion flashes from events: their lights, looks and start times.
    pub(super) live_flashes: Vec<(PooledLight, &'static ExplosionLook, f64)>,
    /// The explosions set off since the window last took them, for the
    /// camera shake (`triggerCameraShake`).
    pub spawned_explosions: Vec<(DVec3, &'static ExplosionLook)>,
    /// Other players charging their blade, in the order they started
    /// (`chargingPlayers`), and this frame's glows on them.
    pub(super) charging: Vec<String>,
    pub(super) charge_glows: Vec<DynamicLight>,
    pub sky: Option<SkyRenderer>,
    /// The map's world fog and the `r_fog` cvars over it (MapLoader's
    /// `WorldFogController.setMap`).
    pub fog: WorldFogController,
    /// The cvars' serial the fog was last resolved at.
    pub(super) fog_serial: Option<u64>,
    /// The liquid surfaces, drawn with the unified water material
    /// (MapLoader's `claimLiquidSurfaces`); none with `r_water 0`.
    pub water: Option<LiquidSurfaceSystem>,
    /// `item_time` at the water's last frame, for its clock.
    pub(super) water_time: f64,
    /// Splashes, wakes and rings where bodies and shots cross a surface
    /// (`LocalLiquid`'s `LiquidInteractionSystem`).
    pub liquid: LiquidInteractionSystem,
    /// The liquid interactions' clock, in seconds.
    pub(super) liquid_clock: f64,
    /// The splash crowns' spray.
    pub(super) splash: SplashDropletRenderer,
    /// The rings' discs (lava, and pools the water system does not draw),
    /// and rings held by `washoi view --ring`.
    pub(super) rings: LiquidRingRenderer,
    pub held_rings: Vec<Ring>,
    /// Ghosts and speed-line bursts where players teleport.
    pub teleports: TeleportEffect,
    pub(super) teleport_renderer: TeleportRenderer,
    /// The map's `misc_particles` emitters (`MapParticles`).
    pub map_particles: MapParticles,
    /// The map's fog brushes (`FogVolumes`).
    pub(super) fog_volumes: FogVolumes,
    /// Each live player's last origin and yaw, for spotting a teleport.
    pub(super) teleport_last: HashMap<String, (DVec3, f64)>,
    /// Bodies breaking into shards on a gib death.
    pub gibs: GibSystem,
    pub(super) gib_renderer: GibRenderer,
    /// `HitSpatter`: blood or sparks where a hit lands.
    pub spatter: HitSpatter,
    pub(super) spatter_renderer: HitSpatterRenderer,
    /// The map's `func_explosive` brush models and which are up
    /// (`BrushModelState`), and the disabled list last applied.
    pub(super) brush_models: BrushModelState,
    /// The map's lifts, restored from the feed each frame so the buttons
    /// press in (`handleButton`).
    pub lifts: LiftSystem,
    pub(super) applied_disabled: Option<Vec<usize>>,
    /// The bomb sites' beacons and detonation (`BombSiteMarkers`).
    pub bomb_sites: BombSiteMarkers,
    /// Fragments thrown from a destroyed brush model (`BombSiteDebris`),
    /// and their models by path.
    pub(super) bomb_debris: BombSiteDebris,
    pub(super) debris_models: HashMap<String, ProjectileModel>,
    /// The bomb itself (`BombRenderer`), and its carrier's spine and yaw
    /// when they were drawn last frame.
    pub(super) bomb: BombRenderer,
    /// The trails behind sliders (`SlideDust`); ours is emitted by the
    /// window, everyone else's with their model.
    pub slide_dust: SlideDust,
    pub(super) slide_dust_renderer: SlideDustRenderer,
    pub(super) bomb_anchor: Option<(String, DVec3, f64)>,
    pub(super) bomb_carrier: Option<String>,
    /// Blast sounds the detonation asked for, not yet played.
    pub(super) blast_sounds: usize,
    /// Each live player's style: their shards take its colour, their
    /// corpse its colour and outline.
    pub(super) live_styles: HashMap<String, PlayerModelVisualStyle>,
    /// The local player's style (their own body is not drawn), which
    /// their corpse takes.
    pub(super) local_style: Option<PlayerModelVisualStyle>,
    /// Non-gib deaths fall as ragdolls (`RagdollSystem`); none without the
    /// player model.
    pub ragdolls: Option<RagdollSystem>,
    pub(super) ragdoll_trails: RagdollTrails,
    /// The blade's swing ribbons (`WeaponSlash.updateTrail`).
    pub(super) slash_trails: SlashTrailRenderer,
    /// Guns in players' hands (`updateWeaponModel`), and who holds which.
    pub(super) hand_weapons: HandWeapons,
    pub(super) live_weapons: Arc<HashMap<String, String>>,
    /// Each live player's body (`modelChoiceFor`), from [`styled_players`].
    pub live_models: HashMap<String, (&'static str, f64)>,
    /// Players whose body a ragdoll took, hidden until they leave the list.
    pub(super) ragdolled: HashSet<String>,
    /// Poses of players who left the list this frame, for a death that
    /// arrives with their leaving.
    pub(super) departed: HashMap<String, DetachedBody>,
    /// Each live player's body matrix, kept a frame past their leaving.
    pub(super) live_worlds: HashMap<String, Mat4>,
    /// The local player (whose own body is not drawn) and their view yaw,
    /// for their corpse (`startLocalRagdollDeathCamera`).
    pub(super) local_player: Option<(String, f64)>,
    /// The player watched through their eyes (`_hiddenEntityId`): their
    /// shots leave the gun on screen (`setFollowedMuzzle`).
    pub(super) followed_player: Option<String>,
    /// Where each drawn player's shots leave from this frame
    /// (`getWeaponMuzzleWorldPosition`).
    pub(super) remote_muzzles: HashMap<String, RemoteMuzzle>,
    /// `beamActive`: our Voltaic beam is drawn from the muzzle each frame.
    pub(super) local_beam: bool,
    /// The blade tips' flashes riding the tip, by anchor: their pixel jitter.
    pub(super) blade_tip_flashes: Vec<(u32, (f64, f64))>,
    pub(super) next_flash_anchor: u32,
    /// The last frame's height in pixels, for the blade tip's jitter.
    pub(super) frame_height: f64,
    /// What the death camera follows, and where and facing what the local
    /// player died; taken by the window to start it.
    pub(super) local_death: Option<(DeathFollow, DVec3, f64)>,
    /// Numbers floating over the players the one on screen hurts.
    pub damage_numbers: DamageNumbers,
    pub(super) damage_number_renderer: DamageNumberRenderer,
    /// POW! sprites over hard hits; no renderer without their images.
    pub cartoon_hits: CartoonHitEffect,
    pub(super) cartoon_hit_renderer: Option<CartoonHitRenderer>,
    /// Tint, fog and view warp while the camera is under a surface.
    pub underwater: UnderwaterView,
    /// The scene fog the underwater view holds this frame.
    pub(super) underwater_fog: Option<UnderwaterFog>,
    /// The fog last handed to the materials.
    pub(super) applied_fog: Option<FogUniforms>,
    pub culling: WorldCulling,
    /// Off draws every batch (`--no-cull`), for comparing.
    pub cull: bool,
    /// The effect lights (projectiles, explosion flashes).
    pub lights: PointLightPool,
    /// Off leaves them dark (`--no-lights`).
    pub dynamic_lights: bool,
    /// Image processing from the HDR target into the output.
    pub post: ImageProcessingPostProcess,
    pub image_processing: ImageProcessingConfiguration,
    /// The first-person weapon, drawn after the scene with depth cleared.
    pub view_model: Option<WeaponViewModel>,
    /// The post-game's winner on show; the view model is not drawn meanwhile.
    pub showcase: Option<PostGameShowcase>,
    /// The other weapons a player in the match may switch to, by id.
    pub holster: HashMap<String, WeaponViewModel>,
    /// `WeaponSwitchAnim`: the slide between weapons in hand.
    pub switch_anim: WeaponSwitchAnim,
    /// The switch's clock, seconds.
    pub(super) switch_clock: f64,
    /// The map's light grid and its sites (`BspManager.getLightingInto`),
    /// which tint the view weapon.
    pub(super) light_grid: Option<LightGrid>,
    pub(super) light_vols: Vec<BspLightVol>,
}

/// `BombSiteMarkers`' blast: the rocket's, `BLAST_SCALE` times as big, and
/// felt across the map but gently (`BLAST_SHAKE_RANGE`).
static BOMB_BLAST_LOOK: ExplosionLook = ExplosionLook {
    scale: ROCKET_EXPLOSION.scale * BLAST_SCALE,
    shake_intensity: 0.35,
    shake_max_range: BLAST_SHAKE_RANGE,
    ..ROCKET_EXPLOSION
};

/// SHAG's image processing: PostProcessingManager's default settings
/// (`colorGrading` and `vignette`), on DefaultRenderingPipeline's
/// `imageProcessing` with Babylon's other defaults.
#[must_use]
pub fn shag_image_processing() -> ImageProcessingConfiguration {
    ImageProcessingConfiguration {
        exposure: 1.0,
        contrast: 1.05,
        vignette: Some(Vignette {
            weight: 0.3,
            stretch: 0.5,
            color: [0.0; 3],
            blend_mode: VignetteBlendMode::Multiply,
            ..Vignette::default()
        }),
        color_matrix: None,
    }
}

/// A pickup the server has not named is there (the map's own state).
fn pickup_shown(active: &[bool], id: u32) -> bool {
    active.get(id as usize).copied().unwrap_or(true)
}

/// `PVSCuller.isPositionVisible` from the last frame's camera; with no vis
/// data, or culling off, everything is in view.
fn in_pvs(pvs: Option<&PvsCuller>, at: DVec3) -> bool {
    pvs.is_none_or(|p| p.is_position_visible(at.as_vec3()))
}

/// The weapons a player can hold (`WeaponsTable`'s ids).
pub(super) const HELD_WEAPONS: [&str; 8] = [
    "blade",
    "machinegun",
    "riotgun",
    "glauncher",
    "rlauncher",
    "plasmagun",
    "voltaic",
    "electrobolt",
];

impl MapRenderer {
    /// What a reel's cameras and the ground probe trace against.
    pub fn shot_world_mut(&mut self) -> Option<&mut SweepCollisionBackend> {
        self.shot_world.as_mut()
    }

    /// Puts `id` in hand from the holster (the view model's weapon switch),
    /// or nothing when dead. The weapon in hand keeps its sway when it
    /// stays.
    ///
    /// With a weapon in hand, a change slides it down, swaps, and slides the
    /// new one up (`switchWeaponAnimated`); empty-handed, it swaps at once.
    pub fn hold(&mut self, id: Option<&str>) {
        let id = id.map(|id| if id == "lasergun" { "voltaic" } else { id });
        let current = self.view_model.as_ref().map(|v| v.view.weapon.clone());
        let heading = self.switch_anim.pending().map(str::to_owned).or(current.clone());
        if heading.as_deref() == id {
            return;
        }
        match id {
            Some(id) if current.is_some() && self.holster.contains_key(id) => {
                if current.as_deref() == Some(id) {
                    // Back to the weapon still in hand: a cut.
                    self.switch_anim.reset();
                    self.hold_now(Some(id));
                } else {
                    self.switch_anim.start(id, self.switch_clock * 1000.0);
                }
            }
            _ => {
                self.switch_anim.reset();
                self.hold_now(id);
            }
        }
    }

    /// `presentFire`'s muzzle flash for the weapon in hand, at its muzzle
    /// under `camera`.
    /// A swing (not a charged throw) also flashes at the blade's tip,
    /// jittered a few pixels, riding it.
    pub fn fire_view_muzzle_flash(&mut self, camera: &Camera, projectile_fire: bool) {
        let Some(view_model) = &self.view_model else {
            return;
        };
        let position = view_model.muzzle_world_position(camera, 5.0);
        let Some(flash) = muzzle_flash(&view_model.view.weapon, position) else {
            return;
        };
        self.hitscan.fire_muzzle_flash(self.effect_time, &flash);
        if view_model.view.weapon == "blade" && !projectile_fire {
            // `(Math.random() - 0.5) * 8` pixels each way.
            let seed = self.effect_time.to_bits() as u32 ^ self.next_flash_anchor.wrapping_mul(0x9E37_79B9);
            let mut random = mulberry32(seed);
            let jitter = ((random() - 0.5) * 8.0, (random() - 0.5) * 8.0);
            if let Some(tip) = view_model.blade_tip_world_position(camera, jitter, self.frame_height) {
                let anchor = self.next_flash_anchor;
                self.next_flash_anchor = self.next_flash_anchor.wrapping_add(1);
                // Older tips are faded by now: a swing's flash lasts 0.16 s.
                if self.blade_tip_flashes.len() >= 8 {
                    self.blade_tip_flashes.remove(0);
                }
                self.blade_tip_flashes.push((anchor, jitter));
                if let Some(flash) = blade_tip_flash(tip, anchor) {
                    self.hitscan.fire_muzzle_flash(self.effect_time, &flash);
                }
            }
        }
    }

    /// The map's light at `position` (`getLightingInto`), `None` outside
    /// its light grid.
    #[must_use]
    pub fn light_at(&self, position: Vec3) -> Option<LightSample> {
        let p = position.as_dvec3();
        // Quake axes: x, z, y up.
        sample_light_vols(self.light_grid.as_ref()?, &self.light_vols, p.x, p.z, p.y)
    }

    /// `setBeamActive`: our Voltaic beam shows (or lets go and fades).
    pub fn set_beam_active(&mut self, active: bool) {
        if active && !self.local_beam {
            self.hitscan.begin_lightning();
        }
        if !active && self.local_beam {
            self.hitscan.stop_lightning(self.effect_time);
        }
        self.local_beam = active;
    }

    /// `updateBeam`: while our beam is on, it leaves this frame's muzzle
    /// along this frame's view, traced from the eye.
    pub fn update_beam(&mut self, camera: &Camera) {
        if !self.local_beam {
            return;
        }
        let Some(view_model) = &self.view_model else {
            return;
        };
        if view_model.view.weapon != "voltaic" {
            return;
        }
        let origin = view_model.muzzle_world_position(camera, 5.0);
        let mut no_world = ();
        let world: &mut dyn ShotWorld = match &mut self.shot_world {
            Some(world) => world,
            None => &mut no_world,
        };
        self.hitscan.update_lightning_beam(
            self.effect_time,
            world,
            &BeamConfig {
                origin,
                trace_origin: Some(camera.position.as_dvec3()),
                direction: camera.forward().as_dvec3(),
                max_range: Some(850.0),
                // The Voltaic's muzzle flash colour.
                color: Some(Vec3::new(1.0, 0.7, 0.1)),
            },
        );
    }

    /// `loadWeapon` with `cancelSwitch`: `id` in hand at once, no slide
    /// (the first weapon of a player we start watching).
    pub fn hold_cut(&mut self, id: Option<&str>) {
        let id = id.map(|id| if id == "lasergun" { "voltaic" } else { id });
        self.switch_anim.reset();
        self.hold_now(id);
    }

    /// Puts `id` in hand now. The one view in the browser keeps its bob,
    /// aim, recoil, swing, holster and light across the swap, so the new
    /// weapon takes them from the old (`WeaponView::carry_from`).
    fn hold_now(&mut self, id: Option<&str>) {
        let current = self.view_model.as_ref().map(|v| v.view.weapon.clone());
        if current.as_deref() == id {
            return;
        }
        let old = self.view_model.take();
        self.view_model = id.and_then(|id| self.holster.remove(id));
        if let Some(mut old) = old {
            if let Some(new) = &mut self.view_model {
                new.view.carry_from(&old.view);
            }
            old.view.cancel_reload();
            old.view.charge.reset();
            old.view.switch_offsets = SwitchAnimOffsets::default();
            self.holster.insert(old.view.weapon.clone(), old);
        }
    }

    /// Steps the switch by `dt` seconds: the swap when the slide down
    /// ends, and the slide's offsets on the weapon in hand.
    pub fn advance_switch(&mut self, dt: f64) {
        self.switch_clock += dt;
        let now = self.switch_clock * 1000.0;
        if let Some(id) = self.switch_anim.update(now) {
            self.hold_now(Some(&id));
            self.switch_anim.loaded(&id, now);
        }
        let offsets = self.switch_anim.offsets(now);
        if let Some(view_model) = &mut self.view_model {
            view_model.view.switch_offsets = offsets;
        }
    }

    /// Writes this frame's camera and culls for it.
    /// Poses and places the players a demo or a match shows this frame,
    /// advancing their clips by `dt`; players missing from `players` are
    /// forgotten.
    /// Throws away the sounds the effects asked for, when nothing can play
    /// them; otherwise they would pile up for the whole match.
    pub fn drop_sounds(&mut self) {
        drop(self.teleports.take_sounds());
        drop(self.liquid.take_sounds());
        self.blast_sounds = 0;
    }

    /// The blast sounds the detonation asked for since the last call.
    pub fn take_blast_sounds(&mut self) -> usize {
        std::mem::take(&mut self.blast_sounds)
    }

    /// `BombSiteMarkers.update` (with `BombSiteDebris.update`): the
    /// brushes the bomb destroyed leave the world and the shots' map, their
    /// fragments fly, and the server's blast schedule plays out. Call after
    /// [`Self::play_effects`], which runs the effects' clock.
    pub fn play_bomb_sites(&mut self, match_state: &MatchStateManager, dt: f64) {
        let disabled: Vec<usize> = match_state
            .bomb()
            .and_then(|b| b.get("disabledModels"))
            .and_then(shagger::shared::json::Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(shagger::shared::json::Value::as_f64)
                    .filter(|i| *i >= 0.0 && i.fract() == 0.0)
                    .map(|i| i as usize)
                    .collect()
            })
            .unwrap_or_default();
        // `BombRenderer.update`: the carrier as drawn last frame.
        let bomb = match_state.bomb();
        let anchor = &self.bomb_anchor;
        self.bomb.update(dt, bomb, |carrier| {
            anchor
                .as_ref()
                .filter(|(id, _, _)| id == carrier)
                .map(|&(_, at, yaw)| (at, yaw))
        });
        // The carrier's id, copied only when it changes hands.
        let carrier = bomb
            .and_then(|b| b.get("carrier"))
            .and_then(shagger::shared::json::Value::as_str);
        if self.bomb_carrier.as_deref() != carrier {
            self.bomb_carrier = carrier.map(str::to_owned);
        }
        if self.applied_disabled.as_ref() != Some(&disabled) {
            // `restore`: the round's reset, then the server's list.
            self.brush_models.restore(&disabled);
            let gone: Vec<usize> = self
                .brush_models
                .definitions
                .iter()
                .map(|d| d.model_index)
                .filter(|&m| !self.brush_models.is_enabled(m))
                .collect();
            self.world.set_disabled_brush_models(&gone);
            if let Some(models) = self.shot_world.as_mut().and_then(|w| w.brush_models.as_mut()) {
                models.restore(&disabled);
            }
            self.applied_disabled = Some(disabled.clone());
        }
        {
            use shagger::physics::collision_backend::TraceOptions;
            use shagger::physics::content_flags::content;
            let models = &self.debris_models;
            let available = |path: &str| models.contains_key(path);
            let mut trace = self.shot_world.as_mut().map(|w| {
                move |from: DVec3, to: DVec3| -> Option<(DVec3, Option<DVec3>)> {
                    let v = |p: DVec3| shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z);
                    let hit = w.trace(
                        v(from),
                        v(to),
                        TraceOptions {
                            radius: Some(0.0),
                            height: Some(0.0),
                            content_mask: Some(content::SOLID),
                        },
                    );
                    (hit.fraction < 1.0).then(|| {
                        (
                            DVec3::new(hit.end_pos.x, hit.end_pos.y, hit.end_pos.z),
                            hit.normal.map(|n| DVec3::new(n.x, n.y, n.z)),
                        )
                    })
                }
            });
            self.bomb_debris.update(
                dt,
                match_state.round(),
                &disabled,
                &self.brush_models.definitions,
                &available,
                trace
                    .as_mut()
                    .map(|t| t as &mut dyn FnMut(DVec3, DVec3) -> Option<(DVec3, Option<DVec3>)>),
            );
        }
        let now = self.effect_time;
        for blast in self.bomb_sites.update(dt, match_state) {
            if blast.sound {
                self.blast_sounds += 1;
            }
            let mut random = mulberry32(self.live_flashes.len() as u32 + blast.position.x.to_bits() as u32);
            self.explosions
                .spawn(now, blast.position, &BOMB_BLAST_LOOK, DVec3::Y, &mut random);
            self.spawned_explosions.push((blast.position, &BOMB_BLAST_LOOK));
            if let Some((intensity, range)) = explosion_flash(&BOMB_BLAST_LOOK, 0.0) {
                let light = self.lights.acquire(
                    self.live_light_clock,
                    blast.position.as_vec3(),
                    BOMB_BLAST_LOOK.outer_color,
                    intensity,
                    range,
                );
                self.live_flashes.push((light, &BOMB_BLAST_LOOK, now));
            }
        }
    }

    /// The bomb sites' beacons this frame, for the HUD.
    #[must_use]
    pub fn bomb_site_poses(&self) -> Vec<MarkerPose> {
        self.bomb_sites.poses()
    }

    /// `effect_dt` is the demo clock's step in a demo (the teleport effects run
    /// on it), the frame's otherwise.
    pub fn set_live_players(
        &mut self,
        players: &[(String, InterpolatedEntity, PlayerModelVisualStyle)],
        hidden: &[&str],
        dt: f64,
        effect_dt: f64,
    ) {
        // Everyone but the hidden (the player whose eyes we look through):
        // borrowed, not a copy of the list.
        let players: Vec<&(String, InterpolatedEntity, PlayerModelVisualStyle)> = players
            .iter()
            .filter(|(id, _, _)| !hidden.contains(&id.as_str()))
            .collect();
        let players = players.as_slice();
        self.update_charge_glows(players);
        self.joints.truncate(self.static_joints);
        self.live_draws.clear();
        self.bomb_anchor = None;
        self.live_actors.clear();
        // `RemotePlayerRenderer` emits everyone's dust; `Game` steps it.
        for (id, p, _) in players.iter().copied() {
            let sliding = p.sliding && p.grounded && p.water_level.unwrap_or(0) == 0 && !p.on_ladder.unwrap_or(false);
            self.slide_dust.emit(id, DVec3::new(p.x, p.y, p.z), p.vx, p.vz, sliding);
        }
        self.slide_dust.step(effect_dt);
        let Some(renderer) = &mut self.remote_players else {
            return;
        };
        // Who is in this frame's list, looked up once rather than searched
        // for each id kept below.
        let present: HashSet<&str> = players.iter().map(|(p, _, _)| p.as_str()).collect();
        // A ragdoll's player stays hidden until they leave the list.
        self.ragdolled.retain(|id| present.contains(id.as_str()));
        let ragdolled = &self.ragdolled;
        self.departed = renderer
            .retain(|id| present.contains(id) && !ragdolled.contains(id))
            .into_iter()
            .collect();
        let departed = &self.departed;
        self.live_worlds
            .retain(|id, _| departed.contains_key(id) || present.contains(id.as_str()));
        self.live_styles
            .retain(|id, _| departed.contains_key(id) || present.contains(id.as_str()));
        // Each frame sets every drawn player's muzzle; kept, not cleared,
        // so their keys are not made anew.
        let ragdolled = &self.ragdolled;
        self.remote_muzzles
            .retain(|id, _| present.contains(id.as_str()) && !ragdolled.contains(id));
        for (id, interp, style) in players.iter().copied() {
            if self.ragdolled.contains(id) {
                continue;
            }
            upsert(&mut self.live_styles, id, *style);
            // The body `styled_players` chose (`modelChoiceFor`).
            let (choice, scale) = self
                .live_models
                .get(id)
                .copied()
                .unwrap_or_else(|| (player_model_choice(PlayerModelSide::Enemy), 1.0));
            renderer.update_live(
                id,
                interp,
                dt,
                1.0,
                self.live_weapons.get(id).map(String::as_str),
                self.effect_time,
                choice,
                scale,
            );
            if let Some(world) = renderer.world_of(id, interp) {
                upsert(&mut self.live_worlds, id, world);
            }
            if self.bomb_carrier.as_deref() == Some(id.as_str()) {
                // `getBoneAnchor(carrier, "Spine1")`, else shoulder height
                // over the hitbox (`carrierFallback`).
                let (at, yaw) = renderer.bone_anchor(id, interp, "Spine1").unwrap_or((
                    DVec3::new(interp.x, interp.y + CARRY_FALLBACK_HEIGHT, interp.z),
                    interp.yaw,
                ));
                self.bomb_anchor = Some((id.clone(), at, yaw));
            }
            let first = self.live_draws.len();
            renderer.draws(id, interp, style, &mut self.joints, &mut self.live_draws);
            let scale_factor = renderer.body_of(id).map_or(1.0, |b| b.model.scale_factor());
            update_powerup_glow(
                &mut self.live_draws[first..],
                interp.powerup_mask.unwrap_or(0),
                self.item_time,
                scale_factor,
            );
            let mut muzzle = RemoteMuzzle {
                tag: None,
                root: None,
                body: DVec3::new(interp.x, interp.y, interp.z),
            };
            if let (Some(weapon), Some(hand)) = (self.live_weapons.get(id), renderer.hand_world(id, interp)) {
                let quad =
                    mask_has_quad(interp.powerup_mask.unwrap_or(0)).then(|| get_quad_damage_pulse(self.item_time));
                self.hand_weapons.draws(weapon, &hand, quad, &mut self.live_draws);
                if let Some((tag, root)) = self.hand_weapons.muzzle(weapon, &hand) {
                    muzzle.tag = tag.map(Vec3::as_dvec3);
                    muzzle.root = Some(root.as_dvec3());
                }
            }
            upsert(&mut self.remote_muzzles, id, muzzle);
            // The player's box (Q3's -15 -15 -24 to 15 15 32, Babylon order).
            let at = DVec3::new(interp.x, interp.y, interp.z);
            self.live_actors.push((
                ReflectionActor {
                    min: at + DVec3::new(-15.0, -24.0, -15.0),
                    max: at + DVec3::new(15.0, 32.0, 15.0),
                    meshes: self.live_draws.len() - first,
                },
                first..self.live_draws.len(),
            ));
        }
        // RemotePlayerRenderer's teleport detection: a jump of more than
        // 256 units between frames. A player gone from the list (dead) is
        // forgotten, so a respawn starts afresh.
        self.teleports.update(effect_dt);
        self.teleport_last.retain(|id, _| present.contains(id.as_str()));
        for (id, interp, _) in players.iter().copied() {
            let at = DVec3::new(interp.x, interp.y, interp.z);
            if let Some((last, last_yaw)) = upsert(&mut self.teleport_last, id, (at, interp.yaw))
                && at.distance_squared(last) > TELEPORT_FX_DIST_SQ
            {
                self.teleports
                    .player_teleported(last, at, last_yaw, interp.yaw, legacy_team_color(None, id));
            }
        }
        self.teleport_renderer.advance(Some(&renderer.model), dt);
        self.teleport_renderer.ghost_draws(
            Some(&renderer.model),
            &self.teleports,
            &mut self.joints,
            &mut self.live_draws,
        );
        // Splashes come from the water level already on the wire
        // (`RemotePlayerRenderer`): replicate causes, derive effects.
        self.liquid_clock += dt;
        let gone: Vec<String> = self
            .liquid
            .body_ids()
            .filter(|id| !present.contains(id))
            .map(str::to_owned)
            .collect();
        for id in gone {
            self.liquid.forget(&id);
        }
        for (id, interp, _) in players.iter().copied() {
            self.notify_liquid(
                id,
                interp.water_level.unwrap_or(0),
                DVec3::new(interp.x, interp.y, interp.z),
                interp.vy,
                false,
            );
        }
    }

    /// `startPostGameShowcase`: the winner, `entity_id`, on show in their
    /// body (`choice`) and `style`, holding `weapon`, in front of `camera`.
    pub fn start_showcase(
        &mut self,
        choice: &str,
        style: PlayerModelVisualStyle,
        weapon: Option<String>,
        camera: &Camera,
        aspect: f64,
    ) {
        self.showcase = None;
        let Some(renderer) = &self.remote_players else {
            return;
        };
        let body = renderer.body_by_choice(choice).clone();
        // `Math.random()` for the dance's shuffle.
        let pick = shagger::shared::math_random::MathRandom::from_entropy().next_f64();
        let start = ShowcaseStart {
            body,
            style,
            weapon,
            camera,
            aspect,
            pick,
        };
        self.showcase = Some(PostGameShowcase::start(start, |from, to| self.trace_map(from, to)));
    }

    /// Moves the showcase on by `dt` and adds its draws to the frame's,
    /// after [`Self::set_live_players`].
    pub fn play_showcase(&mut self, dt: f64) {
        if let Some(showcase) = &mut self.showcase {
            showcase.advance(dt);
            showcase.draws(&self.hand_weapons, &mut self.joints, &mut self.live_draws);
        }
    }

    /// A body's water level this frame (`LiquidInteractionSystem.notify`),
    /// against the map's liquids and pools: `local` is our own predicted
    /// body, heard at our ears.
    pub fn notify_liquid(&mut self, id: &str, level: u8, position: DVec3, vertical_speed: f64, local: bool) {
        let shot_world = self.shot_world.as_ref();
        let contents = |p: DVec3| {
            shot_world.map_or(0, |w| {
                to_shared_contents(
                    w.point_contents(shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z), None),
                )
            })
        };
        let collision: Option<&dyn Fn(DVec3) -> u32> = shot_world.is_some().then_some(&contents);
        self.liquid.notify(
            id,
            level,
            LiquidKind::Water,
            position,
            vertical_speed,
            local,
            self.liquid_clock,
            collision,
            self.water.as_mut().map(|w| w as &mut dyn WaterRipples),
        );
    }

    /// A demo seek (`flushInterpolation`, the presentation clock's reset):
    /// nothing drawn from the old position carries across the jump, or a
    /// player's move would read as a teleport, a splash or a trail.
    pub fn discontinuity(&mut self) {
        self.teleport_last.clear();
        let bodies: Vec<String> = self.liquid.body_ids().map(str::to_owned).collect();
        for id in bodies {
            self.liquid.forget(&id);
        }
        for (_, (def, mut trail)) in self.live_trails.drain() {
            trail.reset();
            self.spare_trails.push((def.id, trail));
        }
        self.gibs.clear();
        self.spatter.clear();
        self.slide_dust.reset();
        self.bomb_debris.clear();
        self.damage_numbers.clear();
    }

    /// Draws the projectiles in flight this frame, facing along their
    /// velocity, each carrying its pool light; call after
    /// [`Self::set_live_players`], which clears the frame's live draws.
    /// `camera` is the eye our own shots leave the gun under.
    pub fn set_live_projectiles(&mut self, projectiles: &[LiveProjectile], dt: f64, camera: &Camera) {
        self.live_light_clock += dt;
        // `processProjectileEntities`' muzzle: a projectile first seen now,
        // near us and flying along our view, is our shot and starts at our
        // barrel (`getProjectileMuzzlePosition`); its drawing converges onto
        // the true flight (`updateServerPosition`).
        let clock = self.live_light_clock;
        self.projectile_seen
            .retain(|_, seen| clock - *seen <= PROJECTILE_FORGET_SECONDS);
        self.projectile_offsets
            .retain(|_, (_, born)| clock - *born < MUZZLE_CONVERGE_SECONDS);
        for p in projectiles {
            let new = upsert(&mut self.projectile_seen, &p.id, clock).is_none();
            if new && let Some(view_model) = &self.view_model {
                let position = DVec3::from(p.position);
                if let Some(muzzle) = projectile_muzzle(view_model, camera, position, DVec3::from(p.velocity)) {
                    self.projectile_offsets.insert(p.id.clone(), (muzzle - position, clock));
                }
            }
        }
        // The projectiles gone since the last frame give back their lights
        // and their trails.
        let flying: HashSet<&str> = projectiles.iter().map(|p| p.id.as_str()).collect();
        let lights = &mut self.lights;
        self.live_projectile_lights.retain(|id, handle| {
            let keep = flying.contains(id.as_str());
            if !keep {
                lights.release(*handle);
            }
            keep
        });
        let spare = &mut self.spare_trails;
        self.live_trails.retain(|id, (def, trail)| {
            let keep = flying.contains(id.as_str());
            if !keep {
                trail.reset();
                spare.push((def.id, std::mem::take(trail)));
            }
            keep
        });
        // A shot into a pool splashes (`ServerProjectiles`).
        let gone: Vec<String> = self
            .liquid
            .projectile_ids()
            .filter(|id| !flying.contains(id))
            .map(str::to_owned)
            .collect();
        for id in gone {
            self.liquid.forget_projectile(&id);
        }
        if let Some(world) = self.shot_world.as_ref() {
            let contents = |p: DVec3| {
                to_shared_contents(
                    world.point_contents(shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z), None),
                )
            };
            for p in projectiles {
                self.liquid.notify_projectile(
                    &p.id,
                    DVec3::from(p.position),
                    js_hypot(&p.velocity),
                    Some(&contents),
                    self.water.as_mut().map(|w| w as &mut dyn WaterRipples),
                );
            }
        }
        for p in projectiles {
            let Some((def, model)) = self.live_projectile_models.iter().find(|(d, _)| d.id == p.def_id) else {
                continue;
            };
            let drawn = DVec3::from(p.position)
                + self
                    .projectile_offsets
                    .get(&p.id)
                    .map_or(DVec3::ZERO, |(offset, born)| {
                        *offset * (1.0 - (clock - born) / MUZZLE_CONVERGE_SECONDS)
                    });
            let position = drawn.as_vec3();
            let velocity = DVec3::from(p.velocity).as_vec3();
            let direction = if velocity.length_squared() > 1e-6 {
                velocity
            } else {
                Vec3::X
            };
            let scale = def.scale.unwrap_or(1.0) as f32;
            model.draws(
                projectile_matrix(position, direction, scale),
                self.item_time,
                &mut self.live_draws,
            );
            if def.trail_effect.is_some() {
                let def: &'static ProjectileDef = def;
                let spare = &mut self.spare_trails;
                if !self.live_trails.contains_key(&p.id) {
                    let reused = spare
                        .iter()
                        .position(|(id, _)| *id == def.id)
                        .map(|i| spare.swap_remove(i).1);
                    self.live_trails.insert(p.id.clone(), (def, reused.unwrap_or_default()));
                }
                let (_, trail) = self.live_trails.get_mut(&p.id).expect("inserted above");
                trail.push(drawn, DVec3::from(p.velocity).normalize_or_zero(), def.trail_length);
            }
            if let Some(intensity) = def.light_intensity.filter(|i| *i > 0.0) {
                match self.live_projectile_lights.get(&p.id) {
                    Some(&handle) => self.lights.set_position(handle, position),
                    None => {
                        let color = def
                            .light_color
                            .map_or(Vec3::ONE, |c| Vec3::new(c.r as f32, c.g as f32, c.b as f32));
                        let handle = self.lights.acquire(
                            self.live_light_clock,
                            position,
                            color,
                            intensity as f32,
                            def.light_range.unwrap_or(20.0) as f32,
                        );
                        self.live_projectile_lights.insert(p.id.clone(), handle);
                    }
                }
            }
        }
    }

    /// Advances the effects' clock by `dt` and starts the effects of the
    /// events that arrived (`GameEvents.ts`): a projectile's impact explodes
    /// with its flash; other players' shots draw their streaks, pellets,
    /// beams and rails.
    /// `getGroundSurfaceFlags`: the surface flags of the ground up to 48
    /// units under `at`, 0 when nothing is there.
    pub fn ground_surface_flags(&mut self, at: DVec3) -> i32 {
        use shagger::babylon::maths::math_vector::Vector3;
        use shagger::physics::collision_backend::TraceOptions;
        use shagger::physics::content_flags::masks;
        let Some(world) = &mut self.shot_world else { return 0 };
        let trace = world.trace(
            Vector3::new(at.x, at.y, at.z),
            Vector3::new(at.x, at.y - 48.0, at.z),
            TraceOptions {
                radius: Some(1.0),
                height: Some(0.0),
                content_mask: Some(masks::PLAYERSOLID),
            },
        );
        if trace.fraction < 1.0 {
            trace.hit_surface_flags
        } else {
            0
        }
    }

    /// FrameEffects' damage numbers and cartoon hits, then GameEvents'
    /// `handleOutgoingDamage` for each hit the player on screen dealt
    /// (`view_entity`): a number over the victim, and a sprite for a hard one.
    pub fn play_hit_feedback(&mut self, events: &[SnapshotEvent], view_entity: Option<&str>, dt: f64) {
        self.damage_numbers.update(dt);
        self.cartoon_hits.update(dt);
        let Some(viewer) = view_entity else {
            return;
        };
        for event in events {
            if let SnapshotEvent::Damage {
                entity_id,
                attacker_id,
                damage,
                at,
                hit_location,
                ..
            } = event
                && attacker_id == viewer
                && attacker_id != entity_id
            {
                let at = DVec3::new(at.x, at.y, at.z);
                if *damage > 0 {
                    self.damage_numbers.spawn(at, *damage, *hit_location);
                }
                self.cartoon_hits.spawn(at, *damage);
            }
        }
    }

    /// FrameEffects' `HitSpatter.update` and GameEvents' `spatterHit`: blood
    /// (sparks off a robot) from where each hit landed, away from whoever
    /// made it, on anyone but the body the view is in (`view_entity`).
    /// `players` and `local` (our entity and feet) place the shooters.
    pub fn play_spatter(
        &mut self,
        events: &[SnapshotEvent],
        view_entity: Option<&str>,
        players: &[(String, InterpolatedEntity, PlayerModelVisualStyle)],
        local: Option<(&str, DVec3)>,
        dt: f64,
    ) {
        self.spatter.update(dt);
        let splats_on = with_cvars(|c| c.get_bool("cl_blood_splats"));
        for event in events {
            let SnapshotEvent::Damage {
                entity_id,
                attacker_id,
                damage,
                weapon_id,
                at,
                hit_location,
                ..
            } = event
            else {
                continue;
            };
            if Some(entity_id.as_str()) == view_entity || *damage == 0 || bloodless(weapon_id) {
                continue;
            }
            let position_of = |id: &str| {
                local.filter(|(l, _)| *l == id).map(|(_, at)| at).or_else(|| {
                    players
                        .iter()
                        .find(|(p, _, _)| p == id)
                        .map(|(_, e, _)| DVec3::new(e.x, e.y, e.z))
                })
            };
            let shooter = (attacker_id != entity_id).then(|| position_of(attacker_id)).flatten();
            let (model, scale) = self.live_models.get(entity_id).copied().unwrap_or(("", 1.0));
            let hit = Hit {
                origin: DVec3::new(at.x, at.y, at.z),
                zone: HitZone::from_location(*hit_location),
                damage: f64::from(*damage),
                away: shooter.map(|s| (at.x - s.x, at.z - s.z)),
                robot: model == "robot",
                scale,
            };
            self.spawn_spatter(&hit, splats_on);
        }
    }

    /// One hit's spatter, with the floor straight under it: one trace for
    /// all its pieces.
    pub fn spawn_spatter(&mut self, hit: &Hit, splats_on: bool) {
        use shagger::physics::collision_backend::TraceOptions;
        use shagger::physics::content_flags::masks;
        let from = HitSpatter::hit_point(hit);
        let floor = self.shot_world.as_mut().and_then(|w| {
            let v = |p: DVec3| shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z);
            let trace = w.trace(
                v(from),
                v(from - DVec3::Y * washoi::client::hit_spatter::FLOOR_REACH),
                TraceOptions {
                    radius: Some(0.0),
                    height: Some(0.0),
                    content_mask: Some(masks::SHOT),
                },
            );
            (!trace.start_solid && trace.fraction < 1.0).then_some(trace.end_pos.y)
        });
        self.spatter.spawn(hit, floor, splats_on);
    }

    /// The shards fly on, and GameEvents' `handleDeathEvent` breaks a body
    /// up when the death gibs: a rail, the bomb, or an explosion of 50 or
    /// more. The shards take the colour the body was drawn in.
    pub fn play_gibs(&mut self, events: &[SnapshotEvent], dt: f64) {
        use shagger::physics::collision_backend::TraceOptions;
        use shagger::physics::content_flags::masks;
        {
            let shot_world = self.shot_world.as_mut();
            let mut trace = shot_world.map(|w| {
                move |from: DVec3, to: DVec3| {
                    let v = |p: DVec3| shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z);
                    let hit = w.trace(
                        v(from),
                        v(to),
                        TraceOptions {
                            radius: Some(0.0),
                            height: Some(0.0),
                            content_mask: Some(masks::SHOT),
                        },
                    );
                    if hit.start_solid || hit.fraction >= 1.0 {
                        return None;
                    }
                    let n = hit.normal?;
                    Some((
                        DVec3::new(hit.end_pos.x, hit.end_pos.y, hit.end_pos.z),
                        DVec3::new(n.x, n.y, n.z),
                    ))
                }
            });
            self.gibs.update(
                dt,
                trace
                    .as_mut()
                    .map(|t| t as &mut dyn FnMut(DVec3, DVec3) -> Option<(DVec3, DVec3)>),
            );
        }
        // RagdollSystem.update: a ragdoll that has faded bursts here.
        if let Some(ragdolls) = &mut self.ragdolls {
            ragdolls.update(dt);
            let gibs_on = with_cvars(|c| render_toggle(c, "r_gibs"));
            for (at, color) in ragdolls.take_gibs() {
                if gibs_on {
                    self.gibs.spawn(at, DVec3::ZERO, color);
                }
            }
        }
        for event in events {
            if let SnapshotEvent::Death {
                entity_id,
                weapon_id,
                damage_type,
                final_damage,
                at,
                vel,
                kb_dir,
                kill_weapon,
                hit_location,
                ..
            } = event
            {
                let facts = DeathFacts {
                    damage_type,
                    kill_weapon: kill_weapon.as_deref(),
                    hit_location: *hit_location,
                    kb_dir_x: kb_dir.x,
                    kb_dir_z: kb_dir.z,
                };
                let kind = if damage_type.is_empty() { weapon_id } else { damage_type };
                let gib = kind == "rail" || kind == "bomb" || (kind == "explosion" && *final_damage >= 50);
                let local = self
                    .local_player
                    .as_ref()
                    .filter(|(id, _)| id == entity_id)
                    .map(|(_, yaw)| *yaw);
                let style = match local {
                    Some(_) => self.local_style.as_ref(),
                    None => None,
                }
                .or_else(|| self.live_styles.get(entity_id))
                .cloned();
                let color = style.as_ref().and_then(|s| s.tint_color);
                let death_pos = DVec3::new(at.x, at.y, at.z);
                if gib {
                    // With gibs off (`r_gibs 0`) nothing flies, but the view
                    // still pulls back from where the body was.
                    if !with_cvars(|c| render_toggle(c, "r_gibs")) {
                        if let Some(yaw) = local {
                            self.local_death = Some((DeathFollow::Still, death_pos, yaw));
                        }
                        continue;
                    }
                    let burst = self.gibs.spawn(death_pos, DVec3::new(vel.x, vel.y, vel.z), color);
                    if let (Some(yaw), Some(burst)) = (local, burst) {
                        self.local_death = Some((DeathFollow::Gibs(burst), death_pos, yaw));
                    }
                    continue;
                }
                // The local player's corpse: a fresh body where they died,
                // facing as they looked (`createDetachedModelAt`).
                if let (Some(yaw), Some(renderer), Some(ragdolls)) =
                    (local, &mut self.remote_players, &mut self.ragdolls)
                {
                    let interp = InterpolatedEntity {
                        x: at.x,
                        y: at.y,
                        z: at.z,
                        yaw,
                        ..InterpolatedEntity::default()
                    };
                    let id = format!("{entity_id}#corpse");
                    renderer.update(&id, &interp, 0.0, 1.0, Some("idle"), None);
                    let world = renderer.world_of(&id, &interp);
                    if let (Some(DetachedBody { body, instance }), Some(world)) = (renderer.take(&id), world)
                        && let Some(serial) = ragdolls.spawn(
                            body,
                            instance,
                            world,
                            DVec3::new(kb_dir.x, kb_dir.y, kb_dir.z),
                            style.as_ref(),
                            Some(&facts),
                        )
                    {
                        self.local_death = Some((DeathFollow::Ragdoll(serial), death_pos, yaw));
                    }
                    continue;
                }
                // `detachModel`: the body leaves the renderer for a ragdoll.
                let Some(renderer) = &mut self.remote_players else {
                    continue;
                };
                let Some(DetachedBody { body, instance }) =
                    renderer.take(entity_id).or_else(|| self.departed.remove(entity_id))
                else {
                    continue;
                };
                self.ragdolled.insert(entity_id.clone());
                let world = self.live_worlds.get(entity_id).copied();
                match (&mut self.ragdolls, world) {
                    (Some(ragdolls), Some(world)) => {
                        ragdolls.spawn(
                            body,
                            instance,
                            world,
                            DVec3::new(kb_dir.x, kb_dir.y, kb_dir.z),
                            style.as_ref(),
                            Some(&facts),
                        );
                    }
                    // No ragdolls: the body breaks up instead.
                    _ => {
                        self.gibs.spawn(death_pos, DVec3::new(vel.x, vel.y, vel.z), color);
                    }
                }
            }
        }
    }

    /// Who holds which weapon (`enemyWeapons`), before
    /// [`Self::set_live_players`].
    pub fn set_live_weapons(&mut self, weapons: &Arc<HashMap<String, String>>) {
        // The feed's own map, shared: no copy a frame.
        self.live_weapons = Arc::clone(weapons);
    }

    /// Names the local player and their view yaw before [`Self::play_gibs`],
    /// so their death leaves a corpse for the death camera.
    /// wash-fps `GameEvents.ts`'s charge glows: a teal light 30 units
    /// above each other player charging their blade, at most six, pulsing
    /// between 4 and 12 (`Game.ts`).
    fn update_charge_glows(&mut self, players: &[&(String, InterpolatedEntity, PlayerModelVisualStyle)]) {
        self.charge_glows.clear();
        let pulse = 8.0 + 4.0 * (self.item_time * 8.0).sin();
        let color = Vec3::new(0.05, 1.0, 0.9);
        for id in &self.charging {
            if self.charge_glows.len() >= MAX_REMOTE_CHARGE_GLOW_LIGHTS {
                break;
            }
            let Some((_, p, _)) = players.iter().find(|(p, _, _)| p == id) else {
                continue;
            };
            self.charge_glows.push(DynamicLight {
                position: Vec3::new(p.x as f32, (p.y + 30.0) as f32, p.z as f32),
                diffuse: color * pulse as f32,
                specular: color * pulse as f32,
                range: 60.0,
            });
        }
    }

    pub fn set_local_player(&mut self, id: Option<&str>, yaw: f64) {
        self.local_player = id.map(|id| (id.to_owned(), yaw));
    }

    /// `setFollowedMuzzle`: the player watched through their eyes, whose
    /// beams, rails and shots leave the gun on screen.
    pub fn set_followed_player(&mut self, id: Option<&str>) {
        self.followed_player = id.map(str::to_owned);
    }

    /// The style the local player's corpse is drawn in: their own body,
    /// never drawn, has no entry among the live players'.
    pub fn set_local_style(&mut self, style: Option<PlayerModelVisualStyle>) {
        self.local_style = style;
    }

    /// The local player's death this frame: what to follow, where they died
    /// and their yaw (the death camera's `start`).
    pub fn take_local_death(&mut self) -> Option<(DeathFollow, DVec3, f64)> {
        self.local_death.take()
    }

    /// The follow handle's `getPosition`.
    #[must_use]
    pub fn follow_position(&self, follow: DeathFollow) -> Option<DVec3> {
        match follow {
            DeathFollow::Ragdoll(serial) => self.ragdolls.as_ref()?.position_of(serial),
            DeathFollow::Gibs(burst) => self.gibs.burst_position(burst),
            DeathFollow::Still => None,
        }
    }

    /// A ray through the map's brushes (shot mask): where it first hits.
    pub fn trace_map(&mut self, from: DVec3, to: DVec3) -> Option<DVec3> {
        use shagger::physics::collision_backend::TraceOptions;
        use shagger::physics::content_flags::masks;
        let world = self.shot_world.as_mut()?;
        let v = |p: DVec3| shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z);
        let hit = world.trace(
            v(from),
            v(to),
            TraceOptions {
                radius: Some(0.0),
                height: Some(0.0),
                content_mask: Some(masks::SHOT),
            },
        );
        (!hit.start_solid && hit.fraction < 1.0).then(|| DVec3::new(hit.end_pos.x, hit.end_pos.y, hit.end_pos.z))
    }

    /// `FPSPickup.reconcileActiveSet` with the server's word on which map
    /// pickups are up, by id, then `dt` seconds of the procedural pickups'
    /// collection bursts and respawn assemblies.
    pub fn play_pickups(&mut self, active: &[bool], dt: f64) {
        self.pickup_active.clear();
        self.pickup_active.extend_from_slice(active);
        let mask = &self.pickup_active;
        self.items
            .reconcile(self.item_time, dt, |id| mask.get(id as usize).copied());
    }

    /// The weapons a player at `at` (the physics origin) stands at, for the
    /// swap prompt: the nearest map weapon that is up
    /// (`FPSPickup.onProximity`) and the nearest weapon on the floor
    /// (`checkPickupProximity`).
    #[must_use]
    pub fn weapons_in_reach(&self, at: DVec3) -> (Option<&'static str>, Option<&str>) {
        let nearest = |spots: &mut dyn Iterator<Item = (DVec3, usize)>| {
            spots
                .filter(|(p, _)| in_pickup_range(at, *p))
                .min_by(|(a, _), (b, _)| horizontal_distance_sq(at, *a).total_cmp(&horizontal_distance_sq(at, *b)))
                .map(|(_, i)| i)
        };
        let map = nearest(
            &mut self
                .weapon_pickup_spots
                .iter()
                .enumerate()
                .filter(|(_, (id, _, _))| pickup_shown(&self.pickup_active, *id))
                .map(|(i, (_, _, p))| (*p, i)),
        )
        .map(|i| self.weapon_pickup_spots[i].1);
        let floor = self
            .dropped_weapons
            .visuals()
            .map(|(_, w)| (DVec3::from_array(w.position), w))
            .filter(|(p, _)| in_pickup_range(at, *p))
            .min_by(|(a, _), (b, _)| horizontal_distance_sq(at, *a).total_cmp(&horizontal_distance_sq(at, *b)))
            .map(|(_, w)| w.weapon_id.as_str());
        (map, floor)
    }

    /// `GameEvents`' dropped weapons, after [`Self::play_effects`] this
    /// frame: the throws `weapon_drop` starts, the snapshot's weapons on the
    /// floor, and `dt` seconds of their motion through the map.
    pub fn play_dropped_weapons(
        &mut self,
        events: &[SnapshotEvent],
        items: &HashMap<String, DroppedWeaponInfo>,
        dt: f64,
    ) {
        let now = self.effect_time * 1000.0;
        for event in events {
            if let SnapshotEvent::WeaponDrop { entity_id, at, vel, .. } = event {
                let thrown = DropThrow {
                    start: [at.x, at.y, at.z],
                    velocity: [vel.x, vel.y, vel.z],
                };
                self.dropped_weapons.stash_throw(entity_id, thrown, now);
            }
        }
        self.dropped_weapons.sync_server_entities(items, now);
        // The shots' collision is the server's map: the throws step through it.
        let Some(backend) = self.shot_world.take() else {
            self.dropped_weapons
                .update(dt, None::<&mut SharedCollisionWorld<SweepBackendAdapter>>);
            return;
        };
        let mut world = SharedCollisionWorld::new(SweepBackendAdapter { backend });
        self.dropped_weapons.update(dt, Some(&mut world));
        self.shot_world = Some(world.backend.backend);
    }

    /// `camera` is the eye our own gun is drawn under.
    pub fn play_effects(
        &mut self,
        events: &[SnapshotEvent],
        weapons: &HashMap<String, String>,
        dt: f64,
        camera: &Camera,
    ) {
        self.effect_time += dt;
        let now = self.effect_time;
        self.explosions.update(now);
        // Killers taunt once the whole snapshot has run, so the killing
        // shot's own fire event cannot end the taunt it caused.
        let mut kill_taunts: Vec<(String, &'static str)> = Vec::new();
        for event in events {
            if let SnapshotEvent::Death {
                entity_id,
                killer_id,
                at,
                ..
            } = event
                && let Some(taunt) = choose_kill_taunt(&KillFacts {
                    entity_id,
                    killer_id,
                    x: at.x,
                    y: at.y,
                    z: at.z,
                })
            {
                kill_taunts.push((killer_id.clone(), taunt));
            }
            // `weapon_charge` from someone else: the glow on them comes and
            // goes with it (our own charge shows on the view model).
            if let SnapshotEvent::WeaponCharge {
                entity_id, charging, ..
            } = event
                && self.local_player.as_ref().is_none_or(|(id, _)| id != entity_id)
            {
                self.charging.retain(|id| id != entity_id);
                if *charging {
                    self.charging.push(entity_id.clone());
                }
            }
            // `handleRemoteWeaponFire`: another player's gun stays raised a
            // moment, and their blade swings.
            if let SnapshotEvent::WeaponFire {
                entity_id, weapon_id, ..
            } = event
                && self.local_player.as_ref().is_none_or(|(id, _)| id != entity_id)
                && let Some(renderer) = &mut self.remote_players
            {
                renderer.note_weapon_fire(entity_id, now);
                if washoi::client::game_events::canonical_weapon_id(weapon_id) == "blade" {
                    renderer.play_attack_anim(entity_id, "blade_slash", 2.0);
                }
            }
            if let SnapshotEvent::ProjectileImpact { def_id, at, normal } = event {
                // Older demos name the weapon; anything else is a rocket.
                let projectile = match def_id.as_str() {
                    "glauncher" => "grenade",
                    "plasmagun" => "plasma_bolt",
                    "rlauncher" => "rocket",
                    other => other,
                };
                let look =
                    explosion_for(projectile).unwrap_or_else(|| explosion_for("rocket").expect("the rocket explodes"));
                let position = DVec3::new(at.x, at.y, at.z);
                let normal = DVec3::new(normal.x, normal.y, normal.z);
                let mut random = mulberry32(self.live_flashes.len() as u32 + now.to_bits() as u32);
                self.explosions.spawn(now, position, look, normal, &mut random);
                self.spawned_explosions.push((position, look));
                if let Some((intensity, range)) = explosion_flash(look, 0.0) {
                    let light = self.lights.acquire(
                        self.live_light_clock,
                        position.as_vec3(),
                        look.outer_color,
                        intensity,
                        range,
                    );
                    self.live_flashes.push((light, look, now));
                }
            }
        }
        let mut no_world = ();
        let world: &mut dyn ShotWorld = match &mut self.shot_world {
            Some(world) => world,
            None => &mut no_world,
        };
        play_shots(
            events,
            &mut EffectTargets {
                now,
                hitscan: &mut self.hitscan,
                world,
                lights: &mut self.lights,
            },
            &|id| weapons.get(id).cloned(),
            &FrameMuzzles {
                local: self.local_player.as_ref().map(|(id, _)| id.as_str()),
                followed: self.followed_player.as_deref(),
                view: self.view_model.as_ref().map(|v| (v, camera)),
                remote: &self.remote_muzzles,
            },
        );
        if let Some(renderer) = &mut self.remote_players {
            for (killer, taunt) in kill_taunts {
                renderer.play_kill_taunt(&killer, taunt, now);
            }
        }
        self.hitscan.update(now, &mut self.lights);
        // Fade the flashes, and give back the ones that are out.
        let lights = &mut self.lights;
        self.live_flashes
            .retain(|&(light, look, born)| match explosion_flash(look, now - born) {
                Some((intensity, range)) => {
                    lights.set_intensity_range(light, intensity, range);
                    true
                }
                None => {
                    lights.release(light);
                    false
                }
            });
    }

    /// The underwater view (`UnderwaterView`, run before the camera renders):
    /// asks what the camera's eye is in, eases the tint and fog, and warps
    /// `camera`'s field of view. Call before [`Self::prepare`].
    pub fn update_underwater(&mut self, camera: &mut Camera, dt: f64) {
        let shot_world = self.shot_world.as_ref();
        let contents = |p: DVec3| {
            shot_world.map_or(0, |w| {
                to_shared_contents(
                    w.point_contents(shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z), None),
                )
            })
        };
        let comfort = shagger::core::cvar::with_cvars(|c| c.get_number("r_underwater_distortion"));
        self.underwater_fog = self.underwater.update(
            camera,
            dt.min(0.1),
            shot_world.is_some().then_some(&contents as &dyn Fn(DVec3) -> u32),
            self.fog.scene_color(),
            comfort,
        );
    }

    /// The fog every material draws with this frame: the underwater view's
    /// while it holds the scene fog, else world fog.
    fn frame_fog(&self) -> FogUniforms {
        self.underwater_fog.map_or_else(
            || self.fog.uniforms(),
            |f| FogUniforms::scene_exp2(f.color, f.density, self.fog.sky_height()),
        )
    }

    pub fn prepare(&mut self, gpu: &Gpu, camera: &Camera, size: [f32; 2]) {
        let aspect = size[0] / size[1];
        self.frame_height = f64::from(size[1]);
        // `cl_gunsize` shrinks the gun into the screen's corner, by its shape.
        if let Some(view_model) = &mut self.view_model {
            view_model.view.aspect_ratio = f64::from(aspect);
        }
        // The camera's matrices, once for the frame.
        let view_proj = camera.view_projection(aspect);
        let view = camera.view();
        let rotation = camera_rotation(view.as_dmat4());
        self.culling.update(camera, aspect);
        if let Some(pvs) = &mut self.item_pvs {
            pvs.update(Some(camera.position));
        }
        // Every cvar the frame reads, in one look.
        let cvars = with_cvars(|c| {
            let serial = c.serial();
            if self.fog_serial != Some(serial) {
                self.fog.refresh(c);
            }
            self.fog_serial = Some(serial);
            FrameCvars::read(c)
        });
        // `r_dynamiclight 0` (the Ultralow preset): no pool lights.
        let dynamic = self.dynamic_lights && cvars.dynamic_lights;
        let mut lights = std::mem::take(&mut self.frame_lights);
        lights.clear();
        if dynamic {
            lights.extend(
                self.lights
                    .lights()
                    .chain(self.charge_glows.iter().copied())
                    .chain(self.bomb.carry_light()),
            );
        }
        self.models.set_dynamic_lights(&lights);
        self.world.set_dynamic_lights(&lights);
        self.frame_lights = lights;
        // `ColorCorrectionPostProcess`, live from its cvars.
        self.image_processing.color_matrix = cvars.color_matrix;
        let uniforms = self.frame_fog();
        if self.applied_fog != Some(uniforms) {
            self.applied_fog = Some(uniforms);
            self.world.set_fog(uniforms);
            self.models.set_fog(uniforms);
            self.explosions.set_fog(uniforms);
            if let Some(sky) = &mut self.sky {
                sky.set_fog(uniforms);
            }
        }
        // `FogVolumes.prepare`: the scene depth while a fog volume is in view.
        let fog_in_view = self.fog_volumes.any_in_view(view_proj);
        self.world.set_scene_depth_reader(fog_in_view);
        // `Q3SoftParticleDepth`: the scene depth, while `r_softhalos` is on
        // and a soft halo is in view.
        let soft = cvars.soft_halos
            && self
                .world
                .has_visible_soft_particles(if self.cull { self.culling.visible() } else { &[] });
        self.world.set_soft_particles(
            gpu,
            view,
            camera.near,
            camera.far,
            (size[0].round() as u32, size[1].round() as u32),
            soft,
        );
        self.world.prepare(gpu, view_proj, camera.position);
        // The weapon pickups lead `model_draws`; the taken ones stay out.
        self.frame_draws.clear();
        let mut next = 0;
        let pvs = self.cull.then_some(self.item_pvs.as_ref()).flatten();
        for (id, span) in &self.weapon_pickup_spans {
            let spot = self.weapon_pickup_spots.iter().find(|(i, _, _)| i == id).map(|s| s.2);
            if !pickup_shown(&self.pickup_active, *id) || spot.is_some_and(|at| !in_pvs(pvs, at)) {
                self.frame_draws.extend_from_slice(&self.model_draws[next..span.start]);
                next = span.end;
            }
        }
        self.frame_draws.extend_from_slice(&self.model_draws[next..]);
        self.frame_draws.extend_from_slice(&self.live_draws);
        // `DroppedWeaponRenderer.update` step 3: PVS decides what is drawn.
        for (_, weapon) in self
            .dropped_weapons
            .visuals()
            .filter(|(_, w)| in_pvs(pvs, DVec3::from(w.position)))
        {
            self.hand_weapons.world_draws(
                &weapon.weapon_id,
                weapon.position,
                weapon.rotation,
                &mut self.frame_draws,
            );
        }
        self.gib_renderer.draws(&self.gibs, &mut self.frame_draws);
        self.spatter_renderer.draws(&self.spatter, &mut self.frame_draws);
        self.bomb.draws(&mut self.frame_draws);
        self.slide_dust.eye = Some(camera.position.as_dvec3());
        self.slide_dust_renderer.draws(&self.slide_dust, &mut self.frame_draws);
        for fragment in self.bomb_debris.fragments() {
            if let Some(model) = self.debris_models.get(&fragment.path) {
                model.draws(fragment.world(), self.item_time, &mut self.frame_draws);
            }
        }
        // The ragdolls' palettes go after the frame's, and leave with it.
        let joints_base = self.joints.len();
        if let Some(ragdolls) = &self.ragdolls {
            ragdolls.draws(&mut self.joints, &mut self.frame_draws);
            self.ragdoll_trails
                .draws(gpu, &mut self.models, ragdolls, &mut self.frame_draws);
        }
        self.items
            .draws(self.item_time, rotation, |at| in_pvs(pvs, at), &mut self.frame_draws);
        let active = &self.pickup_active;
        self.item_models.draws(
            self.item_time,
            camera.position.as_dvec3(),
            rotation,
            |id, at| pickup_shown(active, id) && in_pvs(pvs, at),
            &mut self.frame_draws,
        );
        for (model, world) in &self.projectiles {
            model.draws(*world, self.item_time, &mut self.frame_draws);
        }
        for (trail, color) in &mut self.trails {
            trail.update(gpu, &mut self.models, Some(camera.position.as_dvec3()), *color);
            trail.draws(&mut self.frame_draws);
        }
        if let Some(view_model) = &mut self.view_model {
            let now = view_model.view.clock * 1000.0;
            let slash = &mut view_model.view.slash;
            slash.update_trail(
                now,
                camera.position.as_dvec3(),
                camera.forward().as_dvec3(),
                camera.up().as_dvec3(),
                camera.right().as_dvec3(),
            );
            self.slash_trails
                .draws(gpu, &mut self.models, slash, now, &mut self.frame_draws);
        }
        for (def, trail) in self.live_trails.values_mut() {
            let color = def.light_color.map(|c| Vec3::new(c.r as f32, c.g as f32, c.b as f32));
            trail.update(gpu, &mut self.models, Some(camera.position.as_dvec3()), color);
            trail.draws(&mut self.frame_draws);
        }
        self.models
            .prepare(gpu, view_proj, camera.position, &self.frame_draws, &self.joints);
        self.joints.truncate(joints_base);
        self.explosions.prepare(gpu, view_proj, view, self.effect_time);
        let frame = EffectFrame {
            view_proj,
            view,
            projection: camera.projection(aspect),
            camera_position: camera.position,
            viewport: size,
            time: self.effect_time,
            fog: uniforms,
        };
        self.fog_volumes
            .prepare(gpu, &frame, fog_in_view && self.world.scene_depth_copied());
        self.hitscan.wall_marks.prepare(gpu, &frame);
        self.hitscan.streaks.prepare(gpu, &frame);
        // The muzzle flashes ride the live barrel (`setMuzzleAnchorResolver`),
        // the blade's tip flashes its tip.
        if let Some(view_model) = &self.view_model {
            let height = self.frame_height;
            let tips = &self.blade_tip_flashes;
            self.hitscan.muzzle_flashes.track_anchors(|anchor| match anchor {
                None => Some(view_model.muzzle_world_position(camera, 5.0)),
                Some(id) => tips
                    .iter()
                    .find(|(a, _)| *a == id)
                    .and_then(|(_, jitter)| view_model.blade_tip_world_position(camera, *jitter, height)),
            });
        }
        // Remote beams leave their shooters' guns as they are now; the
        // watched player's, the gun on screen.
        let muzzles = FrameMuzzles {
            local: None,
            followed: self.followed_player.as_deref(),
            view: self.view_model.as_ref().map(|v| (v, camera)),
            remote: &self.remote_muzzles,
        };
        self.hitscan
            .refresh_remote_lightning_starts(&mut |id, direction| muzzles.remote_muzzle(id, direction));
        self.hitscan.muzzle_flashes.prepare(gpu, &frame);
        self.hitscan.impact_sparks.prepare(gpu, &frame);
        self.hitscan.rails.prepare(gpu, &frame);
        self.hitscan.voltaic.prepare(gpu, &frame);
        if let Some(sky) = &self.sky {
            sky.prepare(gpu, camera, aspect);
        }
        {
            let shot_world = self.shot_world.as_ref();
            let contents = |p: DVec3| {
                shot_world.map_or(0, |w| {
                    to_shared_contents(
                        w.point_contents(shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z), None),
                    )
                })
            };
            self.liquid.update(
                self.item_time - self.water_time,
                shot_world.is_some().then_some(&contents as &dyn Fn(DVec3) -> u32),
                self.water.as_mut().map(|w| w as &mut dyn WaterRipples),
            );
        }
        self.splash
            .prepare(gpu, &frame, self.liquid.droplets().map_or(&[], |d| d.sprites()));
        self.rings
            .prepare(gpu, &frame, self.held_rings.iter().chain(self.liquid.rings()));
        self.teleport_renderer.prepare(gpu, &frame, &self.teleports);
        self.map_particles.prepare(gpu, &frame, self.item_time * 1000.0);
        self.damage_number_renderer.prepare(gpu, &frame, &self.damage_numbers);
        if let Some(cartoon) = &mut self.cartoon_hit_renderer {
            cartoon.prepare(gpu, &frame, &self.cartoon_hits);
        }
        if let Some(water) = &mut self.water {
            water.update(self.item_time - self.water_time);
            self.water_time = self.item_time;
            let settings = with_cvars(|c| WaterSettings {
                quality: c.get_number("r_water_quality"),
                debug: c.get_number("r_water_debug"),
                highlights: c.get_number("r_water_highlights"),
                roughness: c.get_number("r_water_roughness"),
                reflection: c.get_number("r_water_reflection"),
                blur: c.get_number("r_water_blur"),
                detail: c.get_number("r_water_detail"),
                contact: c.get_number("r_water_contact"),
            });
            let mut actors = std::mem::take(&mut self.frame_actors);
            actors.clear();
            actors.extend(self.static_actors.iter().chain(&self.live_actors).map(|(a, _)| *a));
            let visible = if self.cull { self.culling.visible() } else { &[] };
            water.prepare(
                gpu,
                camera,
                (size[0] as u32, size[1] as u32),
                &settings,
                uniforms,
                visible,
                &actors,
                cvars.water_actors,
            );
            self.frame_actors = actors;
            self.actor_draws.clear();
            let reflection = water.actor_reflection();
            if let Some((view_proj, eye)) = reflection.view() {
                self.models.set_second_view(gpu, view_proj, eye);
                let statics = self.static_actors.len();
                for &i in reflection.selected() {
                    let draws = if i < statics {
                        &self.model_draws[self.static_actors[i].1.clone()]
                    } else {
                        &self.live_draws[self.live_actors[i - statics].1.clone()]
                    };
                    self.actor_draws.extend_from_slice(draws);
                }
            }
        }
        if let Some(view_model) = &mut self.view_model {
            view_model.dynamic_lights = dynamic;
            view_model.prepare(gpu, camera, aspect);
        }
    }

    /// Records the first-person weapon, in a pass after [`Self::draw`]'s
    /// with the depth cleared.
    pub fn draw_overlay(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.showcase.is_some() {
            return;
        }
        if let Some(view_model) = &self.view_model {
            view_model.draw(pass);
        }
    }

    /// Records the sky and the opaque world and models.
    pub fn draw_opaque(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(sky) = &self.sky {
            sky.draw(pass);
        }
        let visible = if self.cull { self.culling.visible() } else { &[] };
        self.world.draw_visible_opaque(pass, visible);
        self.models.draw_opaque(pass);
    }

    /// Copies the opaque scene for the water to look through, between
    /// [`Self::draw_opaque`] and [`Self::draw_transparent`].
    pub fn capture(&self, encoder: &mut wgpu::CommandEncoder, hdr: &HdrTarget) {
        if let Some(water) = &self.water {
            water.capture(encoder, &hdr.color_texture, &hdr.depth_texture);
            let actor_draws = &self.actor_draws;
            water.capture_actors(
                encoder,
                self.world.vertex_buffer(),
                self.world.index_buffer(),
                &self.models,
                &|d| actor_draws.contains(d),
            );
        }
        self.world.capture_soft_depth(encoder, &hdr.depth_texture);
    }

    /// Records the water, the blended world, the effects and the blended
    /// models, over what [`Self::draw_opaque`] drew.
    pub fn draw_transparent(&self, pass: &mut wgpu::RenderPass<'_>) {
        let visible = if self.cull { self.culling.visible() } else { &[] };
        if let Some(water) = &self.water {
            water.draw(pass, self.world.vertex_buffer(), self.world.index_buffer(), visible);
        }
        // The map's blended surfaces (alpha indices 0-999); its additive
        // ones (1000 and up) come after the effects and the map particles.
        self.world.draw_visible_alpha_blended(pass, visible);
        // Wall marks lie on the walls, under the shots' effects.
        self.hitscan.wall_marks.draw(pass);
        // The effects' alpha indices (40 to 46) put them before the models'.
        self.hitscan.voltaic.draw_beams(pass);
        self.hitscan.rails.draw_beams(pass);
        self.hitscan.voltaic.draw_impacts(pass);
        self.hitscan.rails.draw_impacts(pass);
        self.hitscan.streaks.draw(pass);
        self.hitscan.muzzle_flashes.draw(pass);
        self.hitscan.impact_sparks.draw(pass);
        self.rings.draw(pass);
        self.splash.draw(pass);
        self.teleport_renderer.draw(pass);
        self.explosions.draw(pass);
        // The fog volumes (alpha index 999.25), then `PARTICLE_ALPHA_INDEX`:
        // between the map's blended and additive surfaces, so additive glass
        // in front of a puff adds over it.
        self.fog_volumes.draw(pass, self.world.scene_depth());
        self.map_particles.draw(pass);
        self.world.draw_visible_additive(pass, visible);
        self.models.draw_blended(pass);
        self.damage_number_renderer.draw(pass);
        // Rendering group 1: over everything, so last and without depth.
        if let Some(cartoon) = &self.cartoon_hit_renderer {
            cartoon.draw(pass);
        }
    }
}

/// `map.insert(id, value)` without a new key when `id` is already there
/// (the per-frame maps keep the same players frame after frame): the value
/// it replaced, if any.
fn upsert<V>(map: &mut HashMap<String, V>, id: &str, value: V) -> Option<V> {
    match map.get_mut(id) {
        Some(slot) => Some(std::mem::replace(slot, value)),
        None => map.insert(id.to_owned(), value),
    }
}

/// The cvars [`MapRenderer::prepare`] reads every frame.
struct FrameCvars {
    /// `r_dynamiclight`.
    dynamic_lights: bool,
    /// `r_softhalos` (on when unset).
    soft_halos: bool,
    /// `r_water_actors`.
    water_actors: bool,
    /// `ColorCorrectionPostProcess`'s matrix from its cvars.
    color_matrix: Option<[f64; 9]>,
}

impl FrameCvars {
    fn read(c: &shagger::core::cvar::Cvars) -> Self {
        Self {
            dynamic_lights: render_toggle(c, "r_dynamiclight"),
            soft_halos: c.get("r_softhalos").is_none() || c.get_number("r_softhalos") != 0.0,
            water_actors: c.get_number("r_water_actors") != 0.0,
            color_matrix: active_color_matrix(
                c.get_number("r_colorcorrection"),
                c.get_number("r_colorcorrection_strength"),
                c.get_number("r_colorsimulation"),
            ),
        }
    }
}
