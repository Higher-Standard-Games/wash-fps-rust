//! Port of wash-fps `liquid/client/SplashDroplets.ts`: a splash's crown of
//! spray, pooled and deterministic per splash seed. A droplet that falls
//! back through the surface is handed back as a re-entry, so the crown can
//! ring the water where it lands.

use shagger::shared::js::{js_cos, js_round, js_sin};

/// `MAX_DROPLETS`.
pub const MAX_DROPLETS: usize = 220;
const DROPLET_GRAVITY: f64 = 1400.0;
const DROPLET_DRAG: f64 = 1.6;
const MAX_LIFETIME: f64 = 2.5;
const MIN_SIZE: f64 = 0.3;
const MAX_SIZE: f64 = 0.85;

/// `DropletSpawn`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropletSpawn {
    pub x: f64,
    pub z: f64,
    pub surface_y: f64,
    pub strength: f64,
}

/// `DropletReentry`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropletReentry {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub speed: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Droplet {
    x: f64,
    y: f64,
    z: f64,
    vx: f64,
    vy: f64,
    launch_speed: f64,
    vz: f64,
    surface_y: f64,
    age: f64,
    size: f64,
    alive: bool,
}

/// `seededUnit`: the shader-style hash, in V8's sine.
fn seeded_unit(seed: f64, index: f64) -> f64 {
    let h = js_sin(seed * 12.9898 + index * 78.233) * 43758.5453;
    h - h.floor()
}

/// One live droplet as the renderer takes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropletSprite {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub size: f64,
    pub opacity: f64,
}

/// `SplashDroplets`.
#[derive(Debug, Clone)]
pub struct SplashDroplets {
    droplets: Vec<Droplet>,
    live: Vec<DropletSprite>,
}

impl Default for SplashDroplets {
    fn default() -> Self {
        Self {
            droplets: vec![
                Droplet {
                    launch_speed: 1.0,
                    size: 1.0,
                    ..Droplet::default()
                };
                MAX_DROPLETS
            ],
            live: Vec::with_capacity(MAX_DROPLETS),
        }
    }
}

impl SplashDroplets {
    /// `spawn`: a crown sized by the splash's strength, spread by `seed`.
    pub fn spawn(&mut self, spawn: DropletSpawn, seed: f64) -> usize {
        if ![spawn.x, spawn.surface_y, spawn.z, spawn.strength, seed]
            .iter()
            .all(|v| v.is_finite())
            || spawn.strength <= 0.0
        {
            return 0;
        }
        let strength = spawn.strength.min(1.0);
        let wanted = js_round(3.0 + strength * 21.0) as usize;
        let mut spawned = 0;
        for i in 0..wanted {
            let Some(droplet) = self.droplets.iter_mut().find(|d| !d.alive) else {
                break;
            };
            let i = i as f64;
            let angle = seeded_unit(seed, i) * std::f64::consts::PI * 2.0;
            let spread = 20.0 + strength * (20.0 + seeded_unit(seed, i + 500.0) * 100.0);
            let lift = 80.0 + strength * (100.0 + seeded_unit(seed, i + 1000.0) * 240.0);
            let roll = seeded_unit(seed, i + 1500.0);
            *droplet = Droplet {
                x: spawn.x,
                y: spawn.surface_y,
                z: spawn.z,
                vx: js_cos(angle) * spread,
                vy: lift,
                launch_speed: lift,
                vz: js_sin(angle) * spread,
                surface_y: spawn.surface_y,
                age: 0.0,
                size: MIN_SIZE + (MAX_SIZE - MIN_SIZE) * roll * roll * roll,
                alive: true,
            };
            spawned += 1;
        }
        spawned
    }

    /// `update`: flight under drag and gravity; the ones that came back
    /// down through their surface.
    pub fn update(&mut self, delta_time: f64) -> Vec<DropletReentry> {
        let mut reentries = Vec::new();
        self.live.clear();
        for d in &mut self.droplets {
            if !d.alive {
                continue;
            }
            d.age += delta_time;
            let drag = (1.0 - DROPLET_DRAG * delta_time).max(0.0);
            d.vx *= drag;
            d.vz *= drag;
            d.vy = d.vy * drag - DROPLET_GRAVITY * delta_time;
            d.x += d.vx * delta_time;
            d.y += d.vy * delta_time;
            d.z += d.vz * delta_time;
            if d.vy < 0.0 && d.y <= d.surface_y {
                d.alive = false;
                reentries.push(DropletReentry {
                    x: d.x,
                    y: d.surface_y,
                    z: d.z,
                    speed: d.vy.abs(),
                });
                continue;
            }
            if d.age >= MAX_LIFETIME {
                d.alive = false;
                continue;
            }
            let falling = if d.vy < 0.0 {
                ((d.y - d.surface_y) / 12.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let descent = (-d.vy / d.launch_speed).clamp(0.0, 1.0);
            self.live.push(DropletSprite {
                position: [d.x, d.y, d.z],
                velocity: [d.vx, d.vy, d.vz],
                size: d.size * (1.0 - descent * 0.65),
                opacity: (d.age / 0.035).min(1.0) * falling,
            });
        }
        reentries
    }

    /// The droplets drawn this frame (`buffer` and friends).
    #[must_use]
    pub fn sprites(&self) -> &[DropletSprite] {
        &self.live
    }

    /// `count`.
    #[must_use]
    pub fn count(&self) -> usize {
        self.live.len()
    }

    /// `aliveCount`.
    #[must_use]
    pub fn alive_count(&self) -> usize {
        self.droplets.iter().filter(|d| d.alive).count()
    }

    pub fn clear(&mut self) {
        for d in &mut self.droplets {
            d.alive = false;
        }
        self.live.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(strength: f64) -> DropletSpawn {
        DropletSpawn {
            x: 0.0,
            z: 0.0,
            surface_y: 0.0,
            strength,
        }
    }

    #[test]
    fn crowns_scale_rise_and_come_back_down() {
        let mut d = SplashDroplets::default();
        assert_eq!(d.spawn(spawn(0.0), 1.0), 0);
        assert_eq!(d.spawn(spawn(f64::NAN), 1.0), 0);
        assert_eq!(d.spawn(spawn(1.0), 1.0), 24);
        assert_eq!(d.spawn(spawn(0.1), 2.0), 5);
        d.update(1.0 / 60.0);
        assert_eq!(d.count(), 29);
        assert!(d.sprites().iter().all(|s| s.position[1] > 0.0));
        let mut reentries = 0;
        for _ in 0..200 {
            reentries += d.update(1.0 / 60.0).len();
        }
        assert_eq!(reentries, 29);
        assert_eq!(d.alive_count(), 0);
        // Same seed, same crown.
        let (mut a, mut b) = (SplashDroplets::default(), SplashDroplets::default());
        a.spawn(spawn(0.7), 9.0);
        b.spawn(spawn(0.7), 9.0);
        a.update(0.05);
        b.update(0.05);
        assert_eq!(a.sprites(), b.sprites());
        // The pool caps the spray.
        let mut full = SplashDroplets::default();
        let total: usize = (0..20).map(|s| full.spawn(spawn(1.0), f64::from(s))).sum();
        assert_eq!(total, MAX_DROPLETS);
    }
}
