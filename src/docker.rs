use crate::system::{DOCKER_PROBE_TIMEOUT, capture_stdout, command_output_with_timeout};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct DockerMount {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "Name")]
    name: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Resources {
    pub(crate) container_id: Option<String>,
    pub(crate) container_name: Option<String>,
    pub(crate) container_status: Option<String>,
    pub(crate) volumes: Vec<String>,
    pub(crate) image: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum BuildxHealth {
    Ready,
    PermissionDenied(String),
    Error(String),
}

pub(crate) fn docker_daemon_running() -> bool {
    command_output_with_timeout(
        "docker",
        &["version", "--format", "{{.Server.Version}}"],
        DOCKER_PROBE_TIMEOUT,
    )
    .is_ok_and(|output| output.is_some_and(|o| o.status.success()))
}

pub(crate) fn probe_buildx_health() -> Result<Option<BuildxHealth>> {
    let Some(version) =
        command_output_with_timeout("docker", &["buildx", "version"], DOCKER_PROBE_TIMEOUT)
            .context("failed to probe docker buildx")?
    else {
        return Ok(Some(BuildxHealth::Error(
            "docker buildx version timed out".to_owned(),
        )));
    };

    if !version.status.success() {
        let stderr = String::from_utf8_lossy(&version.stderr).trim().to_owned();
        if stderr.is_empty() {
            return Ok(None);
        }
        return Ok(Some(BuildxHealth::Error(stderr)));
    }

    let Some(inspect) =
        command_output_with_timeout("docker", &["buildx", "inspect"], DOCKER_PROBE_TIMEOUT)
            .context("failed to inspect docker buildx")?
    else {
        return Ok(Some(BuildxHealth::Error(
            "docker buildx inspect timed out".to_owned(),
        )));
    };

    if inspect.status.success() {
        return Ok(Some(BuildxHealth::Ready));
    }

    let stderr = String::from_utf8_lossy(&inspect.stderr).trim().to_owned();
    let details = if stderr.is_empty() {
        "docker buildx inspect failed".to_owned()
    } else {
        stderr
    };

    Ok(Some(classify_buildx_failure(&details)))
}

pub(crate) fn classify_buildx_failure(details: &str) -> BuildxHealth {
    if details
        .to_ascii_lowercase()
        .contains("operation not permitted")
    {
        BuildxHealth::PermissionDenied(details.to_owned())
    } else {
        BuildxHealth::Error(details.to_owned())
    }
}

pub(crate) fn find_container_id(workspace: &Path, include_stopped: bool) -> Result<Option<String>> {
    let filter = format!("label=devcontainer.local_folder={}", workspace.display());
    let args = if include_stopped {
        vec!["ps", "-aq", "--filter", filter.as_str()]
    } else {
        vec!["ps", "-q", "--filter", filter.as_str()]
    };

    let output = capture_stdout("docker", &args).context("failed to discover devcontainer")?;

    Ok(output
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned))
}

pub(crate) fn discover_resources(workspace: &Path) -> Result<Resources> {
    let Some(container_id) = find_container_id(workspace, true)? else {
        return Ok(Resources::default());
    };

    let container_status = capture_stdout(
        "docker",
        &["inspect", &container_id, "--format", "{{.State.Status}}"],
    )?;
    let container_name = capture_stdout(
        "docker",
        &["inspect", &container_id, "--format", "{{.Name}}"],
    )?;
    let mounts_json = capture_stdout(
        "docker",
        &["inspect", &container_id, "--format", "{{json .Mounts}}"],
    )?;
    let image = capture_stdout(
        "docker",
        &["inspect", &container_id, "--format", "{{.Config.Image}}"],
    )?;

    let mounts: Vec<DockerMount> = serde_json::from_str(&mounts_json)
        .context("failed to parse docker mount metadata for the devcontainer")?;

    let volumes = mounts
        .into_iter()
        .filter(|mount| mount.kind == "volume")
        .filter_map(|mount| mount.name)
        .collect();

    Ok(Resources {
        container_id: Some(container_id),
        container_name: Some(container_name.trim_start_matches('/').to_owned()),
        container_status: Some(container_status),
        volumes,
        image: if image.is_empty() { None } else { Some(image) },
    })
}

pub(crate) fn print_destroy_summary(resources: &Resources) {
    println!();
    println!("The following resources will be permanently removed:");
    println!();

    if let Some(container_name) = resources.container_name.as_deref() {
        println!("  Container:  {container_name}");
    } else if let Some(container_id) = resources.container_id.as_deref() {
        println!("  Container:  {container_id}");
    }

    if let Some(status) = resources.container_status.as_deref()
        && matches!(status, "running" | "restarting")
    {
        println!("              (currently running and will be force-stopped)");
    }

    if !resources.volumes.is_empty() {
        println!("  Volumes:");
        for volume in &resources.volumes {
            println!("              {volume}");
        }
    }

    if let Some(image) = resources.image.as_deref() {
        println!("  Image:      {image}");
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::{BuildxHealth, classify_buildx_failure};

    #[test]
    fn buildx_permission_errors_are_classified() {
        assert_eq!(
            classify_buildx_failure(
                "failed to update builder last activity time: open ~/.docker/buildx/activity/tmp: operation not permitted"
            ),
            BuildxHealth::PermissionDenied(
                "failed to update builder last activity time: open ~/.docker/buildx/activity/tmp: operation not permitted"
                    .to_owned()
            )
        );
    }
}
