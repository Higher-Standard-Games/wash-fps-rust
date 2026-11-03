//! wash-fps `console/commands/DemoPlayCommand.ts`: `demoplay` plays the
//! demo from its start, cuts a highlight reel of it, prints its highlights,
//! shows or hides its overlays, or stops it.
//!
//! The TypeScript plays the demo `demo <file>` loaded; here the demo is the
//! one the window plays, and stopping it closes the window as `disconnect`
//! does a match.

use shagger::console::command::{Command, CommandDef, CommandOutput};
use shagger::console::command_registry::CommandRegistry;
use shagger::shared::js::{js_number_to_string, js_parse_float};

use crate::console::{ConsoleContext, ConsoleRequest};

/// `MAX_REEL_MINUTES`: a highlight reel is a short video.
const MAX_REEL_MINUTES: f64 = 3.0;
const USAGE: &str = "demoplay [cinematic <minutes> | hud <on|off|toggle> | analyze | stop]";
/// `DemoService`'s error with nothing loaded.
const NO_DEMO: &str = "No demo loaded. Use `demo <filename>` first.";

pub struct DemoPlayCommand(CommandDef);

impl Default for DemoPlayCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "demoplay",
            description: "Control demo playback (normal, cinematic, HUD, analyze, stop)",
            usage: Some(USAGE),
            ..CommandDef::default()
        })
    }
}

impl Command<ConsoleContext> for DemoPlayCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }

    fn execute(
        &mut self,
        args: &[String],
        _: &mut CommandRegistry<ConsoleContext>,
        game: &mut ConsoleContext,
    ) -> CommandOutput {
        let subcommand = args.first().map(|a| a.to_lowercase());
        match subcommand.as_deref() {
            Some("hud") => {
                let Some(demo) = &mut game.demo else {
                    return "No demo playback is active.".into();
                };
                demo.hud_visible = match args.get(1).map(|a| a.to_lowercase()).as_deref() {
                    Some("on") => true,
                    Some("off") => false,
                    Some("toggle") => !demo.hud_visible,
                    _ => return "Usage: demoplay hud <on|off|toggle>".into(),
                };
                format!("Demo HUD {}.", if demo.hud_visible { "on" } else { "off" }).into()
            }
            Some("stop") => {
                if game.demo.is_none() {
                    return "No demo playback is active.".into();
                }
                game.requests.push(ConsoleRequest::DemoStop);
                "Demo playback stopped.".into()
            }
            Some("analyze") => {
                if game.demo.is_none() {
                    return format!("demoplay analyze: {NO_DEMO}").into();
                }
                // The window prints the report once it has read the demo.
                game.requests.push(ConsoleRequest::DemoAnalyze);
                CommandOutput::None
            }
            Some("cinematic") => {
                let asked = args.get(1).map_or(f64::NAN, |a| js_parse_float(a));
                if asked.is_nan() || asked <= 0.0 {
                    return "Usage: demoplay cinematic <minutes>\nExample: demoplay cinematic 2".into();
                }
                let minutes = asked.min(MAX_REEL_MINUTES);
                if game.demo.is_none() {
                    return format!("demoplay cinematic: {NO_DEMO}").into();
                }
                game.requests.push(ConsoleRequest::DemoCinematic { minutes });
                format!(
                    "Playing cinematic highlight reel ({} min)…",
                    js_number_to_string(minutes)
                )
                .into()
            }
            None => {
                if game.demo.is_none() {
                    return format!("demoplay: {NO_DEMO}").into();
                }
                game.requests.push(ConsoleRequest::DemoPlay);
                "Demo playback started. Use 'demoplay stop' to stop.".into()
            }
            Some(_) => format!("Unknown subcommand '{}'. Usage: {USAGE}", args[0]).into(),
        }
    }

    fn completions(&self, _input: &str, args: &[String]) -> Option<Vec<String>> {
        let words: &[&str] = if args.first().is_some_and(|a| a.eq_ignore_ascii_case("hud")) {
            &["on", "off", "toggle"]
        } else if args.len() <= 1 {
            &["cinematic", "hud", "analyze", "stop"]
        } else {
            &[]
        };
        Some(words.iter().map(|w| (*w).to_owned()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::DemoConsole;

    fn run(game: &mut ConsoleContext, line: &[&str]) -> CommandOutput {
        let args: Vec<String> = line.iter().map(|s| (*s).to_owned()).collect();
        DemoPlayCommand::default().execute(&args, &mut CommandRegistry::new(), game)
    }

    #[test]
    fn without_a_demo_nothing_is_asked() {
        let mut game = ConsoleContext::default();
        assert_eq!(run(&mut game, &["hud", "on"]), "No demo playback is active.".into());
        assert_eq!(run(&mut game, &["stop"]), "No demo playback is active.".into());
        assert_eq!(run(&mut game, &[]), format!("demoplay: {NO_DEMO}").into());
        assert!(game.requests.is_empty());
    }

    #[test]
    fn a_demo_takes_its_subcommands() {
        let mut game = ConsoleContext {
            demo: Some(DemoConsole { hud_visible: true }),
            ..ConsoleContext::default()
        };
        assert_eq!(run(&mut game, &["HUD", "toggle"]), "Demo HUD off.".into());
        assert!(!game.demo.unwrap().hud_visible);
        assert_eq!(
            run(&mut game, &["hud", "sideways"]),
            "Usage: demoplay hud <on|off|toggle>".into()
        );
        assert_eq!(
            run(&mut game, &["cinematic", "9"]),
            "Playing cinematic highlight reel (3 min)…".into()
        );
        assert_eq!(
            run(&mut game, &["cinematic", "1.5min"]),
            "Playing cinematic highlight reel (1.5 min)…".into()
        );
        assert!(matches!(run(&mut game, &["cinematic", "x"]), CommandOutput::Text(t) if t.starts_with("Usage")));
        assert_eq!(run(&mut game, &["analyze"]), CommandOutput::None);
        assert_eq!(run(&mut game, &["stop"]), "Demo playback stopped.".into());
        assert_eq!(
            game.requests,
            [
                ConsoleRequest::DemoCinematic { minutes: 3.0 },
                ConsoleRequest::DemoCinematic { minutes: 1.5 },
                ConsoleRequest::DemoAnalyze,
                ConsoleRequest::DemoStop,
            ]
        );
    }
}
