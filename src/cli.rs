use crate::config::{MountAccess, Preset};
use clap::{ArgAction, Args, Parser, Subcommand};
use std::ffi::OsString;
use std::path::PathBuf;

const CLONE_LONG_ABOUT: &str = "\
Manage Bulkhead-owned isolated Git clones under `.bulkhead/clones/`.

Use `bulkhead clone shell <name>` for the normal day-to-day flow:
- jump back into an existing managed clone by name
- or create it when missing and then open the Bulkhead shell there

This mode keeps the source checkout and source repository metadata out of the
container. If you want Git worktrees too, create them inside the isolated
clone rather than against the original repository.";

const CLONE_AFTER_LONG_HELP: &str = "\
Examples:
  bulkhead clone list
  bulkhead clone shell feature-x
  bulkhead clone shell feature-x --create
  bulkhead clone shell feature-x --create --base origin/main";

const CLONE_SHELL_LONG_ABOUT: &str = "\
Open a Bulkhead shell in a Bulkhead-managed isolated clone.

If the named clone already exists under `.bulkhead/clones/`, Bulkhead opens the
normal shell flow there.

If it does not exist:
- interactive terminals prompt before creation
- non-interactive runs require `--create`

When Bulkhead creates a managed clone from this command, it:
- creates an independent local clone with `git clone --no-local --no-hardlinks`
- uses the clone name as the default branch name unless `--detach` is used
- bootstraps Bulkhead files when safe
- then opens the shell there";

const CLONE_SHELL_AFTER_LONG_HELP: &str = "\
Examples:
  bulkhead clone shell feature-x
  bulkhead clone shell feature-x --create
  bulkhead clone shell feature-x --create --base origin/main
  bulkhead clone shell review-fix --create --branch fix/review
  bulkhead clone shell scratch --create --detach

Tip:
  Use `bulkhead clone list` first if you are not sure what managed clones
  already exist for the current repository.";

const CLONE_LIST_LONG_ABOUT: &str = "\
List the Bulkhead-managed isolated clones for the current repository.

Each clone lives under `.bulkhead/clones/` in the current repository root.";

const CLONE_LIST_AFTER_LONG_HELP: &str = "\
Examples:
  bulkhead clone list
  bulkhead clone shell feature-x";

const CLONE_REMOVE_LONG_ABOUT: &str = "\
Delete a Bulkhead-managed isolated clone.

This removes only `.bulkhead/clones/<name>` for the current repository. It does
not touch the source checkout or any other managed clones.";

const CLONE_REMOVE_AFTER_LONG_HELP: &str = "\
Examples:
  bulkhead clone remove feature-x
  bulkhead clone remove feature-x --force";

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
    #[command(
        subcommand,
        about = "Manage Bulkhead-owned isolated Git clones",
        long_about = CLONE_LONG_ABOUT,
        after_long_help = CLONE_AFTER_LONG_HELP
    )]
    Clone(CloneCommands),
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
pub(crate) enum CloneCommands {
    #[command(
        about = "Open a Bulkhead shell in a managed isolated clone",
        long_about = CLONE_SHELL_LONG_ABOUT,
        after_long_help = CLONE_SHELL_AFTER_LONG_HELP
    )]
    Shell {
        /// Managed clone name under `.bulkhead/clones/`
        name: String,
        #[arg(long, action = ArgAction::SetTrue)]
        /// Create the clone immediately if it does not exist
        create: bool,
        #[arg(long)]
        /// Base commit or branch to check out after cloning
        base: Option<String>,
        #[arg(short = 'b', long)]
        /// Branch name to create in the new clone
        branch: Option<String>,
        #[arg(short = 'd', long, action = ArgAction::SetTrue, conflicts_with = "branch")]
        /// Leave the new clone in detached HEAD mode
        detach: bool,
        #[arg(long, action = ArgAction::SetTrue)]
        /// Allow creating a clone even when the source repository has uncommitted changes
        allow_dirty_source: bool,
        #[arg(long, action = ArgAction::SetTrue)]
        /// Skip installing Bulkhead files if the clone does not already have them
        no_template: bool,
        #[arg(long, value_enum)]
        /// Bulkhead template preset to install when bootstrapping
        preset: Option<Preset>,
        #[arg(long, action = ArgAction::SetTrue)]
        /// Prompt for the Bulkhead template preset interactively
        wizard: bool,
        #[arg(short = 'y', long, action = ArgAction::SetTrue)]
        /// Accept the default Bulkhead template preset without prompting
        yes: bool,
    },
    #[command(
        visible_alias = "ls",
        about = "List Bulkhead-managed isolated clones",
        long_about = CLONE_LIST_LONG_ABOUT,
        after_long_help = CLONE_LIST_AFTER_LONG_HELP
    )]
    List,
    #[command(
        about = "Remove a Bulkhead-managed isolated clone",
        long_about = CLONE_REMOVE_LONG_ABOUT,
        after_long_help = CLONE_REMOVE_AFTER_LONG_HELP
    )]
    Remove {
        /// Managed clone name under `.bulkhead/clones/`
        name: String,
        #[arg(short, long, action = ArgAction::SetTrue)]
        /// Remove the clone without a confirmation prompt
        force: bool,
    },
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
