use crate::prompt::confirm;
use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum DevcontainerInstaller {
    Brew,
    Npm,
}

pub(crate) fn command_exists(program: &str) -> bool {
    resolve_command_path(program).is_some()
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

pub(crate) fn ensure_command(program: &str) -> Result<()> {
    if command_exists(program) {
        return Ok(());
    }

    bail!("{program} is not installed or not in PATH")
}

pub(crate) fn ensure_docker_daemon() -> Result<()> {
    ensure_command("docker")?;

    let output = new_command("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .context("failed to probe the Docker daemon")?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.trim().is_empty() {
        print_docker_daemon_help();
        bail!("Docker is installed, but the Docker daemon is not reachable")
    }

    print_docker_daemon_help();
    bail!(
        "Docker is installed, but the Docker daemon is not reachable: {}",
        stderr.trim()
    )
}

pub(crate) fn ensure_devcontainer_cli(offer_install: bool) -> Result<()> {
    if command_exists("devcontainer") {
        return Ok(());
    }

    println!("The Dev Container CLI is required for this command.");
    print_devcontainer_install_help();

    if !offer_install {
        bail!("devcontainer is not installed");
    }

    if io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && let Some(installer) =
            choose_devcontainer_installer(command_exists("brew"), command_exists("npm"))
    {
        let prompt = format!(
            "Install the Dev Container CLI now with `{}`?",
            installer.command_string()
        );
        if confirm(&prompt)? {
            install_devcontainer(false)?;
            if command_exists("devcontainer") {
                return Ok(());
            }

            bail!("devcontainer install completed, but the binary is still not on PATH");
        }
    }

    bail!("devcontainer is not installed")
}

pub(crate) fn choose_devcontainer_installer(
    has_brew: bool,
    has_npm: bool,
) -> Option<DevcontainerInstaller> {
    if has_brew {
        Some(DevcontainerInstaller::Brew)
    } else if has_npm {
        Some(DevcontainerInstaller::Npm)
    } else {
        None
    }
}

pub(crate) fn install_devcontainer(prompt_before_install: bool) -> Result<()> {
    if command_exists("devcontainer") {
        println!("devcontainer is already installed.");
        return Ok(());
    }

    let Some(installer) =
        choose_devcontainer_installer(command_exists("brew"), command_exists("npm"))
    else {
        bail!("could not auto-install devcontainer because neither brew nor npm is available");
    };

    if prompt_before_install && io::stdin().is_terminal() && io::stdout().is_terminal() {
        let prompt = format!(
            "Install the Dev Container CLI now with `{}`?",
            installer.command_string()
        );
        if !confirm(&prompt)? {
            bail!("devcontainer install skipped");
        }
    }

    println!(
        "Installing devcontainer with `{}`...",
        installer.command_string()
    );
    run_command(installer.program(), &installer.args())?;

    if !command_exists("devcontainer") {
        bail!("install command finished, but `devcontainer` is still not available on PATH");
    }

    println!("devcontainer is now installed.");
    Ok(())
}

pub(crate) fn print_devcontainer_install_help() {
    println!("Install options:");
    println!("  Homebrew: brew install devcontainer");
    println!("  npm:      npm install -g @devcontainers/cli");
    println!(
        "  script:   curl -fsSL https://raw.githubusercontent.com/devcontainers/cli/main/scripts/install.sh | sh"
    );
}

pub(crate) fn print_docker_daemon_help() {
    println!("Docker is installed, but the daemon is not reachable.");
    println!("Start your container runtime first, then retry.");
    println!("Common options:");
    println!("  Docker Desktop: open Docker Desktop and wait until it reports it is running");
    println!("  OrbStack: open OrbStack");
    println!("  Colima: colima start");
}

pub(crate) fn print_buildx_permission_help(details: &str) {
    println!("Docker buildx is reachable, but it hit a permission error.");
    println!(
        "This is often the `~/.docker/buildx/activity/... operation not permitted` failure that surfaces later during `devcontainer up`."
    );
    println!(
        "Check ownership and permissions under `~/.docker` and `~/.docker/buildx`, then retry."
    );
    println!("buildx details: {}", details);
}

pub(crate) fn run_command(program: &str, args: &[OsString]) -> Result<()> {
    let status = new_command(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run {}", render_command(program, args)))?;

    if status.success() {
        return Ok(());
    }

    bail!("command failed: {}", render_command(program, args))
}

pub(crate) fn run_command_allow_failure(program: &str, args: &[OsString]) -> Result<()> {
    let _ = new_command(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run {}", render_command(program, args)))?;

    Ok(())
}

pub(crate) fn capture_stdout(program: &str, args: &[&str]) -> Result<String> {
    let output = new_command(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {}", render_command(program, args)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "command failed: {}{}",
            render_command(program, args),
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", stderr.trim())
            }
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn render_command<I, S>(program: &str, args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut rendered = vec![program.to_owned()];
    rendered.extend(
        args.into_iter()
            .map(|arg| arg.as_ref().to_string_lossy().into_owned()),
    );
    rendered.join(" ")
}

fn resolve_command_path(program: &str) -> Option<PathBuf> {
    if program.trim().is_empty() {
        return None;
    }

    let candidate = Path::new(program);
    if candidate.components().count() > 1 {
        return is_executable_file(candidate).then(|| candidate.to_path_buf());
    }

    resolve_command_in_paths(program, &command_search_paths())
}

fn resolve_command_in_paths(program: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    search_paths.iter().find_map(|entry| {
        let candidate = entry.join(program);
        is_executable_file(&candidate).then_some(candidate)
    })
}

fn new_command(program: &str) -> Command {
    let mut command = if let Some(path) = resolve_command_path(program) {
        Command::new(path)
    } else {
        Command::new(program)
    };

    if let Some(path) = command_path_env() {
        command.env("PATH", path);
    }

    command
}

fn command_path_env() -> Option<OsString> {
    std::env::join_paths(command_search_paths()).ok()
}

fn command_search_paths() -> Vec<PathBuf> {
    let path_entries = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    let home_dir = std::env::var_os("HOME").map(PathBuf::from);
    let username = std::env::var("USER").ok();

    build_command_search_paths(path_entries, home_dir.as_deref(), username.as_deref())
}

fn build_command_search_paths(
    path_entries: Vec<PathBuf>,
    home_dir: Option<&Path>,
    username: Option<&str>,
) -> Vec<PathBuf> {
    let mut search_paths = Vec::new();
    push_unique_path(&mut search_paths, PathBuf::from("/run/wrappers/bin"));

    for entry in path_entries {
        push_unique_path(&mut search_paths, entry);
    }

    for entry in nix_profile_candidates(home_dir, username) {
        push_unique_path(&mut search_paths, entry);
    }

    search_paths
}

fn nix_profile_candidates(home_dir: Option<&Path>, username: Option<&str>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(home_dir) = home_dir {
        candidates.push(home_dir.join(".local/state/nix/profile/bin"));
        candidates.push(home_dir.join(".nix-profile/bin"));
    }

    if let Some(username) = username {
        candidates.push(PathBuf::from(format!(
            "/etc/profiles/per-user/{username}/bin"
        )));
    }

    candidates.push(PathBuf::from("/nix/profile/bin"));
    candidates.push(PathBuf::from("/nix/var/nix/profiles/default/bin"));
    candidates.push(PathBuf::from("/run/current-system/sw/bin"));

    candidates
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
    }
}

impl DevcontainerInstaller {
    fn program(self) -> &'static str {
        match self {
            DevcontainerInstaller::Brew => "brew",
            DevcontainerInstaller::Npm => "npm",
        }
    }

    fn args(self) -> Vec<OsString> {
        match self {
            DevcontainerInstaller::Brew => {
                vec![OsString::from("install"), OsString::from("devcontainer")]
            }
            DevcontainerInstaller::Npm => vec![
                OsString::from("install"),
                OsString::from("-g"),
                OsString::from("@devcontainers/cli"),
            ],
        }
    }

    fn command_string(self) -> &'static str {
        match self {
            DevcontainerInstaller::Brew => "brew install devcontainer",
            DevcontainerInstaller::Npm => "npm install -g @devcontainers/cli",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DevcontainerInstaller, build_command_search_paths, choose_devcontainer_installer,
        nix_profile_candidates, resolve_command_in_paths,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn brew_is_preferred_over_npm() {
        assert_eq!(
            choose_devcontainer_installer(true, true),
            Some(DevcontainerInstaller::Brew)
        );
        assert_eq!(
            choose_devcontainer_installer(false, true),
            Some(DevcontainerInstaller::Npm)
        );
        assert_eq!(choose_devcontainer_installer(false, false), None);
    }

    #[test]
    fn nix_search_paths_include_wrapper_and_profiles() {
        let paths = build_command_search_paths(
            vec![PathBuf::from("/custom/bin")],
            Some(Path::new("/home/alice")),
            Some("alice"),
        );

        assert_eq!(paths[0], PathBuf::from("/run/wrappers/bin"));
        assert!(paths.contains(&PathBuf::from("/custom/bin")));
        assert!(paths.contains(&PathBuf::from("/home/alice/.local/state/nix/profile/bin")));
        assert!(paths.contains(&PathBuf::from("/home/alice/.nix-profile/bin")));
        assert!(paths.contains(&PathBuf::from("/etc/profiles/per-user/alice/bin")));
        assert!(paths.contains(&PathBuf::from("/nix/profile/bin")));
        assert!(paths.contains(&PathBuf::from("/nix/var/nix/profiles/default/bin")));
        assert!(paths.contains(&PathBuf::from("/run/current-system/sw/bin")));
    }

    #[test]
    fn nix_profile_candidates_skip_user_specific_entries_without_context() {
        let candidates = nix_profile_candidates(None, None);

        assert!(!candidates.iter().any(|path| path.starts_with("/home")));
        assert!(
            !candidates
                .iter()
                .any(|path| path.starts_with("/etc/profiles/per-user"))
        );
        assert!(candidates.contains(&PathBuf::from("/nix/profile/bin")));
        assert!(candidates.contains(&PathBuf::from("/run/current-system/sw/bin")));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_command_in_paths_finds_executable() {
        let base = std::env::temp_dir().join(format!(
            "bulkhead-system-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bin_dir = base.join("bin");
        let command_path = bin_dir.join("devcontainer");

        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(&command_path, "#!/bin/sh\nexit 0\n").unwrap();

        let mut permissions = fs::metadata(&command_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&command_path, permissions).unwrap();

        let resolved = resolve_command_in_paths("devcontainer", &[bin_dir]);
        assert_eq!(resolved, Some(command_path.clone()));

        fs::remove_file(&command_path).unwrap();
        fs::remove_dir(base.join("bin")).unwrap();
        fs::remove_dir(base).unwrap();
    }
}
