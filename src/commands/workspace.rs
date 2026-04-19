use crate::cli::LogsArgs;
use crate::config::{
    Preset, config_path, existing_directory, gitconfig_target, instantiate_template,
    load_bulkhead_config, workspace_path,
};
use crate::devcontainer::render_workspace_devcontainer;
use crate::docker::{
    discover_resources, docker_daemon_running, find_container_id, print_destroy_summary,
};
use crate::prompt::{
    confirm, confirm_default_yes, is_interactive_terminal, prompt_preset_selection,
};
use crate::system::{
    command_exists, ensure_command, ensure_devcontainer_cli, ensure_docker_daemon, run_command,
    run_command_allow_failure,
};
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

const DOCKERFILE: &str = include_str!("../../templates/Dockerfile");
const POST_CREATE_SCRIPT: &str = include_str!("../../templates/bulkhead-post-create.sh");

pub(crate) fn template(
    directory: Option<PathBuf>,
    force: bool,
    preset: Option<Preset>,
    wizard: bool,
    yes: bool,
) -> Result<()> {
    let workspace = existing_directory(directory)?;
    let preset = select_template_preset(preset, wizard, yes)?;
    write_workspace_template(&workspace, preset, force)?;

    println!(
        "Bulkhead template installed in {} using the `{}` preset.",
        workspace.display(),
        preset.as_str()
    );
    Ok(())
}

pub(crate) fn maybe_bootstrap_workspace(workspace: &Path) -> Result<()> {
    if config_path(workspace).is_file() {
        return Ok(());
    }

    if !is_interactive_terminal() {
        bail!(
            "missing {}. Run `bulkhead template` in the workspace first.",
            config_path(workspace).display()
        );
    }

    println!("No Bulkhead config found in {}.", workspace.display());
    println!(
        "Bulkhead always mounts the current directory read-write in the container, but it does not expose the rest of your laptop unless you add extra host paths."
    );

    if !confirm_default_yes("Create a Bulkhead workspace here now?")? {
        bail!(
            "missing {}. Run `bulkhead template` in the workspace first.",
            config_path(workspace).display()
        );
    }

    let preset = prompt_preset_selection(Preset::Agent)?;
    write_workspace_template(workspace, preset, false)?;

    println!(
        "Initialized {} with the `{}` preset. Edit `bulkhead.toml` to add more host paths later.",
        workspace.display(),
        preset.as_str()
    );

    Ok(())
}

pub(crate) fn warn_rebuild_if_running(workspace: &Path, what: &str) -> Result<()> {
    if find_container_id(workspace, false)?.is_some() {
        println!("The container is currently running. Run `bulkhead rebuild` to apply {what}.");
    }

    Ok(())
}

pub(crate) fn up(workspace: Option<PathBuf>) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    start_workspace(&workspace, false)
}

pub(crate) fn rebuild(workspace: Option<PathBuf>) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    start_workspace(&workspace, true)
}

pub(crate) fn down(workspace: Option<PathBuf>) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    ensure_command("docker")?;
    ensure_docker_daemon()?;

    let Some(container_id) = find_container_id(&workspace, false)? else {
        println!("No running devcontainer found for {}", workspace.display());
        return Ok(());
    };

    run_command(
        "docker",
        &[OsString::from("stop"), OsString::from(container_id)],
    )
}

pub(crate) fn shell(workspace: Option<PathBuf>) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    start_workspace(&workspace, false)?;

    run_command(
        "devcontainer",
        &[
            OsString::from("exec"),
            OsString::from("--workspace-folder"),
            workspace.as_os_str().to_os_string(),
            OsString::from("bash"),
        ],
    )
}

pub(crate) fn status(workspace: Option<PathBuf>) -> Result<()> {
    let workspace = workspace_path(workspace)?;

    println!("Workspace: {}", workspace.display());
    let path = config_path(&workspace);
    println!(
        "  config:      {}",
        if path.is_file() {
            path.display().to_string()
        } else {
            "missing".to_owned()
        }
    );

    if !path.is_file() {
        println!("  container:   not initialized");
        return Ok(());
    }

    let config = load_bulkhead_config(&workspace)?;
    println!("  remote user: {}", config.remote_user);
    println!(
        "  git config:  {}",
        if config.git.enabled {
            format!("enabled -> {}", gitconfig_target(&config.remote_user))
        } else {
            "disabled".to_owned()
        }
    );
    println!("  path mounts: {}", config.path.len());

    if !command_exists("docker") {
        println!("  docker:      missing");
        return Ok(());
    }

    if !docker_daemon_running() {
        println!("  docker:      installed, daemon unavailable");
        return Ok(());
    }

    let resources = discover_resources(&workspace)?;
    if let Some(container_id) = resources.container_id.as_deref() {
        println!(
            "  container:   {} ({})",
            resources.container_name.as_deref().unwrap_or(container_id),
            resources.container_status.as_deref().unwrap_or("unknown")
        );
        println!("  id:          {}", container_id);
        if let Some(image) = resources.image.as_deref() {
            println!("  image:       {}", image);
        }
        if !resources.volumes.is_empty() {
            println!("  volumes:     {}", resources.volumes.len());
        }
    } else {
        println!("  container:   not created");
    }

    Ok(())
}

