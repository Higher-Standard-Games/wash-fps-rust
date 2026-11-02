//! wash-fps `console/commands/SayTeamCommand.ts`.

use shagger::console::command::{Command, CommandDef, CommandOutput};
use shagger::console::command_registry::CommandRegistry;

use crate::console::{ConsoleContext, ConsoleRequest};

pub struct SayTeamCommand(CommandDef);

impl Default for SayTeamCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "say_team",
            description: "Send a message to your teammates",
            usage: Some("say_team <message>"),
            ..CommandDef::default()
        })
    }
}

impl Command<ConsoleContext> for SayTeamCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(
        &mut self,
        args: &[String],
        _: &mut CommandRegistry<ConsoleContext>,
        game: &mut ConsoleContext,
    ) -> CommandOutput {
        let text = args.join(" ").trim().to_owned();
        if text.is_empty() {
            return "Usage: say_team <message>".into();
        }
        if game.server.is_none() {
            return "Not connected to a game server".into();
        }
        game.requests.push(ConsoleRequest::Say { text, team: true });
        "Team message submitted.".into()
    }
}
