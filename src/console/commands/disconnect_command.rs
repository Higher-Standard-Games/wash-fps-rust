//! wash-fps `console/commands/DisconnectCommand.ts`: leaves the game server
//! for the lobby (here, the match window closes and the menu returns).

use shagger::console::command::{Command, CommandDef, CommandOutput};
use shagger::console::command_registry::CommandRegistry;

use crate::console::{ConsoleContext, ConsoleRequest};

pub struct DisconnectCommand(CommandDef);

impl Default for DisconnectCommand {
    fn default() -> Self {
        Self(CommandDef {
            name: "disconnect",
            description: "Disconnect from current game server",
            usage: Some("disconnect"),
            ..CommandDef::default()
        })
    }
}

impl Command<ConsoleContext> for DisconnectCommand {
    fn definition(&self) -> &CommandDef {
        &self.0
    }
    fn execute(
        &mut self,
        _: &[String],
        _: &mut CommandRegistry<ConsoleContext>,
        game: &mut ConsoleContext,
    ) -> CommandOutput {
        let Some(server) = game.server.clone() else {
            return "Not connected to any server".into();
        };
        game.requests.push(ConsoleRequest::Disconnect);
        format!("Disconnected from {server}").into()
    }
}
