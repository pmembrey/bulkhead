use crate::config::{
    BulkheadConfig, MountAccess, PreinstalledAgent, config_path, devcontainer_relative_path,
    gitconfig_target, home_dir, is_docker_socket_path, load_bulkhead_config, resolve_mount_source,
    resolve_path_for_policy_checks, resolve_plain_host_path, resolve_workspace_config_path,
    sanitize_volume_name,
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const DEVCONTAINER_SCHEMA: &str =
    "https://raw.githubusercontent.com/devcontainers/spec/main/schemas/devContainer.schema.json";
const NODE_FEATURE: &str = "ghcr.io/devcontainers/features/node:1";
const POST_CREATE_SCRIPT_NAME: &str = "bulkhead-post-create.sh";

#[derive(Debug, Serialize)]
pub(crate) struct GeneratedDevcontainer {
    #[serde(rename = "$schema")]
    schema: &'static str,
    name: String,
    build: BuildSpec,
    features: BTreeMap<String, Value>,
    #[serde(rename = "runArgs")]
    run_args: Vec<String>,
    init: bool,
    #[serde(rename = "updateRemoteUserUID")]
    update_remote_user_uid: bool,
    #[serde(rename = "containerUser")]
    container_user: String,
    #[serde(rename = "remoteUser")]
    remote_user: String,
    mounts: Vec<String>,
    #[serde(rename = "containerEnv")]
    container_env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", rename = "remoteEnv")]
    remote_env: BTreeMap<String, String>,
    customizations: Value,
    #[serde(skip_serializing_if = "Option::is_none", rename = "initializeCommand")]
    initialize_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "postCreateCommand")]
    post_create_command: Option<String>,
    #[serde(rename = "workspaceMount")]
    workspace_mount: String,
    #[serde(rename = "workspaceFolder")]
    workspace_folder: String,
}

#[derive(Debug, Serialize)]
struct BuildSpec {
    context: String,
    dockerfile: String,
    args: BTreeMap<String, String>,
}

pub(crate) fn render_workspace_devcontainer(workspace: &Path) -> Result<()> {
    ensure_workspace_layout(workspace)?;
    let config = load_bulkhead_config(workspace)?;
    let generated = generate_devcontainer(workspace, &config)?;
    write_generated_devcontainer(workspace, &generated)
}

pub(crate) fn persist_bulkhead_document(
    workspace: &Path,
    document: toml_edit::DocumentMut,
) -> Result<()> {
    let rendered = document.to_string();
    let config = crate::config::load_inline_config(&rendered)
        .context("failed to validate generated bulkhead config")?;
    let generated = generate_devcontainer(workspace, &config)?;

    crate::config::write_bulkhead_document(workspace, &document)?;
    write_generated_devcontainer(workspace, &generated)
}

pub(crate) fn ensure_workspace_layout(workspace: &Path) -> Result<()> {
    if !config_path(workspace).is_file() {
        bail!(
            "missing {}. Run `bulkhead template` in the workspace first.",
            config_path(workspace).display()
        );
    }

    let devcontainer_dir = workspace.join(".devcontainer");
    fs::create_dir_all(&devcontainer_dir)
        .with_context(|| format!("failed to create {}", devcontainer_dir.display()))?;

    let config = load_bulkhead_config(workspace)?;
    let dockerfile_path = resolve_workspace_config_path(workspace, &config.build.dockerfile)?;
    if !dockerfile_path.is_file() {
        bail!(
            "missing {}. Update [build].dockerfile or run `bulkhead template --force` to restore the managed files.",
            dockerfile_path.display()
        );
    }

    let context_path = resolve_workspace_config_path(workspace, &config.build.context)?;
    if !context_path.is_dir() {
        bail!(
            "missing {}. Update [build].context or create that directory before starting the workspace.",
            context_path.display()
        );
    }

    if !config.agents.is_empty() {
        let post_create_script = devcontainer_dir.join(POST_CREATE_SCRIPT_NAME);
        if !post_create_script.is_file() {
            bail!(
                "missing {}. Run `bulkhead template --force` to restore the agent bootstrap script.",
                post_create_script.display()
            );
        }
    }

    Ok(())
}

