//! Liquid state as the simulation sees it (wash-fps `liquid/LiquidTypes.ts`).

/// `LiquidKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidKind {
    Water,
    Slime,
    Lava,
}

impl LiquidKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Water => "water",
            Self::Slime => "slime",
            Self::Lava => "lava",
        }
    }
}

/// `WaterLevel`: 0 dry, 1 feet wet, 2 waist deep (swimming), 3 submerged.
pub type WaterLevel = u8;

/// `LiquidSample`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiquidSample {
    pub level: WaterLevel,
    /// The dominant liquid at the feet, or none when dry.
    pub kind: Option<LiquidKind>,
    /// Contents at the foot probe, Q3's `watertype`.
    pub raw_contents: u32,
    pub foot_contents: u32,
    pub waist_contents: u32,
    pub eye_contents: u32,
}

/// `DRY_SAMPLE`.
pub const DRY_SAMPLE: LiquidSample = LiquidSample {
    level: 0,
    kind: None,
    raw_contents: 0,
    foot_contents: 0,
    waist_contents: 0,
    eye_contents: 0,
};

/// `LiquidTransitions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidTransitions {
    pub entered: bool,
    pub exited: bool,
    pub head_under: bool,
    pub head_clear: bool,
}

/// `liquidTransitions`.
#[must_use]
pub fn liquid_transitions(previous: &LiquidSample, current: &LiquidSample) -> LiquidTransitions {
    let (was_in, is_in) = (previous.level > 0, current.level > 0);
    let (was_under, is_under) = (previous.level == 3, current.level == 3);
    LiquidTransitions {
        entered: !was_in && is_in,
        exited: was_in && !is_in,
        head_under: !was_under && is_under,
        head_clear: was_under && !is_under,
    }
}

/// `isSwimming`: levels 2 and 3 swim; level 1 wades.
#[must_use]
pub fn is_swimming(level: WaterLevel) -> bool {
    level >= 2
}

/// `fallDamageScale`.
#[must_use]
pub fn fall_damage_scale(level: WaterLevel) -> f64 {
    match level {
        0 => 1.0,
        1 => 0.5,
        2 => 0.25,
        _ => 0.0,
    }
}
