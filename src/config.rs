use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

pub(crate) const AGENT_PRESET_TOML: &str = include_str!("../templates/bulkhead.toml");
pub(crate) const AUDIT_PRESET_TOML: &str = include_str!("../templates/bulkhead-audit.toml");
pub(crate) const MINIMAL_PRESET_TOML: &str = include_str!("../templates/bulkhead-minimal.toml");
const TEMPLATE_REMOTE_USER_PLACEHOLDER: &str = "__BULKHEAD_REMOTE_USER__";
const CONFIG_FILE_NAME: &str = "bulkhead.toml";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BulkheadConfig {
    #[serde(default = "default_name")]
    pub(crate) name: String,
    #[serde(default = "default_workspace_folder")]
    pub(crate) workspace_folder: String,
    #[serde(default = "default_remote_user")]
    pub(crate) remote_user: String,
    #[serde(default)]
    pub(crate) build: BuildConfig,
    #[serde(default)]
    pub(crate) git: GitConfigFeature,
    #[serde(default = "default_features")]
    pub(crate) features: Vec<String>,
    #[serde(default = "default_run_args")]
    pub(crate) run_args: Vec<String>,
    #[serde(default = "default_build_args")]
    pub(crate) build_args: BTreeMap<String, String>,
    #[serde(default = "default_container_env")]
    pub(crate) container_env: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) volume: Vec<VolumeMountConfig>,
    #[serde(default)]
    pub(crate) path: Vec<PathMountConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BuildConfig {
    #[serde(default = "default_build_dockerfile")]
    pub(crate) dockerfile: String,
    #[serde(default = "default_build_context")]
    pub(crate) context: String,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            dockerfile: default_build_dockerfile(),
            context: default_build_context(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct GitConfigFeature {
    #[serde(default)]
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct VolumeMountConfig {
    pub(crate) name: String,
    pub(crate) target: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PathMountConfig {
    pub(crate) source: String,
    pub(crate) target: String,
    #[serde(default)]
    pub(crate) access: MountAccess,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MountAccess {
    #[default]
    Ro,
    Rw,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub(crate) enum Preset {
    Agent,
    Audit,
    Minimal,
}

pub(crate) fn load_bulkhead_config(workspace: &Path) -> Result<BulkheadConfig> {
    let path = config_path(workspace);
    let contents =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;

    load_inline_config(&contents).with_context(|| format!("failed to parse {}", path.display()))
}

pub(crate) fn load_inline_config(contents: &str) -> Result<BulkheadConfig> {
    toml::from_str(contents).context("failed to parse inline bulkhead config")
}

pub(crate) fn load_bulkhead_document(workspace: &Path) -> Result<DocumentMut> {
    let path = config_path(workspace);
    let contents =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;

    contents
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse {}", path.display()))
}

pub(crate) fn write_bulkhead_document(workspace: &Path, document: &DocumentMut) -> Result<()> {
    let rendered = document.to_string();
    load_inline_config(&rendered).context("failed to validate generated bulkhead config")?;

    let path = config_path(workspace);
    fs::write(&path, rendered).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn upsert_path_mount_in_document(
    document: &mut DocumentMut,
    source: &str,
    target: &str,
    access: MountAccess,
) -> Result<()> {
    let paths = ensure_path_mounts_array(document)?;

    if let Some(table) = paths
        .iter_mut()
        .find(|table| table["target"].as_str() == Some(target))
    {
        table["source"] = value(source);
        table["access"] = value(access.as_str());
    } else {
        let mut table = Table::new();
        table["source"] = value(source);
        table["target"] = value(target);
        table["access"] = value(access.as_str());
        paths.push(table);
    }

    Ok(())
}

pub(crate) fn remove_path_mount_in_document(
    document: &mut DocumentMut,
    needle: &str,
) -> Result<bool> {
    let Some(item) = document.as_table_mut().get_mut("path") else {
        return Ok(false);
    };

    let paths = item
        .as_array_of_tables_mut()
        .context("`path` must be declared as [[path]] entries")?;
    let original_len = paths.iter().count();
    paths.retain(|table| {
        table["target"].as_str() != Some(needle) && table["source"].as_str() != Some(needle)
    });
    let removed = paths.iter().count() != original_len;

    if paths.is_empty() {
        document.remove("path");
    }

    Ok(removed)
}

pub(crate) fn ensure_path_mounts_array(document: &mut DocumentMut) -> Result<&mut ArrayOfTables> {
    let item = document["path"].or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
    item.as_array_of_tables_mut()
        .context("`path` must be declared as [[path]] entries")
}

pub(crate) fn gitconfig_target(remote_user: &str) -> String {
    if remote_user == "root" {
        "/root/.gitconfig".to_owned()
    } else {
        format!("/home/{remote_user}/.gitconfig")
    }
}

pub(crate) fn resolve_mount_source(workspace: &Path, source: &str) -> Result<String> {
    if source.contains("${") {
        return Ok(source.to_owned());
    }

    let home_dir = home_dir()?;

    if let Some(path) = resolve_plain_host_path(workspace, source, &home_dir) {
        return Ok(path.to_string_lossy().into_owned());
    }

    bail!("unable to resolve mount source {source}")
}

pub(crate) fn resolve_plain_host_path(
    workspace: &Path,
    source: &str,
    home_dir: &Path,
) -> Option<PathBuf> {
    if source == "~" {
        return Some(home_dir.to_path_buf());
    }

    if let Some(rest) = source.strip_prefix("~/") {
        return Some(home_dir.join(rest));
    }

    let source_path = Path::new(source);
    if source_path.is_absolute() {
        return Some(source_path.to_path_buf());
    }

    if source.contains("${") {
        return None;
    }

    Some(workspace.join(source_path))
}

pub(crate) fn resolve_path_for_policy_checks(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }

    if let Some(parent) = path.parent() {
        if let Ok(canonical_parent) = fs::canonicalize(parent) {
            if let Some(file_name) = path.file_name() {
                return canonical_parent.join(file_name);
            }

            return canonical_parent;
        }
    }

    path.to_path_buf()
}

pub(crate) fn is_docker_socket_path(path: &Path) -> bool {
    let candidate = resolve_path_for_policy_checks(path);

    ["/var/run/docker.sock", "/run/docker.sock"]
        .into_iter()
        .map(Path::new)
        .map(resolve_path_for_policy_checks)
        .any(|socket| candidate == socket)
}

pub(crate) fn sanitize_volume_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect();

    sanitized.trim_matches('-').to_owned()
}

pub(crate) fn existing_directory(path: Option<PathBuf>) -> Result<PathBuf> {
    let path = path.unwrap_or(std::env::current_dir().context("failed to read current directory")?);
    let canonical = fs::canonicalize(&path)
        .with_context(|| format!("failed to resolve directory {}", path.display()))?;

    if !canonical.is_dir() {
        bail!("{} is not a directory", canonical.display());
    }

    Ok(canonical)
}

pub(crate) fn workspace_path(path: Option<PathBuf>) -> Result<PathBuf> {
    existing_directory(path)
}

pub(crate) fn config_path(workspace: &Path) -> PathBuf {
    workspace.join(CONFIG_FILE_NAME)
}

pub(crate) fn home_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home))
}

pub(crate) fn detected_username() -> Option<String> {
    for key in ["BULKHEAD_REMOTE_USER", "USER", "LOGNAME"] {
        if let Some(value) = std::env::var_os(key) {
            let value = value.to_string_lossy().trim().to_owned();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    None
}

pub(crate) fn instantiate_template(contents: &str) -> Result<String> {
    let mut document = contents
        .parse::<DocumentMut>()
        .context("failed to parse embedded Bulkhead template")?;
    let remote_user = if document["remote_user"].as_str() == Some(TEMPLATE_REMOTE_USER_PLACEHOLDER)
    {
        default_remote_user()
    } else {
        document["remote_user"]
            .as_str()
            .unwrap_or(TEMPLATE_REMOTE_USER_PLACEHOLDER)
            .to_owned()
    };
    document["remote_user"] = value(remote_user);
    Ok(document.to_string())
}

pub(crate) fn resolve_mount_access(access: Option<MountAccess>, rw: bool) -> MountAccess {
    if rw {
        MountAccess::Rw
    } else {
        access.unwrap_or(MountAccess::Ro)
    }
}

pub(crate) fn resolve_workspace_config_path(workspace: &Path, raw: &str) -> Result<PathBuf> {
    if raw.trim().is_empty() {
        bail!("workspace-relative config paths must not be empty");
    }

    if raw.contains("${") || raw == "~" || raw.starts_with("~/") {
        bail!("workspace-relative config paths may not use variables or home-directory expansion");
    }

    let raw_path = Path::new(raw);
    if raw_path.is_absolute() {
        bail!("workspace-relative config paths must not be absolute: {raw}");
    }

    let mut resolved = workspace.to_path_buf();
    for component in raw_path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => resolved.push(segment),
            Component::ParentDir => {
                if resolved == workspace {
                    bail!("workspace-relative config path escapes the workspace: {raw}");
                }
                resolved.pop();
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("workspace-relative config paths must not be absolute: {raw}");
            }
        }
    }

    Ok(resolved)
}

pub(crate) fn devcontainer_relative_path(workspace: &Path, target: &Path) -> Result<String> {
    let target = target
        .strip_prefix(workspace)
        .with_context(|| format!("{} is outside the workspace", target.display()))?;

    let devcontainer_dir = Path::new(".devcontainer");
    if target == devcontainer_dir {
        return Ok(".".to_owned());
    }

    if let Ok(stripped) = target.strip_prefix(devcontainer_dir) {
        if stripped.as_os_str().is_empty() {
            return Ok(".".to_owned());
        }

        return Ok(stripped.to_string_lossy().into_owned());
    }

    Ok(format!("../{}", target.to_string_lossy()))
}

impl Preset {
    pub(crate) const fn choices() -> &'static [Preset] {
        &[Preset::Agent, Preset::Audit, Preset::Minimal]
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Preset::Agent => "agent",
            Preset::Audit => "audit",
            Preset::Minimal => "minimal",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Preset::Agent => {
                "Safe default for local agent work: current directory is writable in-container, host access stays limited to read-only git identity."
            }
            Preset::Audit => {
                "Like agent, but enables NET_ADMIN and NET_RAW for network inspection and firewall tooling."
            }
            Preset::Minimal => {
                "Only the current directory and a history volume. No extra host binds beyond Bulkhead's managed files."
            }
        }
    }

    pub(crate) fn template(self) -> &'static str {
        match self {
            Preset::Agent => AGENT_PRESET_TOML,
            Preset::Audit => AUDIT_PRESET_TOML,
            Preset::Minimal => MINIMAL_PRESET_TOML,
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Preset> {
        match value.trim().to_ascii_lowercase().as_str() {
            "agent" => Some(Preset::Agent),
            "audit" => Some(Preset::Audit),
            "minimal" => Some(Preset::Minimal),
            _ => None,
        }
    }
}

impl MountAccess {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            MountAccess::Ro => "ro",
            MountAccess::Rw => "rw",
        }
    }
}