pub(crate) fn write_generated_devcontainer(
    workspace: &Path,
    generated: &GeneratedDevcontainer,
) -> Result<()> {
    let rendered = serde_json::to_string_pretty(generated)
        .context("failed to render devcontainer.json")?
        + "\n";

    let path = workspace.join(".devcontainer/devcontainer.json");
    fs::write(&path, rendered).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn generate_devcontainer(
    workspace: &Path,
    config: &BulkheadConfig,
) -> Result<GeneratedDevcontainer> {
    validate_config(workspace, config)?;

    let workspace_folder = normalize_container_path(&config.workspace_folder)?;
    let devcontainer_target = join_container_path(&workspace_folder, ".devcontainer");
    let bulkhead_config_target = join_container_path(&workspace_folder, "bulkhead.toml");
    let dockerfile_path = resolve_workspace_config_path(workspace, &config.build.dockerfile)?;
    let context_path = resolve_workspace_config_path(workspace, &config.build.context)?;

    Ok(GeneratedDevcontainer {
        schema: DEVCONTAINER_SCHEMA,
        name: config.name.clone(),
        build: BuildSpec {
            context: devcontainer_relative_path(workspace, &context_path)?,
            dockerfile: devcontainer_relative_path(workspace, &dockerfile_path)?,
            args: build_args(config),
        },
        features: build_features(config),
        run_args: config.run_args.clone(),
        init: true,
        update_remote_user_uid: true,
        container_user: config.remote_user.clone(),
        remote_user: config.remote_user.clone(),
        mounts: build_mounts(
            workspace,
            config,
            &devcontainer_target,
            &bulkhead_config_target,
        )?,
        container_env: build_container_env(config),
        remote_env: build_remote_env(config),
        customizations: default_customizations(),
        initialize_command: initialize_command(config),
        post_create_command: post_create_command(config, &devcontainer_target),
        workspace_mount: format!(
            "source=${{localWorkspaceFolder}},target={workspace_folder},type=bind,consistency=delegated"
        ),
        workspace_folder,
    })
}

pub(crate) fn validate_config(workspace: &Path, config: &BulkheadConfig) -> Result<()> {
    let workspace_folder = normalize_container_path(&config.workspace_folder)?;
    let reserved_devcontainer_target = join_container_path(&workspace_folder, ".devcontainer");
    let reserved_bulkhead_config_target = join_container_path(&workspace_folder, "bulkhead.toml");
    let git_target = gitconfig_target(&config.remote_user);

    if config.remote_user.trim().is_empty() {
        bail!("remote_user must not be empty");
    }

    if !config
        .remote_user
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        bail!("remote_user may only contain ASCII letters, numbers, underscores, dashes, and dots");
    }

    if config.name.trim().is_empty() {
        bail!("name must not be empty");
    }

    if config.build_args.contains_key("BULKHEAD_REMOTE_USER") {
        bail!("build_args.BULKHEAD_REMOTE_USER is reserved for Bulkhead");
    }

    let dockerfile_path = resolve_workspace_config_path(workspace, &config.build.dockerfile)?;
    let context_path = resolve_workspace_config_path(workspace, &config.build.context)?;

    if !dockerfile_path.starts_with(workspace) {
        bail!("build.dockerfile must stay within the workspace");
    }

    if !context_path.starts_with(workspace) {
        bail!("build.context must stay within the workspace");
    }

    if config
        .run_args
        .iter()
        .any(|arg| arg.to_ascii_uppercase().contains("SYS_ADMIN"))
    {
        bail!(
            "SYS_ADMIN capability detected in run_args; this defeats the read-only .devcontainer mount"
        );
    }

    let mut targets = BTreeSet::new();
    targets.insert(reserved_devcontainer_target.clone());
    targets.insert(reserved_bulkhead_config_target.clone());
    let home_dir = home_dir()?;
    let resolved_home_dir = resolve_path_for_policy_checks(&home_dir);
    let mut volume_names = BTreeSet::new();
    let mut seen_agents = BTreeSet::new();

    for key in reserved_container_env_keys(config) {
        if config.container_env.contains_key(key) {
            bail!("container_env.{key} is reserved for Bulkhead");
        }
    }

    if config.git.enabled && !targets.insert(git_target.clone()) {
        bail!("duplicate mount target {}", git_target);
    }

    for agent in &config.agents {
        if !seen_agents.insert(*agent) {
            bail!("duplicate agent {}", agent.as_str());
        }

        if !targets.insert(agent.config_target(&config.remote_user)) {
            bail!(
                "duplicate mount target {}",
                agent.config_target(&config.remote_user)
            );
        }
    }

    for volume in &config.volume {
        if volume.name.trim().is_empty() {
            bail!("volume names must not be empty");
        }

        let sanitized_name = sanitize_volume_name(&volume.name);
        if sanitized_name.is_empty() {
            bail!(
                "volume name {} is not usable after sanitization",
                volume.name
            );
        }

        if !volume_names.insert(sanitized_name) {
            bail!("duplicate volume name {}", volume.name);
        }

        if !targets.insert(volume.target.clone()) {
            bail!("duplicate mount target {}", volume.target);
        }

        ensure_absolute_container_path(&volume.target)?;
    }

    for path in &config.path {
        if !targets.insert(path.target.clone()) {
            bail!("duplicate mount target {}", path.target);
        }

        ensure_absolute_container_path(&path.target)?;

        if path.target == reserved_devcontainer_target {
            bail!(
                "{reserved_devcontainer_target} is reserved for Bulkhead's read-only config mount"
            );
        }

        if path.target == reserved_bulkhead_config_target {
            bail!(
                "{reserved_bulkhead_config_target} is reserved for Bulkhead's read-only config mount"
            );
        }

        let source = resolve_mount_source(workspace, &path.source)?;
        let source_lower = source.to_ascii_lowercase();
        let target_lower = path.target.to_ascii_lowercase();

        if source_lower.contains("docker.sock") || target_lower.contains("docker.sock") {
            bail!("mounting the Docker socket is not allowed");
        }

        if let Some(resolved_home) = resolve_plain_host_path(workspace, &path.source, &home_dir) {
            let resolved_home = resolve_path_for_policy_checks(&resolved_home);

            if path.access == MountAccess::Rw && resolved_home == resolved_home_dir {
                bail!("mounting your entire home directory read-write is not allowed");
            }

            if is_docker_socket_path(&resolved_home) {
                bail!("mounting the Docker socket is not allowed");
            }
        }
    }

    Ok(())
}

