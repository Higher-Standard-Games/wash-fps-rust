//! Port of wash-fps `client/BombSiteMarkers.ts`: where the sites are, and
//! what happens when one goes up.
//!
//!  - **Markers.** A floating A/B beacon above each site, drawn through
//!    walls: both teams know where the sites are. The TypeScript builds a
//!    billboarded glyph plate and a tilted, turning torus in the scene with
//!    depth off; here [`BombSiteMarkers::draw`] projects the same shapes
//!    and draws them on the HUD layer, under the widgets.
//!
//!  - **The detonation.** The server sends the whole blast schedule at
//!    once, and [`BombSiteMarkers::update`] plays it out, handing back the
//!    blasts due this frame for the renderer to spawn and sound.

use shagger::rendering::glam::{DVec3, Mat4, Vec3, Vec4};
use shagger::shared::json::Value;
use shagger::ui::hud_2d::{Align, Hud2d, TextStyle, hex};

use crate::client::hud::washiki::Fonts;
use crate::client::match_state_manager::MatchStateManager;

/// Height above the site origin: clear of head height, under most ceilings.
pub const MARKER_HEIGHT: f64 = 128.0;
pub const MARKER_SIZE: f64 = 44.0;
/// Idle motion, so a marker reads as an objective rather than as scenery.
pub const BOB_AMPLITUDE: f64 = 6.0;
pub const BOB_RATE: f64 = 1.1;
/// Ring revolutions per second.
pub const SPIN_RATE: f64 = 0.9;
/// Tilt on the ring, so it never presents perfectly edge-on or flat.
pub const RING_TILT: f64 = 0.42;
pub const RING_THICKNESS: f64 = 2.4;
/// Amber, and deliberately not a team colour: a site belongs to the round.
pub const MARKER_GOLD_CSS: &str = "#ffbd3d";
pub const GLYPH_CSS: &str = "#fff4d6";
/// Backing behind the glyph (`rgba(24, 14, 2, 0.55)`).
pub const GLYPH_BACKING: [f32; 4] = [24.0 / 255.0, 14.0 / 255.0, 2.0 / 255.0, 0.55];
/// The ring's material alpha.
pub const RING_ALPHA: f32 = 0.75;
/// Site blasts are bigger than a rocket.
pub const BLAST_SCALE: f64 = 2.2;
/// How far the detonation is felt. Map-scale, not weapon-scale.
pub const BLAST_SHAKE_RANGE: f64 = 2600.0;
/// Seconds between blast sounds, so a long chain reads as blasts not mud.
pub const BLAST_SOUND_MIN_GAP: f64 = 0.09;
/// The torus's tessellation.
const RING_SEGMENTS: usize = 28;

/// One site from the bomb objective.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pub id: i64,
    pub label: String,
    pub position: DVec3,
}

/// A blast due this frame; `sound` when it is not throttled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blast {
    pub position: DVec3,
    pub radius: f64,
    pub sound: bool,
}

/// A beacon this frame: its label, centre (the root, bobbing) and the
/// ring's turn.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerPose {
    pub label: String,
    pub center: DVec3,
    pub spin: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
    z: f64,
    radius: f64,
    delay: f64,
}