pub(crate) fn logs(args: LogsArgs) -> Result<()> {
    let workspace = workspace_path(args.workspace.workspace)?;
    ensure_command("docker")?;
    ensure_docker_daemon()?;

    let Some(container_id) = find_container_id(&workspace, true)? else {
        println!("No devcontainer found for {}", workspace.display());
        return Ok(());
    };

    let mut docker_args = vec![
        OsString::from("logs"),
        OsString::from("--tail"),
        OsString::from(args.tail.to_string()),
    ];

    if args.follow {
        docker_args.push(OsString::from("--follow"));
    }

    docker_args.push(OsString::from(container_id));
    run_command("docker", &docker_args)
}

pub(crate) fn exec(workspace: Option<PathBuf>, command: Vec<OsString>) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    start_workspace(&workspace, false)?;

    let mut args = vec![
        OsString::from("exec"),
        OsString::from("--workspace-folder"),
        workspace.as_os_str().to_os_string(),
    ];
    args.extend(command);

    run_command("devcontainer", &args)
}

pub(crate) fn destroy(workspace: Option<PathBuf>, force: bool) -> Result<()> {
    let workspace = workspace_path(workspace)?;
    ensure_command("docker")?;
    ensure_docker_daemon()?;
    let resources = discover_resources(&workspace)?;

    if resources.container_id.is_none() {
        println!("No devcontainer found for {}", workspace.display());
        return Ok(());
    }

    print_destroy_summary(&resources);

    if !force
        && matches!(
            resources.container_status.as_deref(),
            Some("running" | "restarting")
        )
        && !confirm("Force-stop the running container?")?
    {
        println!("Aborted.");
        return Ok(());
    }

    if !force && !confirm("Destroy these resources?")? {
        println!("Aborted.");
        return Ok(());
    }

    if let Some(status) = resources.container_status.as_deref() {
        if matches!(status, "running" | "restarting") {
            if let Some(container_id) = resources.container_id.as_deref() {
                run_command_allow_failure(
                    "docker",
                    &[OsString::from("stop"), OsString::from(container_id)],
                )?;
            }
        }
    }

    if let Some(container_id) = resources.container_id.as_deref() {
        run_command_allow_failure(
            "docker",
            &[
                OsString::from("rm"),
                OsString::from("-f"),
                OsString::from(container_id),
            ],
        )?;
    }

    for volume in &resources.volumes {
        run_command_allow_failure(
            "docker",
            &[
                OsString::from("volume"),
                OsString::from("rm"),
                OsString::from("-f"),
                OsString::from(volume),
            ],
        )?;
    }

    if let Some(image) = resources.image.as_deref() {
        run_command_allow_failure(
            "docker",
            &[
                OsString::from("rmi"),
                OsString::from("-f"),
                OsString::from(image),
            ],
        )?;
    }

    println!("All resources destroyed for {}", workspace.display());
    Ok(())
}

fn select_template_preset(preset: Option<Preset>, wizard: bool, yes: bool) -> Result<Preset> {
    if let Some(preset) = preset {
        return Ok(preset);
    }

    if yes {
        return Ok(Preset::Agent);
    }

    if wizard || is_interactive_terminal() {
        return prompt_preset_selection(Preset::Agent);
    }

    Ok(Preset::Agent)
}

fn write_workspace_template(workspace: &Path, preset: Preset, force: bool) -> Result<()> {
    let devcontainer_dir = workspace.join(".devcontainer");
    let bulkhead_toml_path = config_path(workspace);
    let dockerfile_path = devcontainer_dir.join("Dockerfile");
    let post_create_script_path = devcontainer_dir.join("bulkhead-post-create.sh");
    let files = [
        &bulkhead_toml_path,
        &dockerfile_path,
        &post_create_script_path,
    ];

    if !force {
        for path in files {
            if path.exists() {
                bail!(
                    "{} already exists. Re-run with --force to overwrite the template.",
                    path.display()
                );
            }
        }
    }

    fs::create_dir_all(&devcontainer_dir)
        .with_context(|| format!("failed to create {}", devcontainer_dir.display()))?;

    fs::write(
        &bulkhead_toml_path,
        instantiate_template(preset.template())?,
    )
    .with_context(|| format!("failed to write {}", bulkhead_toml_path.display()))?;
    fs::write(&dockerfile_path, DOCKERFILE)
        .with_context(|| format!("failed to write {}", dockerfile_path.display()))?;
    fs::write(&post_create_script_path, POST_CREATE_SCRIPT)
        .with_context(|| format!("failed to write {}", post_create_script_path.display()))?;

    render_workspace_devcontainer(workspace)
}

fn start_workspace(workspace: &Path, rebuild: bool) -> Result<()> {
    maybe_bootstrap_workspace(workspace)?;
    ensure_docker_daemon()?;
    ensure_devcontainer_cli(true)?;
    render_workspace_devcontainer(workspace)?;

    if !rebuild && find_container_id(workspace, false)?.is_some() {
        return Ok(());
    }

    let mut args = vec![
        OsString::from("up"),
        OsString::from("--workspace-folder"),
        workspace.as_os_str().to_os_string(),
    ];

    if rebuild {
        args.push(OsString::from("--remove-existing-container"));
    }

    run_command("devcontainer", &args)
}
