use crate::config::{MountAccess, Preset};
use clap::{ArgAction, Args, Parser, Subcommand};
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "bulkhead",
    version,
    about = "Manage a Bulkhead devcontainer from a small Rust CLI."
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    #[command(
        visible_alias = "init",
        about = "Create Bulkhead workspace files in a directory"
    )]
    Template {
        directory: Option<PathBuf>,
        #[arg(short, long, action = ArgAction::SetTrue)]
        force: bool,
        #[arg(long, value_enum)]
        preset: Option<Preset>,
        #[arg(long, action = ArgAction::SetTrue)]
        wizard: bool,
        #[arg(short = 'y', long, action = ArgAction::SetTrue)]
        yes: bool,
    },
    #[command(
        visible_alias = "start",
        about = "Start or ensure the Bulkhead container is running"
    )]
    Up {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(about = "Rebuild the Bulkhead container from the current config")]
    Rebuild {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(visible_alias = "stop", about = "Stop the running Bulkhead container")]
    Down {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(about = "Open an interactive shell in the Bulkhead container")]
    Shell {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(about = "Show the current Bulkhead workspace and container status")]
    Status {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(about = "Show Docker logs from the Bulkhead container")]
    Logs(LogsArgs),
    Exec {
        #[command(flatten)]
        workspace: WorkspaceArgs,
        #[arg(
            required = true,
            num_args = 1..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        command: Vec<OsString>,
    },
    #[command(about = "Remove the container and its Bulkhead-managed resources")]
    Destroy {
        #[command(flatten)]
        workspace: WorkspaceArgs,
        #[arg(short, long, action = ArgAction::SetTrue)]
        force: bool,
    },
    Doctor {
        #[arg(long, action = ArgAction::SetTrue)]
        fix: bool,
    },
    #[command(subcommand)]
    Mount(MountCommands),
    #[command(subcommand)]
    Config(ConfigCommands),
}

#[derive(Args, Debug, Clone)]
pub(crate) struct WorkspaceArgs {
    #[arg(short, long)]
    pub(crate) workspace: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct LogsArgs {
    #[command(flatten)]
    pub(crate) workspace: WorkspaceArgs,
    #[arg(short = 'f', long, action = ArgAction::SetTrue)]
    pub(crate) follow: bool,
    #[arg(long, default_value_t = 100)]
    pub(crate) tail: usize,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ConfigCommands {
    #[command(
        visible_alias = "gitconfig",
        about = "Manage the host .gitconfig feature"
    )]
    Git {
        #[command(subcommand)]
        command: GitConfigCommands,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum MountCommands {
    #[command(about = "Add or update a host path mount")]
    Add {
        source: String,
        target: String,
        #[arg(long, value_enum, group = "mount_access")]
        access: Option<MountAccess>,
        #[arg(long, action = ArgAction::SetTrue, group = "mount_access")]
        rw: bool,
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(about = "Remove a host path mount by source or target")]
    Remove {
        path: String,
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(about = "List configured host path mounts")]
    List {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum GitConfigCommands {
    #[command(
        visible_alias = "on",
        about = "Enable the managed host .gitconfig mount"
    )]
    Enable {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(
        visible_alias = "off",
        about = "Disable the managed host .gitconfig mount"
    )]
    Disable {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
    #[command(about = "Show whether the managed host .gitconfig mount is enabled")]
    Status {
        #[command(flatten)]
        workspace: WorkspaceArgs,
    },
}