fn number(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn explosion_points(explosion: Option<&Value>) -> Vec<Point> {
    explosion
        .and_then(|e| e.get("points"))
        .and_then(Value::as_array)
        .map(|points| {
            points
                .iter()
                .filter_map(|p| {
                    Some(Point {
                        x: number(p, "x")?,
                        y: number(p, "y")?,
                        z: number(p, "z")?,
                        radius: number(p, "radius").unwrap_or(0.0),
                        delay: number(p, "delay").unwrap_or(0.0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `bombExplosionSignature`: a stable per-round identity for a repeated
/// explosion snapshot; `None` without points.
#[must_use]
pub fn bomb_explosion_signature(round: u32, explosion: Option<&Value>) -> Option<String> {
    let points = explosion_points(explosion);
    if points.is_empty() {
        return None;
    }
    let points: Vec<String> = points
        .iter()
        .map(|p| format!("{},{},{},{},{}", p.x, p.y, p.z, p.radius, p.delay))
        .collect();
    Some(format!("{round}:{}", points.join(";")))
}

/// The bomb objective's sites.
#[must_use]
pub fn bomb_sites(bomb: Option<&Value>) -> Vec<Site> {
    bomb.and_then(|b| b.get("sites"))
        .and_then(Value::as_array)
        .map(|sites| {
            sites
                .iter()
                .filter_map(|s| {
                    Some(Site {
                        id: number(s, "id")? as i64,
                        label: s.get("label").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        position: DVec3::new(number(s, "x")?, number(s, "y")?, number(s, "z")?),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `BombSiteMarkers`.
#[derive(Debug, Clone)]
pub struct BombSiteMarkers {
    elapsed: f64,
    /// When the last blast sound played.
    last_blast_sound: f64,
    /// Blasts still waiting to fire, as absolute times in `elapsed`.
    pending: Vec<(f64, DVec3, f64)>,
    /// Explosions already played, so a repeated snapshot does not replay.
    played_signature: Option<String>,
    sites: Vec<Site>,
}

impl Default for BombSiteMarkers {
    fn default() -> Self {
        Self {
            elapsed: 0.0,
            last_blast_sound: f64::NEG_INFINITY,
            pending: Vec::new(),
            played_signature: None,
            sites: Vec::new(),
        }
    }
}

impl BombSiteMarkers {
    /// `update`: the sites to mark and the blasts due this frame.
    pub fn update(&mut self, dt: f64, match_state: &MatchStateManager) -> Vec<Blast> {
        self.elapsed += dt;
        // A mode without sites shows none.
        self.sites = bomb_sites(match_state.bomb());
        self.schedule_explosion(match_state.round(), match_state.explosion());
        self.fire_pending()
    }

    /// `scheduleExplosion`: the server's schedule into pending blasts,
    /// exactly once. The snapshot repeats the explosion for about a second,
    /// so a repeat is recognised by its signature.
    fn schedule_explosion(&mut self, round: u32, explosion: Option<&Value>) {
        let Some(signature) = bomb_explosion_signature(round, explosion) else {
            // The gap after the repeats arms the next detonation, including
            // round 1 of a rematch, where the round number can repeat.
            self.played_signature = None;
            return;
        };
        if self.played_signature.as_ref() == Some(&signature) {
            return;
        }
        self.played_signature = Some(signature);
        for p in explosion_points(explosion) {
            self.pending
                .push((self.elapsed + p.delay.max(0.0), DVec3::new(p.x, p.y, p.z), p.radius));
        }
    }

    /// `firePending`: the blasts whose time has come, the sound throttled
    /// so a thirty-point chain stays a sequence of blasts.
    fn fire_pending(&mut self) -> Vec<Blast> {
        let elapsed = self.elapsed;
        let (due, waiting): (Vec<_>, Vec<_>) = self.pending.iter().partition(|(at, _, _)| *at <= elapsed);
        self.pending = waiting;
        due.into_iter()
            .map(|(_, position, radius)| {
                let sound = elapsed - self.last_blast_sound >= BLAST_SOUND_MIN_GAP;
                if sound {
                    self.last_blast_sound = elapsed;
                }
                Blast {
                    position,
                    radius,
                    sound,
                }
            })
            .collect()
    }

    /// The beacons this frame (`syncMarkers`' bob and spin).
    #[must_use]
    pub fn poses(&self) -> Vec<MarkerPose> {
        self.sites
            .iter()
            .map(|site| MarkerPose {
                label: site.label.clone(),
                center: site.position
                    + DVec3::new(
                        0.0,
                        MARKER_HEIGHT + (self.elapsed * BOB_RATE + site.id as f64).sin() * BOB_AMPLITUDE,
                        0.0,
                    ),
                spin: self.elapsed * SPIN_RATE,
            })
            .collect()
    }

    /// `dispose`'s reset, for a new map or match.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.played_signature = None;
        self.sites.clear();
    }
}

/// A point in world space to CSS pixels, with its view depth; `None`
/// behind the camera.
fn project(view_proj: Mat4, size: [f32; 2], p: DVec3) -> Option<([f32; 2], f32)> {
    let clip = view_proj * Vec4::new(p.x as f32, p.y as f32, p.z as f32, 1.0);
    if clip.w <= 1e-3 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(([(ndc.x * 0.5 + 0.5) * size[0], (0.5 - ndc.y * 0.5) * size[1]], clip.w))
}

/// Draws `poses` on the HUD layer, through walls. `view_proj` is the
/// camera's, `size` the HUD's in CSS pixels, `pixels_per_unit` the
/// projection's scale (`height / 2 / tan(fov / 2)`): a world length at view
/// depth `w` covers `length * pixels_per_unit / w` pixels.
pub fn draw_markers(
    hud: &mut Hud2d,
    fonts: &Fonts,
    poses: &[MarkerPose],
    view_proj: Mat4,
    size: [f32; 2],
    pixels_per_unit: f32,
) {
    let gold = hex(MARKER_GOLD_CSS, 1.0);
    let ring_color = hex(MARKER_GOLD_CSS, RING_ALPHA);
    for pose in poses {
        let Some((center, depth)) = project(view_proj, size, pose.center) else {
            continue;
        };
        let px = |length: f64| length as f32 * pixels_per_unit / depth;
        // The ring: the torus's centre line, tilted about X, turned about Y.
        let radius = MARKER_SIZE * 1.18 / 2.0;
        let (tilt_sin, tilt_cos) = RING_TILT.sin_cos();
        let (spin_sin, spin_cos) = pose.spin.sin_cos();
        let ring: Vec<Option<([f32; 2], f32)>> = (0..=RING_SEGMENTS)
            .map(|i| {
                let a = i as f64 / RING_SEGMENTS as f64 * std::f64::consts::TAU;
                let (x, z) = (radius * a.cos(), radius * a.sin());
                // Rotation about X by the tilt, then about Y by the spin
                // (Babylon's left-handed rotations).
                let (y, z) = (-z * tilt_sin, z * tilt_cos);
                let (x, z) = (x * spin_cos + z * spin_sin, -x * spin_sin + z * spin_cos);
                project(view_proj, size, pose.center + DVec3::new(x, y, z))
            })
            .collect();
        for pair in ring.windows(2) {
            let (Some((a, da)), Some((b, db))) = (pair[0], pair[1]) else {
                continue;
            };
            let half = |d: f32| (RING_THICKNESS as f32 * pixels_per_unit / d / 2.0).max(0.5);
            let dir = Vec3::new(b[0] - a[0], b[1] - a[1], 0.0);
            let len = dir.length();
            if len <= f32::EPSILON {
                continue;
            }
            let n = [-dir.y / len, dir.x / len];
            let (ha, hb) = (half(da), half(db));
            hud.polygon(
                &[
                    [a[0] + n[0] * ha, a[1] + n[1] * ha],
                    [b[0] + n[0] * hb, b[1] + n[1] * hb],
                    [b[0] - n[0] * hb, b[1] - n[1] * hb],
                    [a[0] - n[0] * ha, a[1] - n[1] * ha],
                ],
                &[ring_color; 4],
            );
        }
        // The plate: a pointy-top hexagon framing the letter.
        let plate = px(MARKER_SIZE);
        let hex_radius = plate * 0.43;
        let corners: Vec<[f32; 2]> = (0..6)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_3 * i as f32 - std::f32::consts::FRAC_PI_2;
                [center[0] + a.cos() * hex_radius, center[1] + a.sin() * hex_radius]
            })
            .collect();
        hud.polygon(&corners, &[GLYPH_BACKING; 6]);
        let stroke = plate * 0.035 / 2.0;
        for i in 0..6 {
            let (a, b) = (corners[i], corners[(i + 1) % 6]);
            let outward = |p: [f32; 2], k: f32| {
                let (dx, dy) = (p[0] - center[0], p[1] - center[1]);
                let l = dx.hypot(dy).max(f32::EPSILON);
                [p[0] + dx / l * k, p[1] + dy / l * k]
            };
            hud.polygon(
                &[
                    outward(a, stroke),
                    outward(b, stroke),
                    outward(b, -stroke),
                    outward(a, -stroke),
                ],
                &[gold; 4],
            );
        }
        // `drawText` at 68 % down the plate, bold, half its size.
        let style = TextStyle::new(fonts.body, plate * 0.5, hex(GLYPH_CSS, 1.0)).align(Align::Center);
        hud.text(center[0], center[1] - plate / 2.0 + plate * 0.68, &pose.label, &style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger::shared::json::parse;

    fn json(text: &str) -> Value {
        parse(text).unwrap()
    }

    #[test]
    fn signature_names_round_and_points() {
        let e = json(r#"{"points": [{"x": 1, "y": 2, "z": 3, "radius": 400, "delay": 0.6}]}"#);
        assert_eq!(
            bomb_explosion_signature(3, Some(&e)).as_deref(),
            Some("3:1,2,3,400,0.6")
        );
        assert_eq!(bomb_explosion_signature(3, Some(&json(r#"{"points": []}"#))), None);
        assert_eq!(bomb_explosion_signature(3, None), None);
    }

    #[test]
    fn plays_a_repeated_schedule_once_and_throttles_sound() {
        let mut markers = BombSiteMarkers::default();
        let e = json(
            r#"{"points": [
            {"x": 0, "y": 0, "z": 0, "radius": 400, "delay": 0.0},
            {"x": 1, "y": 0, "z": 0, "radius": 400, "delay": 0.0},
            {"x": 2, "y": 0, "z": 0, "radius": 400, "delay": 0.5}
        ]}"#,
        );
        markers.schedule_explosion(1, Some(&e));
        let first = markers.fire_pending();
        assert_eq!(first.len(), 2);
        assert!(first[0].sound && !first[1].sound);
        // The repeat is ignored.
        markers.schedule_explosion(1, Some(&e));
        assert!(markers.fire_pending().is_empty());
        markers.elapsed += 0.5;
        let late = markers.fire_pending();
        assert_eq!(late.len(), 1);
        assert!(late[0].sound);
        // The gap re-arms it.
        markers.schedule_explosion(1, None);
        markers.schedule_explosion(1, Some(&e));
        assert_eq!(markers.fire_pending().len(), 2);
    }

    #[test]
    fn reads_sites() {
        let bomb = json(r#"{"kind": "bomb", "sites": [{"id": 0, "label": "A", "x": 1, "y": 2, "z": 3}]}"#);
        let sites = bomb_sites(Some(&bomb));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].label, "A");
        assert_eq!(sites[0].position, DVec3::new(1.0, 2.0, 3.0));
        assert!(bomb_sites(None).is_empty());
    }
}
