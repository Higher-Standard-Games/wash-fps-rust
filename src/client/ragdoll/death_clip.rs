//! wash-fps `client/ragdoll/DeathClip.ts`: the clip a body plays while it is
//! thrown, before the ragdoll takes over.

use shagger::shared::js::js_hypot;

/// A push within 60 degrees of the victim's facing came from behind
/// (`BEHIND_COS`, `Math.cos(Math.PI / 3)` as V8 gives it).
pub const BEHIND_COS: f64 = 0.5000000000000001;
/// Below this horizontal share the push is straight up and has no side
/// (`MIN_HORIZONTAL`).
pub const MIN_HORIZONTAL: f64 = 0.1;

/// `DeathClip`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathClip {
    FlyingBack,
    FromBack,
    Headshot,
}

impl DeathClip {
    /// The clip's name in the player animations.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FlyingBack => "death_flying_back",
            Self::FromBack => "death_from_back",
            Self::Headshot => "death_headshot",
        }
    }
}

/// `DeathFacts`: what a death event says about the killing blow.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DeathFacts<'a> {
    pub damage_type: &'a str,
    pub kill_weapon: Option<&'a str>,
    pub hit_location: Option<&'a str>,
    pub kb_dir_x: f64,
    pub kb_dir_z: f64,
}

/// `chooseDeathClip`: a kill from behind plays `death_from_back` whatever
/// dealt it; from the front or side a headshot plays `death_headshot`, and a
/// riotgun blast or an explosion plays `death_flying_back`; anything else
/// goes limp at once.
#[must_use]
pub fn choose_death_clip(event: &DeathFacts<'_>, victim_yaw: f64) -> Option<DeathClip> {
    let kb_x = event.kb_dir_x;
    let kb_z = event.kb_dir_z;
    let horizontal = js_hypot(&[kb_x, kb_z]);
    if horizontal < MIN_HORIZONTAL {
        return None;
    }
    let along = (victim_yaw.sin() * kb_x + victim_yaw.cos() * kb_z) / horizontal;
    if along >= BEHIND_COS {
        return Some(DeathClip::FromBack);
    }
    if event.hit_location == Some("head") {
        return Some(DeathClip::Headshot);
    }
    if event.kill_weapon == Some("riotgun") || event.damage_type == "explosion" {
        return Some(DeathClip::FlyingBack);
    }
    None
}

/// `deathClipYaw`: facing the shot for a blow from the front, so the push
/// throws it straight back, and away from it for one from behind.
#[must_use]
pub fn death_clip_yaw(clip: DeathClip, kb_dir_x: f64, kb_dir_z: f64) -> f64 {
    if clip == DeathClip::FromBack {
        kb_dir_x.atan2(kb_dir_z)
    } else {
        (-kb_dir_x).atan2(-kb_dir_z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(kb_x: f64, kb_z: f64) -> DeathFacts<'static> {
        DeathFacts {
            kb_dir_x: kb_x,
            kb_dir_z: kb_z,
            ..DeathFacts::default()
        }
    }

    #[test]
    fn behind_beats_everything() {
        // Facing +Z, pushed along +Z: the shot came from behind.
        let mut f = facts(0.0, 1.0);
        f.hit_location = Some("head");
        assert_eq!(choose_death_clip(&f, 0.0), Some(DeathClip::FromBack));
        // 50 degrees off still counts; 70 does not.
        let near = 50_f64.to_radians();
        assert_eq!(
            choose_death_clip(&facts(near.sin(), near.cos()), 0.0),
            Some(DeathClip::FromBack)
        );
        let far = 70_f64.to_radians();
        assert_eq!(choose_death_clip(&facts(far.sin(), far.cos()), 0.0), None);
    }

    #[test]
    fn from_the_front() {
        let mut f = facts(0.0, -1.0);
        assert_eq!(choose_death_clip(&f, 0.0), None);
        f.kill_weapon = Some("riotgun");
        assert_eq!(choose_death_clip(&f, 0.0), Some(DeathClip::FlyingBack));
        f.hit_location = Some("head");
        assert_eq!(choose_death_clip(&f, 0.0), Some(DeathClip::Headshot));
        let blast = DeathFacts {
            damage_type: "explosion",
            ..facts(1.0, 0.0)
        };
        assert_eq!(choose_death_clip(&blast, 0.0), Some(DeathClip::FlyingBack));
        // Straight up has no side.
        assert_eq!(
            choose_death_clip(
                &DeathFacts {
                    damage_type: "explosion",
                    ..facts(0.05, 0.05)
                },
                0.0
            ),
            None
        );
    }

    #[test]
    fn the_body_turns_to_the_shot() {
        // Pushed along +X from the front: it faces -X.
        let yaw = death_clip_yaw(DeathClip::FlyingBack, 1.0, 0.0);
        assert!((yaw + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((death_clip_yaw(DeathClip::FromBack, 1.0, 0.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }
}