fn build_args(config: &BulkheadConfig) -> BTreeMap<String, String> {
    let mut build_args = config.build_args.clone();
    build_args.insert(
        "BULKHEAD_REMOTE_USER".to_owned(),
        config.remote_user.clone(),
    );
    build_args
}

fn build_features(config: &BulkheadConfig) -> BTreeMap<String, Value> {
    let mut features = BTreeMap::new();
    for feature in &config.features {
        features.insert(feature.clone(), json!({}));
    }

    if !config.agents.is_empty() && !features.contains_key(NODE_FEATURE) {
        features.insert(NODE_FEATURE.to_owned(), json!({ "version": "22" }));
    }

    features
}

fn build_container_env(config: &BulkheadConfig) -> BTreeMap<String, String> {
    let mut env = config.container_env.clone();

    if !config.agents.is_empty() {
        env.insert(
            "BULKHEAD_SELECTED_AGENTS".to_owned(),
            config
                .agents
                .iter()
                .map(|agent| agent.as_str())
                .collect::<Vec<_>>()
                .join(","),
        );
    }

    if config.agents.contains(&PreinstalledAgent::Claude) {
        env.insert(
            "CLAUDE_CONFIG_DIR".to_owned(),
            PreinstalledAgent::Claude.config_target(&config.remote_user),
        );
        env.entry("DISABLE_AUTOUPDATER".to_owned())
            .or_insert_with(|| "1".to_owned());
    }

    env
}

fn build_remote_env(config: &BulkheadConfig) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();

    if !config.agents.is_empty() {
        env.insert(
            "PATH".to_owned(),
            format!(
                "{}/.local/bin:${{containerEnv:PATH}}",
                remote_user_home(&config.remote_user)
            ),
        );
    }

    if config.agents.contains(&PreinstalledAgent::Claude) {
        env.insert(
            "ANTHROPIC_API_KEY".to_owned(),
            "${localEnv:ANTHROPIC_API_KEY:}".to_owned(),
        );
        env.insert(
            "CLAUDE_CODE_OAUTH_TOKEN".to_owned(),
            "${localEnv:CLAUDE_CODE_OAUTH_TOKEN:}".to_owned(),
        );
    }

    if config.agents.contains(&PreinstalledAgent::Codex) {
        env.insert(
            "OPENAI_API_KEY".to_owned(),
            "${localEnv:OPENAI_API_KEY:}".to_owned(),
        );
    }

    env
}

