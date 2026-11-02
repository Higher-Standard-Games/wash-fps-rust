//! wash-fps `console/index.ts`: Washoi's own commands, registered after
//! the engine's (`registerFpsCommands`), and the console as the game holds
//! it: the registry, the drop-down and what the commands ask of the game.

pub mod commands;

use std::path::PathBuf;

use shagger::console::command_registry::CommandRegistry;
use shagger::console::commands::register_engine_commands;
use shagger::debug::debug_console::DebugConsole;
use shagger::ui::egui_layer::egui;

/// What a command asks of the game, which acts on it after the frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleRequest {
    Say { text: String, team: bool },
    Disconnect,
}

/// The game as the commands see it (`Game.getInstance()`).
#[derive(Debug, Default)]
pub struct ConsoleContext {
    pub quit: bool,
    /// The game server, while connected.
    pub server: Option<String>,
    pub requests: Vec<ConsoleRequest>,
}

/// `registerFpsCommands`.
pub fn register_fps_commands(registry: &mut CommandRegistry<ConsoleContext>) {
    registry.register(Box::new(commands::disconnect_command::DisconnectCommand::default()));
    registry.register(Box::new(commands::say_team_command::SayTeamCommand::default()));
}

/// The console, its commands and their context.
pub struct GameConsole {
    pub registry: CommandRegistry<ConsoleContext>,
    pub console: DebugConsole,
    pub ctx: ConsoleContext,
}

impl GameConsole {
    /// Every command; `.cfg` files and the history live in `config_dir`.
    #[must_use]
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        let mut registry = CommandRegistry::new();
        register_engine_commands(&mut registry, config_dir.clone(), |c: &mut ConsoleContext| {
            c.quit = true
        });
        register_fps_commands(&mut registry);
        let history = config_dir.map(|d| d.join("console-history.json"));
        Self {
            registry,
            console: DebugConsole::new(history),
            ctx: ConsoleContext::default(),
        }
    }

    /// Draws it, when open.
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.console.ui(ctx, &mut self.registry, &mut self.ctx);
    }

    /// Runs one line, as the console's input would.
    pub fn run(&mut self, line: &str) {
        self.console.input = line.to_owned();
        self.console.submit(&mut self.registry, &mut self.ctx);
    }
}