fn default_name() -> String {
    "Bulkhead Agent Sandbox".to_owned()
}

fn default_workspace_folder() -> String {
    "/workspace".to_owned()
}

fn default_build_dockerfile() -> String {
    ".devcontainer/Dockerfile".to_owned()
}

fn default_build_context() -> String {
    ".devcontainer".to_owned()
}

fn default_remote_user() -> String {
    detected_username().unwrap_or_else(|| "vscode".to_owned())
}

fn default_features() -> Vec<String> {
    vec!["ghcr.io/devcontainers/features/github-cli:1".to_owned()]
}

fn default_run_args() -> Vec<String> {
    Vec::new()
}

fn default_build_args() -> BTreeMap<String, String> {
    BTreeMap::from([("TZ".to_owned(), "${localEnv:TZ:UTC}".to_owned())])
}

fn default_container_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("NPM_CONFIG_AUDIT".to_owned(), "true".to_owned()),
        ("NPM_CONFIG_FUND".to_owned(), "false".to_owned()),
        ("NPM_CONFIG_IGNORE_SCRIPTS".to_owned(), "true".to_owned()),
        (
            "NPM_CONFIG_MINIMUM_RELEASE_AGE".to_owned(),
            "1440".to_owned(),
        ),
        ("NPM_CONFIG_SAVE_EXACT".to_owned(), "true".to_owned()),
        ("NPM_CONFIG_UPDATE_NOTIFIER".to_owned(), "false".to_owned()),
        ("PIP_DISABLE_PIP_VERSION_CHECK".to_owned(), "1".to_owned()),
        ("PYTHONDONTWRITEBYTECODE".to_owned(), "1".to_owned()),
        ("UV_LINK_MODE".to_owned(), "copy".to_owned()),
    ])
}

