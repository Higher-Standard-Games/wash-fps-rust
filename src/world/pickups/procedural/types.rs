//! wash-fps `world/pickups/procedural/types.ts`: what a procedural pickup
//! recipe holds. A recipe is cosmetic only; amounts and respawns stay with
//! the server.

/// `ProceduralPickupFamily`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProceduralPickupFamily {
    Health,
    Armor,
    Ammo,
    Powerup,
}

/// `RGB`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

/// Shorthand for an [`Rgb`].
#[must_use]
pub const fn rgb(r: f64, g: f64, b: f64) -> Rgb {
    Rgb { r, g, b }
}

/// `collectStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectStyle {
    Collapse,
    Shatter,
}

/// `respawnStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespawnStyle {
    Assemble,
    Materialize,
}

/// `ProceduralPickupRecipe`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralPickupRecipe {
    /// Registry id, e.g. `health_medium` or `armor_ya`.
    pub id: &'static str,
    pub family: ProceduralPickupFamily,
    /// Tier kind, e.g. `small`, `mega`, `shard`, `ra`.
    pub kind: &'static str,
    /// Core energy colour (emissive).
    pub color: Rgb,
    /// Ring, plate and floor accent colour.
    pub accent_color: Rgb,
    /// Size multiplier for all generated geometry.
    pub scale: f64,
    pub ring_count: u32,
    pub plate_count: u32,
    pub mote_count: u32,
    pub shell_pieces: u32,
    pub satellite_count: u32,
    /// Vertical bob amplitude (world units).
    pub bob_amplitude: f64,
    /// Bob frequency (cycles per second).
    pub bob_frequency: f64,
    /// Yaw spin of the cosmetic root (degrees per second).
    pub spin_speed: f64,
    /// Emissive breathing frequency (cycles per second).
    pub pulse_frequency: f64,
    pub figure_eight: bool,
    /// Faint vertical beacon strength, 0 to 1.
    pub beacon_strength: f64,
    pub collect_style: CollectStyle,
    pub respawn_style: RespawnStyle,
    /// Mega, ultra, red armour and the quad: longer transients, a double
    /// beat in the pulse.
    pub high_tier: bool,
    /// Ultra: flicker.
    pub unstable: bool,
    /// Ammo only: the package path of the simpleitem icon, with no
    /// extension.
    pub icon_texture: Option<String>,
}
