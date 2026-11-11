//! wash-fps `console/commands/TimeinCommand.ts`.

use shagger::console::command::{Command, CommandDef, CommandOutput};
use shagger::console::command_registry::CommandRegistry;

use crate::console::{ConsoleContext, ConsoleRequest};

pub struct TimeinCommand(CommandDef);

impl Default for TimeinCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "timein",
            description: "End a timeout: play resumes after a three-second count",
            usage: Some("timein"),
            ..CommandDef::default()
        })
    }
}

impl Command<ConsoleContext> for TimeinCommand {
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
        game.requests.push(ConsoleRequest::CallPause(false));
        "Time in called.".into()
    }
}
