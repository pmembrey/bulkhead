use crate::prompt::confirm;
use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum DevcontainerInstaller {
    Brew,
    Npm,
}

pub(crate) fn command_exists(program: &str) -> bool {
    if program.trim().is_empty() {
        return false;
    }

    let candidate = Path::new(program);
    if candidate.components().count() > 1 {
        return is_executable_file(candidate);
    }

    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|entry| is_executable_file(&entry.join(program)))
    })
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

    let output = Command::new("docker")
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

    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        if let Some(installer) =
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
    let status = Command::new(program)
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
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run {}", render_command(program, args)))?;

    Ok(())
}

pub(crate) fn capture_stdout(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
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
    use super::{DevcontainerInstaller, choose_devcontainer_installer};

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
}
