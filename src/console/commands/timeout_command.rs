//! wash-fps `console/commands/TimeoutCommand.ts`.

use shagger::console::command::{Command, CommandDef, CommandOutput};
use shagger::console::command_registry::CommandRegistry;

use crate::console::{ConsoleContext, ConsoleRequest};

pub struct TimeoutCommand(CommandDef);

impl Default for TimeoutCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "timeout",
            description: "Pause the match: one timeout per player, up to two minutes (Duel, Amphi, S&D)",
            usage: Some("timeout"),
            ..CommandDef::default()
        })
    }
}

impl Command<ConsoleContext> for TimeoutCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(
        &mut self,
        _: &[String],
        _: &mut CommandRegistry<ConsoleContext>,
        game: &mut ConsoleContext,
    ) -> CommandOutput {
        if game.server.is_none() {
            return "Not connected to a game server".into();
        }
        game.requests.push(ConsoleRequest::CallPause(true));
        "Timeout called.".into()
    }
}
