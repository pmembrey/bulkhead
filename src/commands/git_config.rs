use super::workspace::{maybe_bootstrap_workspace, warn_rebuild_if_running};
use crate::cli::GitConfigCommands;
use crate::config::{
    config_path, gitconfig_target, load_bulkhead_config, load_bulkhead_document, workspace_path,
};
use crate::devcontainer::{ensure_workspace_layout, persist_bulkhead_document};
use anyhow::{Result, bail};

pub(crate) fn git_config(command: GitConfigCommands) -> Result<()> {
    match command {
        GitConfigCommands::Enable { workspace } => set_git_config_mount(workspace.workspace, true),
        GitConfigCommands::Disable { workspace } => {
            set_git_config_mount(workspace.workspace, false)
        }
        GitConfigCommands::Status { workspace } => git_config_status(workspace.workspace),
    }
}

fn set_git_config_mount(workspace: Option<std::path::PathBuf>, enabled: bool) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    maybe_bootstrap_workspace(&workspace)?;
    ensure_workspace_layout(&workspace)?;

    let config = load_bulkhead_config(&workspace)?;
    let target = gitconfig_target(&config.remote_user);

    if enabled && config.path.iter().any(|mount| mount.target == target) {
        bail!(
            "cannot enable managed git config: {} is already used by a generic path mount. Remove that mount first.",
            target
        );
    }

    let mut doc = load_bulkhead_document(&workspace)?;
    doc["git"]["enabled"] = toml_edit::value(enabled);
    persist_bulkhead_document(&workspace, doc)?;

    if enabled {
        println!(
            "Enabled host .gitconfig mount -> {} in {}",
            target,
            config_path(&workspace).display()
        );
    } else {
        println!(
            "Disabled host .gitconfig mount in {}",
            config_path(&workspace).display()
        );
    }

    warn_rebuild_if_running(&workspace, "config changes")
}

fn git_config_status(workspace: Option<std::path::PathBuf>) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    ensure_workspace_layout(&workspace)?;
    let config = load_bulkhead_config(&workspace)?;

    if config.git.enabled {
        println!(
            "Managed git config is enabled: ~/.gitconfig -> {} (ro)",
            gitconfig_target(&config.remote_user)
        );
    } else {
        println!("Managed git config is disabled.");
    }

    Ok(())
}