fn build_mounts(
    workspace: &Path,
    config: &BulkheadConfig,
    devcontainer_target: &str,
    bulkhead_config_target: &str,
) -> Result<Vec<String>> {
    let mut mounts = Vec::new();

    for volume in &config.volume {
        mounts.push(format!(
            "source=bulkhead-${{localWorkspaceFolderBasename}}-{}-${{devcontainerId}},target={},type=volume",
            sanitize_volume_name(&volume.name),
            volume.target
        ));
    }

    if config.git.enabled {
        let source = resolve_mount_source(workspace, "~/.gitconfig")?;
        mounts.push(format!(
            "source={source},target={},type=bind,readonly",
            gitconfig_target(&config.remote_user)
        ));
    }

    for agent in &config.agents {
        mounts.push(format!(
            "source=bulkhead-${{localWorkspaceFolderBasename}}-{}-${{devcontainerId}},target={},type=volume",
            agent.as_str(),
            agent.config_target(&config.remote_user)
        ));
    }

    for path in &config.path {
        let source = resolve_mount_source(workspace, &path.source)?;
        let mut mount = format!("source={source},target={},type=bind", path.target);
        if path.access == MountAccess::Ro {
            mount.push_str(",readonly");
        }
        mounts.push(mount);
    }

    mounts.push(format!(
        "source=${{localWorkspaceFolder}}/.devcontainer,target={devcontainer_target},type=bind,readonly"
    ));
    mounts.push(format!(
        "source=${{localWorkspaceFolder}}/bulkhead.toml,target={bulkhead_config_target},type=bind,readonly"
    ));

    Ok(mounts)
}

fn initialize_command(config: &BulkheadConfig) -> Option<String> {
    if config.git.enabled {
        return Some("test -f \"$HOME/.gitconfig\" || touch \"$HOME/.gitconfig\"".to_owned());
    }

    None
}

fn post_create_command(config: &BulkheadConfig, devcontainer_target: &str) -> Option<String> {
    if config.agents.is_empty() {
        return None;
    }

    Some(format!(
        "bash {devcontainer_target}/{POST_CREATE_SCRIPT_NAME}"
    ))
}

fn default_customizations() -> Value {
    json!({
        "vscode": {
            "settings": {
                "terminal.integrated.defaultProfile.linux": "bash",
                "terminal.integrated.profiles.linux": {
                    "bash": {
                        "path": "bash",
                        "icon": "terminal-bash"
                    }
                },
                "files.trimTrailingWhitespace": true,
                "files.insertFinalNewline": true,
                "files.trimFinalNewlines": true
            }
        }
    })
}

fn reserved_container_env_keys(config: &BulkheadConfig) -> Vec<&'static str> {
    let mut keys = Vec::new();

    if !config.agents.is_empty() {
        keys.push("BULKHEAD_SELECTED_AGENTS");
    }

    if config.agents.contains(&PreinstalledAgent::Claude) {
        keys.push("CLAUDE_CONFIG_DIR");
    }

    keys
}

fn remote_user_home(remote_user: &str) -> String {
    if remote_user == "root" {
        "/root".to_owned()
    } else {
        format!("/home/{remote_user}")
    }
}

pub(crate) fn normalize_container_path(path: &str) -> Result<String> {
    if !path.starts_with('/') {
        bail!("container paths must be absolute: {path}");
    }

    if path == "/" {
        return Ok("/".to_owned());
    }

    Ok(path.trim_end_matches('/').to_owned())
}

fn ensure_absolute_container_path(path: &str) -> Result<()> {
    if !path.starts_with('/') {
        bail!("container paths must be absolute: {path}");
    }

    Ok(())
}

