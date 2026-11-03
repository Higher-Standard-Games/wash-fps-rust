//! Part of wash-fps `client/DemoService.ts`: `analyzeLoadedDemo`'s report
//! of a demo's highlights, as `demoplay analyze` prints it. The rest of the
//! service (loading, the viewer, the reel) is the window's and
//! [`crate::client::cinematic_demo_player`]'s here.

use shagger::shared::demo::demo_analyzer::DemoAnalysis;
use shagger::shared::js::js_round;

/// `Number.prototype.toFixed(1)` for a score: Rust's formatting rounds an
/// exact tie to even, JavaScript's up.
fn to_fixed_1(x: f64) -> String {
    let tenths = x * 10.0;
    if tenths.is_finite() && (tenths - tenths.floor() - 0.5).abs() == 0.0 && x * 20.0 == (x * 20.0).floor() {
        return format!("{:.1}", (tenths.floor() + 1.0) / 10.0);
    }
    format!("{x:.1}")
}

/// `padStart` and `padEnd` count UTF-16 units; names are padded by chars,
/// which is the same for every name a lobby lets through but the rare
/// astral emoji.
fn pad_end(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

/// `analyzeLoadedDemo`'s text: the map and length, then the first 20
/// highlights by tick, score, type, player and tags.
#[must_use]
pub fn analysis_report(analysis: &DemoAnalysis, map: &str, duration_ms: f64) -> String {
    let highlights = &analysis.highlights;
    if highlights.is_empty() {
        return "No highlights found in this demo.".to_owned();
    }
    let duration_sec = js_round(duration_ms / 1000.0) as i64;
    let (mm, ss) = (duration_sec.div_euclid(60), duration_sec.rem_euclid(60));
    let mut lines = vec![
        format!("HIGHLIGHT ANALYSIS — {map} ({mm}:{ss:02})"),
        format!("Found {} highlights", highlights.len()),
        String::new(),
        "#    TICK   SCORE  TYPE              PLAYER             TAGS".to_owned(),
        "─".repeat(72),
    ];
    for (i, h) in highlights.iter().take(20).enumerate() {
        let tags: Vec<&str> = h.tags.iter().map(|t| t.as_str()).collect();
        let tags = if tags.is_empty() {
            "—".to_owned()
        } else {
            tags.join(", ")
        };
        lines.push(format!(
            "{:>3}  {:>6}  {:>6}  {}  {}  {tags}",
            i + 1,
            h.peak_tick,
            to_fixed_1(h.score),
            pad_end(h.kind.as_str(), 17),
            pad_end(&h.primary_player_name, 18),
        ));
    }
    if highlights.len() > 20 {
        lines.push(format!("  … and {} more", highlights.len() - 20));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_round_ties_up_as_to_fixed_does() {
        assert_eq!(to_fixed_1(0.25), "0.3");
        assert_eq!(to_fixed_1(2.75), "2.8");
        assert_eq!(to_fixed_1(1.04), "1.0");
        assert_eq!(to_fixed_1(12.0), "12.0");
    }

    #[test]
    fn an_empty_demo_says_so() {
        let analysis = DemoAnalysis {
            highlights: Vec::new(),
            total_ticks: 0,
            total_duration_ms: 0.0,
            player_names: Default::default(),
            frags: Vec::new(),
        };
        assert_eq!(
            analysis_report(&analysis, "washoi_dm14", 61_000.0),
            "No highlights found in this demo."
        );
    }

    /// Every recorded demo's report: its heading, then a row a highlight.
    #[test]
    fn recorded_demos_list_their_highlights() {
        use crate::client::cinematic_demo_player::{NoWorld, analyze_demo};
        use crate::client::demo_playback::DemoPlayback;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/demos");
        let mut listed = 0;
        for file in std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "shagdemo"))
        {
            let demo = DemoPlayback::load(&file).unwrap();
            let analysis = analyze_demo(&demo, &mut NoWorld);
            let report = analysis_report(&analysis, &demo.header.map, demo.duration_ms);
            if analysis.highlights.is_empty() {
                continue;
            }
            listed += 1;
            let lines: Vec<&str> = report.lines().collect();
            assert!(lines[0].starts_with(&format!("HIGHLIGHT ANALYSIS — {} (", demo.header.map)));
            assert_eq!(lines[1], format!("Found {} highlights", analysis.highlights.len()));
            let rows = analysis.highlights.len().min(20);
            assert!(lines[5].starts_with("  1  "), "{report}");
            assert_eq!(lines.len(), 5 + rows + usize::from(analysis.highlights.len() > 20));
        }
        assert!(listed > 0);
    }
}
