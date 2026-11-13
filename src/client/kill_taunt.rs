//! wash-fps `client/KillTaunt.ts`: whether a killer taunts after a kill,
//! and with which clip. The roll is a hash of the death event, so every
//! viewer and every replay of a demo sees the same taunt.

use shagger::animation::player_model_manager::TAUNT_ANIMS;
use shagger::shared::js::js_round;

/// Share of kills after which the killer taunts.
pub const KILL_TAUNT_CHANCE: f64 = 0.3;

/// What a death event says about who died where, by whom (`KillFacts`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KillFacts<'a> {
    pub entity_id: &'a str,
    pub killer_id: &'a str,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// `chooseKillTaunt`: the taunt a killer plays over their upper body after
/// the kill, or `None` (seven kills in ten, and every suicide or world
/// death).
#[must_use]
pub fn choose_kill_taunt(event: &KillFacts<'_>) -> Option<&'static str> {
    choose_kill_taunt_from(event, &TAUNT_ANIMS)
}

/// [`choose_kill_taunt`] among `taunts`.
#[must_use]
pub fn choose_kill_taunt_from<'t>(event: &KillFacts<'_>, taunts: &[&'t str]) -> Option<&'t str> {
    if event.killer_id.is_empty() || event.killer_id == event.entity_id || taunts.is_empty() {
        return None;
    }
    let text = format!(
        "{}|{}|{}|{}|{}",
        event.entity_id,
        event.killer_id,
        js_int(event.x),
        js_int(event.y),
        js_int(event.z)
    );
    let hash = fnv1a(&text);
    if f64::from(hash % 1000) / 1000.0 >= KILL_TAUNT_CHANCE {
        return None;
    }
    Some(taunts[(hash / 1000) as usize % taunts.len()])
}

/// `${Math.round(v || 0)}`: a coordinate rounded as JavaScript prints it
/// (`-0` as `0`, `NaN` as `0`).
fn js_int(v: f64) -> String {
    let v = if v.is_nan() { 0.0 } else { js_round(v) };
    if v == 0.0 { "0".to_owned() } else { format!("{v}") }
}

/// FNV-1a over the text's UTF-16 code units, as `charCodeAt` reads them.
fn fnv1a(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for unit in text.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kill(i: usize) -> (String, String, f64, f64, f64) {
        (format!("e{}", i % 7), format!("k{}", i % 5), i as f64 * 13.7, 24.0, -(i as f64) * 3.1)
    }

    fn facts(k: &(String, String, f64, f64, f64)) -> KillFacts<'_> {
        KillFacts {
            entity_id: &k.0,
            killer_id: &k.1,
            x: k.2,
            y: k.3,
            z: k.4,
        }
    }

    #[test]
    fn taunts_after_about_three_kills_in_ten_spread_over_all_three() {
        let mut counts = std::collections::BTreeMap::new();
        let kills = 6000;
        for i in 0..kills {
            if let Some(taunt) = choose_kill_taunt(&facts(&kill(i))) {
                *counts.entry(taunt).or_insert(0usize) += 1;
            }
        }
        let taunted: usize = counts.values().sum();
        let share = taunted as f64 / kills as f64;
        assert!(share > KILL_TAUNT_CHANCE - 0.03 && share < KILL_TAUNT_CHANCE + 0.03, "{share}");
        assert_eq!(counts.keys().copied().collect::<Vec<_>>(), ["taunt_1", "taunt_2", "taunt_3"]);
        for count in counts.values() {
            assert!(*count as f64 / taunted as f64 > 0.25);
        }
    }

    #[test]
    fn never_taunts_for_a_suicide_or_a_death_with_no_killer() {
        for i in 0..200 {
            let k = kill(i);
            let own = KillFacts {
                killer_id: k.0.as_str(),
                ..facts(&k)
            };
            assert_eq!(choose_kill_taunt(&own), None);
            let world = KillFacts {
                killer_id: "",
                ..facts(&k)
            };
            assert_eq!(choose_kill_taunt(&world), None);
        }
    }

    #[test]
    fn matches_the_browser_kill_for_kill() {
        // `chooseKillTaunt` in V8 over the test's first forty kills.
        let golden = [
            None, None, None, None, None, None, Some("taunt_1"), None, None, None, None, None, Some("taunt_1"), None,
            Some("taunt_1"), None, None, None, None, Some("taunt_2"), None, None, None, None, Some("taunt_3"),
            Some("taunt_3"), None, None, Some("taunt_3"), None, None, None, Some("taunt_1"), None, None, None, None,
            None, None, Some("taunt_2"),
        ];
        for (i, want) in golden.iter().enumerate() {
            assert_eq!(choose_kill_taunt(&facts(&kill(i))), *want, "kill {i}");
        }
    }

    #[test]
    fn hashes_as_javascript_does() {
        // FNV-1a's published vectors, and the coordinates printed as
        // `${Math.round(v)}` prints them.
        assert_eq!(fnv1a(""), 0x811c_9dc5);
        assert_eq!(fnv1a("a"), 0xe40c_292c);
        assert_eq!(fnv1a("foobar"), 0xbf9c_f968);
        assert_eq!(js_int(-0.4), "0");
        assert_eq!(js_int(2.5), "3");
        assert_eq!(js_int(-2.5), "-2");
        assert_eq!(js_int(f64::NAN), "0");
        assert_eq!(js_int(-130.0), "-130");
    }
}