fn join_container_path(base: &str, segment: &str) -> String {
    if base == "/" {
        format!("/{}", segment.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            segment.trim_start_matches('/')
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{generate_devcontainer, normalize_container_path};
    use crate::config::{
        AGENT_PRESET_TOML, AUDIT_PRESET_TOML, BulkheadConfig, MINIMAL_PRESET_TOML,
        gitconfig_target, instantiate_template, load_inline_config,
    };
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn template_config_renders() {
        let rendered = instantiate_template(AGENT_PRESET_TOML).unwrap();
        let config: BulkheadConfig = load_inline_config(&rendered).unwrap();
        let generated = generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).unwrap();

        assert_eq!(generated.remote_user, config.remote_user);
        assert_eq!(generated.container_user, config.remote_user);
        assert_eq!(
            generated.build.args.get("BULKHEAD_REMOTE_USER"),
            Some(&config.remote_user)
        );
        assert!(
            generated
                .mounts
                .iter()
                .any(|mount| mount.contains("target=/workspace/.devcontainer")
                    && mount.contains("readonly"))
        );
        assert!(
            generated
                .mounts
                .iter()
                .any(|mount| mount.contains("target=/workspace/bulkhead.toml")
                    && mount.contains("readonly"))
        );
        assert!(generated.mounts.iter().any(|mount| {
            mount.contains(&format!("target={}", gitconfig_target(&config.remote_user)))
        }));
        assert!(generated.run_args.is_empty());
    }

    #[test]
    fn rw_mounts_are_rendered_from_bulkhead_toml() {
        let config = load_inline_config(
            r#"
name = "Custom"

[[path]]
source = "drop"
target = "/drop"
access = "rw"
"#,
        )
        .unwrap();

        let generated = generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).unwrap();
        assert!(
            generated.mounts.iter().any(
                |mount| mount.contains("source=/tmp/bulkhead-test/drop,target=/drop,type=bind")
            )
        );
        assert!(generated.mounts.iter().all(|mount| {
            !mount.contains("source=/tmp/bulkhead-test/drop,target=/drop,type=bind,readonly")
        }));
    }

    #[test]
    fn rejects_sys_admin_capability() {
        let config = load_inline_config(
            r#"
run_args = ["--cap-add=SYS_ADMIN"]
"#,
        )
        .unwrap();

        assert!(generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).is_err());
    }

    #[test]
    fn rejects_docker_socket_mount() {
        let config = load_inline_config(
            r#"
[[path]]
source = "/var/run/docker.sock"
target = "/var/run/docker.sock"
"#,
        )
        .unwrap();

        assert!(generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).is_err());
    }

    #[test]
    fn workspace_folder_is_normalized() {
        assert_eq!(
            normalize_container_path("/workspace/").unwrap(),
            "/workspace"
        );
    }

    #[test]
    fn audit_preset_keeps_network_caps() {
        let rendered = instantiate_template(AUDIT_PRESET_TOML).unwrap();
        let config: BulkheadConfig = load_inline_config(&rendered).unwrap();
        let generated = generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).unwrap();

        assert_eq!(
            generated.run_args,
            vec!["--cap-add=NET_ADMIN", "--cap-add=NET_RAW"]
        );
    }

    #[test]
    fn minimal_preset_skips_gitconfig_mount() {
        let rendered = instantiate_template(MINIMAL_PRESET_TOML).unwrap();
        let config: BulkheadConfig = load_inline_config(&rendered).unwrap();
        let generated = generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).unwrap();

        assert!(
            generated
                .mounts
                .iter()
                .all(|mount| !mount.contains("/.gitconfig"))
        );
    }

    #[test]
    fn custom_build_paths_render_relative_to_devcontainer_json() {
        let config = load_inline_config(
            r#"
[build]
dockerfile = ".bulkhead/Dockerfile"
context = ".bulkhead"
"#,
        )
        .unwrap();

        let generated = generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).unwrap();

        assert_eq!(generated.build.dockerfile, "../.bulkhead/Dockerfile");
        assert_eq!(generated.build.context, "../.bulkhead");
    }

    #[test]
    fn selected_agents_add_provider_mounts_and_env() {
        let config = load_inline_config(
            r#"
remote_user = "agent"
agents = ["claude", "codex"]
"#,
        )
        .unwrap();

        let generated = generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).unwrap();

        assert_eq!(
            generated.features.get(super::NODE_FEATURE),
            Some(&json!({ "version": "22" }))
        );
        assert!(generated.mounts.iter().any(|mount| {
            mount.contains("target=/home/agent/.claude") && mount.contains("type=volume")
        }));
        assert!(generated.mounts.iter().any(|mount| {
            mount.contains("target=/home/agent/.codex") && mount.contains("type=volume")
        }));
        assert_eq!(
            generated.container_env.get("BULKHEAD_SELECTED_AGENTS"),
            Some(&"claude,codex".to_owned())
        );
        assert_eq!(
            generated.container_env.get("CLAUDE_CONFIG_DIR"),
            Some(&"/home/agent/.claude".to_owned())
        );
        assert_eq!(
            generated.remote_env.get("OPENAI_API_KEY"),
            Some(&"${localEnv:OPENAI_API_KEY:}".to_owned())
        );
        assert_eq!(
            generated.remote_env.get("ANTHROPIC_API_KEY"),
            Some(&"${localEnv:ANTHROPIC_API_KEY:}".to_owned())
        );
        assert_eq!(
            generated.post_create_command,
            Some("bash /workspace/.devcontainer/bulkhead-post-create.sh".to_owned())
        );
    }

    #[test]
    fn duplicate_agents_are_rejected() {
        let config = load_inline_config(
            r#"
agents = ["claude", "claude"]
"#,
        )
        .unwrap();

        assert!(generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).is_err());
    }

    #[test]
    fn reserved_agent_container_env_keys_are_rejected() {
        let config = load_inline_config(
            r#"
agents = ["claude"]

[container_env]
CLAUDE_CONFIG_DIR = "/tmp/claude"
"#,
        )
        .unwrap();

        assert!(generate_devcontainer(Path::new("/tmp/bulkhead-test"), &config).is_err());
    }
}