#[cfg(test)]
mod tests {
    use super::{
        AGENT_PRESET_TOML, AUDIT_PRESET_TOML, BulkheadConfig, MINIMAL_PRESET_TOML, MountAccess,
        Preset, instantiate_template, load_inline_config, resolve_mount_access,
        upsert_path_mount_in_document,
    };
    use crate::devcontainer::validate_config;
    use std::path::Path;
    #[cfg(unix)]
    use std::{
        fs,
        os::unix::fs::symlink,
        time::{SystemTime, UNIX_EPOCH},
    };
    use toml_edit::DocumentMut;

    #[test]
    fn path_edit_preserves_template_comments() {
        let rendered = instantiate_template(AGENT_PRESET_TOML).unwrap();
        let mut document = rendered.parse::<DocumentMut>().unwrap();

        upsert_path_mount_in_document(&mut document, "~/drop", "/drop", MountAccess::Rw).unwrap();
        let updated = document.to_string();

        assert!(updated.contains("The managed `[git]` block above mounts your host"));
        assert!(updated.contains("[[path]]"));
        assert!(updated.contains("source = \"~/drop\""));
    }

    #[test]
    fn template_instantiation_uses_placeholder_remote_user() {
        let rendered = instantiate_template(AGENT_PRESET_TOML).unwrap();
        let config: BulkheadConfig = load_inline_config(&rendered).unwrap();

        assert_ne!(config.remote_user, "__BULKHEAD_REMOTE_USER__");
    }

