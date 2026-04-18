use super::workspace::{maybe_bootstrap_workspace, warn_rebuild_if_running};
use crate::cli::MountCommands;
use crate::config::{
    config_path, gitconfig_target, load_bulkhead_config, load_bulkhead_document,
    remove_path_mount_in_document, resolve_mount_access, upsert_path_mount_in_document,
    workspace_path,
};
use crate::devcontainer::{ensure_workspace_layout, persist_bulkhead_document};
use anyhow::{Result, bail};
use std::path::PathBuf;

pub(crate) fn mount(command: MountCommands) -> Result<()> {
    match command {
        MountCommands::Add {
            source,
            target,
            access,
            rw,
            workspace,
        } => mount_add(
            workspace.workspace,
            source,
            target,
            resolve_mount_access(access, rw),
        ),
        MountCommands::Remove { path, workspace } => mount_remove(workspace.workspace, path),
        MountCommands::List { workspace } => mount_list(workspace.workspace),
    }
}

fn mount_add(
    workspace: Option<PathBuf>,
    source: String,
    target: String,
    access: crate::config::MountAccess,
) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    maybe_bootstrap_workspace(&workspace)?;
    ensure_workspace_layout(&workspace)?;

    let mut doc = load_bulkhead_document(&workspace)?;
    upsert_path_mount_in_document(&mut doc, &source, &target, access)?;
    persist_bulkhead_document(&workspace, doc)?;

    println!(
        "Added mount {} -> {} ({}) in {}",
        source,
        target,
        access.as_str(),
        config_path(&workspace).display()
    );

    warn_rebuild_if_running(&workspace, "mount changes")
}

fn mount_remove(workspace: Option<PathBuf>, path: String) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    ensure_workspace_layout(&workspace)?;

    let mut doc = load_bulkhead_document(&workspace)?;
    if !remove_path_mount_in_document(&mut doc, &path)? {
        bail!("no mount found matching `{}`", path);
    }

    persist_bulkhead_document(&workspace, doc)?;

    println!(
        "Removed mount `{}` from {}",
        path,
        config_path(&workspace).display()
    );

    warn_rebuild_if_running(&workspace, "mount changes")
}

fn mount_list(workspace: Option<PathBuf>) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    ensure_workspace_layout(&workspace)?;
    let config = load_bulkhead_config(&workspace)?;

    println!("Mounts in {}:", config_path(&workspace).display());

    if config.git.enabled {
        println!(
            "  ~/.gitconfig -> {} (ro, managed)",
            gitconfig_target(&config.remote_user)
        );
    }

    if config.path.is_empty() {
        if !config.git.enabled {
            println!("  No extra host path mounts configured.");
        }
        return Ok(());
    }

    for mount in &config.path {
        println!(
            "  {} -> {} ({})",
            mount.source,
            mount.target,
            mount.access.as_str()
        );
    }

    Ok(())
}
