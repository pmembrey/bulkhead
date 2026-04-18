use crate::docker::{BuildxHealth, docker_daemon_running, probe_buildx_health};
use crate::system::{
    command_exists, install_devcontainer, print_buildx_permission_help,
    print_devcontainer_install_help, print_docker_daemon_help,
};
use anyhow::Result;

pub(crate) fn doctor(fix: bool) -> Result<()> {
    let docker_installed = command_exists("docker");
    let docker_daemon_running = if docker_installed {
        docker_daemon_running()
    } else {
        false
    };
    let buildx_health = if docker_daemon_running {
        probe_buildx_health()?
    } else {
        None
    };
    let devcontainer_installed = command_exists("devcontainer");
    let brew_installed = command_exists("brew");
    let npm_installed = command_exists("npm");

    println!("Environment checks:");
    println!(
        "  docker:       {}",
        if docker_installed {
            "installed"
        } else {
            "missing"
        }
    );
    println!(
        "  docker daemon:{}",
        if docker_daemon_running {
            " reachable"
        } else {
            " unavailable"
        }
    );
    println!(
        "  devcontainer: {}",
        if devcontainer_installed {
            "installed"
        } else {
            "missing"
        }
    );
    if let Some(health) = buildx_health.as_ref() {
        match health {
            BuildxHealth::Ready => println!("  buildx:       ready"),
            BuildxHealth::PermissionDenied(_) => {
                println!("  buildx:       permission problem")
            }
            BuildxHealth::Error(_) => println!("  buildx:       error"),
        }
    }
    println!(
        "  brew:         {}",
        if brew_installed {
            "installed"
        } else {
            "missing"
        }
    );
    println!(
        "  npm:          {}",
        if npm_installed {
            "installed"
        } else {
            "missing"
        }
    );

    if !docker_installed {
        println!();
        println!("Docker is required before Bulkhead can start a container.");
    } else if !docker_daemon_running {
        println!();
        print_docker_daemon_help();
    } else if let Some(BuildxHealth::PermissionDenied(details)) = buildx_health.as_ref() {
        println!();
        print_buildx_permission_help(details);
    } else if let Some(BuildxHealth::Error(details)) = buildx_health.as_ref() {
        println!();
        println!(
            "Docker buildx is installed, but the health probe failed: {}",
            details
        );
    }

    if devcontainer_installed {
        return Ok(());
    }

    println!();
    print_devcontainer_install_help();

    if fix {
        install_devcontainer(false)?;
    } else {
        println!("Run `bulkhead doctor --fix` to install the recommended option automatically.");
    }

    Ok(())
}