    #[test]
    fn rw_flag_overrides_default_mount_access() {
        assert_eq!(resolve_mount_access(None, true), MountAccess::Rw);
        assert_eq!(
            resolve_mount_access(Some(MountAccess::Ro), false),
            MountAccess::Ro
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_rw_mount_that_resolves_to_home_via_symlink() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("bulkhead-symlink-test-{unique}"));
        fs::create_dir_all(&workspace).unwrap();

        let home = std::env::var_os("HOME").unwrap();
        symlink(Path::new(&home), workspace.join("home-link")).unwrap();

        let config = load_inline_config(
            r#"
[[path]]
source = "home-link"
target = "/escape"
access = "rw"
"#,
        )
        .unwrap();

        let result = validate_config(&workspace, &config);

        let _ = fs::remove_file(workspace.join("home-link"));
        let _ = fs::remove_dir(&workspace);

        assert!(result.is_err());
    }

    #[test]
    fn preset_round_trips() {
        assert_eq!(Preset::from_str("agent"), Some(Preset::Agent));
        assert_eq!(Preset::from_str("audit"), Some(Preset::Audit));
        assert_eq!(Preset::from_str("minimal"), Some(Preset::Minimal));
        assert_eq!(Preset::choices().len(), 3);
        let _ = AUDIT_PRESET_TOML;
        let _ = MINIMAL_PRESET_TOML;
    }
}
