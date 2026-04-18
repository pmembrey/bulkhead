mod doctor;
mod git_config;
mod mount;
mod workspace;

use crate::cli::{Commands, ConfigCommands};
use anyhow::Result;

pub(crate) fn dispatch(command: Commands) -> Result<()> {
    match command {
        Commands::Template {
            directory,
            force,
            preset,
            wizard,
            yes,
        } => workspace::template(directory, force, preset, wizard, yes),
        Commands::Up { workspace } => workspace::up(workspace.workspace),
        Commands::Rebuild { workspace } => workspace::rebuild(workspace.workspace),
        Commands::Down { workspace } => workspace::down(workspace.workspace),
        Commands::Shell { workspace } => workspace::shell(workspace.workspace),
        Commands::Status { workspace } => workspace::status(workspace.workspace),
        Commands::Logs(args) => workspace::logs(args),
        Commands::Exec { workspace, command } => workspace::exec(workspace.workspace, command),
        Commands::Destroy { workspace, force } => workspace::destroy(workspace.workspace, force),
        Commands::Doctor { fix } => doctor::doctor(fix),
        Commands::Mount(command) => mount::mount(command),
        Commands::Config(command) => match command {
            ConfigCommands::Git { command } => git_config::git_config(command),
        },
    }
}
