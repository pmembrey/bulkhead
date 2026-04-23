use super::workspace;
use super::workspace::{bootstrap_workspace_template_if_missing, select_template_preset};
use crate::cli::CloneCommands;
use crate::config::{Preset, existing_directory};
use crate::prompt::{confirm, is_interactive_terminal};
use crate::system::{
    capture_stdout, capture_stdout_in_dir, ensure_command, run_command, run_command_in_dir,
};
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const BULKHEAD_DIR_NAME: &str = ".bulkhead";
const CLONES_DIR_NAME: &str = "clones";
const MAX_CLONE_NAME_LEN: usize = 255;

pub(crate) fn clone(command: CloneCommands) -> Result<()> {
    match command {
        CloneCommands::Shell {
            name,
            create,
            base,
            branch,
            detach,
            allow_dirty_source,
            no_template,
            preset,
            wizard,
            yes,
        } => shell(ShellCloneOptions {
            name,
            create,
            base,
            branch,
            detach,
            allow_dirty_source,
            no_template,
            preset,
            wizard,
            yes,
        }),
        CloneCommands::List => list(),
        CloneCommands::Remove { name, force } => remove(&name, force),
    }
}

struct ShellCloneOptions {
    name: String,
    create: bool,
    base: Option<String>,
    branch: Option<String>,
    detach: bool,
    allow_dirty_source: bool,
    no_template: bool,
    preset: Option<Preset>,
    wizard: bool,
    yes: bool,
}

struct ManagedCloneDetails {
    branch: Option<String>,
    detached_head: Option<String>,
    dirty: Option<bool>,
}

fn shell(options: ShellCloneOptions) -> Result<()> {
    ensure_command("git")?;
    validate_clone_name(&options.name)?;

    let source_repo_root = source_repo_root()?;
    let clone_path = managed_clone_path(&source_repo_root, &options.name);

    if clone_path.exists() {
        let workspace = existing_managed_clone_directory(&source_repo_root, &clone_path)?;
        ensure_git_repository(&workspace).with_context(|| {
            format!(
                "managed clone `{}` is not a usable Git repository",
                options.name
            )
        })?;
        println!("Using existing managed clone at {}.", workspace.display());
        return workspace::shell(Some(workspace));
    }

    if !options.create {
        if !is_interactive_terminal() {
            bail!(
                "no managed clone named `{}` exists. Re-run with `bulkhead clone shell {} --create` if you want to create it.",
                options.name,
                options.name
            );
        }

        let prompt = format!(
            "No managed clone named `{}` exists. Create it now?",
            options.name
        );
        if !confirm(&prompt)? {
            bail!(
                "no managed clone named `{}` exists. Use `bulkhead clone list` to inspect existing clones.",
                options.name
            );
        }
    }

    prepare_clone_creation(&source_repo_root, options.allow_dirty_source)?;
    create_managed_clone(&source_repo_root, &clone_path, &options)?;
    let workspace = existing_managed_clone_directory(&source_repo_root, &clone_path)?;
    workspace::shell(Some(workspace))
}

fn list() -> Result<()> {
    ensure_command("git")?;
    let source_repo_root = source_repo_root()?;
    ensure_managed_clone_storage_is_not_symlinked(&source_repo_root)?;
    let clone_root = managed_clone_root(&source_repo_root);

    if !clone_root.is_dir() {
        println!("No Bulkhead-managed clones found.");
        return Ok(());
    }

    let mut entries = fs::read_dir(&clone_root)
        .with_context(|| format!("failed to read {}", clone_root.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    if entries.is_empty() {
        println!("No Bulkhead-managed clones found.");
        return Ok(());
    }

    println!("Bulkhead-managed clones:");
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        match inspect_managed_clone(&path) {
            Ok(details) => {
                let head = details
                    .branch
                    .map(|branch| format!("branch {branch}"))
                    .or_else(|| details.detached_head.map(|sha| format!("detached @ {sha}")))
                    .unwrap_or_else(|| "unknown HEAD".to_owned());
                let dirty = match details.dirty {
                    Some(true) => "dirty",
                    Some(false) => "clean",
                    None => "dirty state unavailable",
                };
                println!("  {name} -> {} ({head}, {dirty})", path.display());
            }
            Err(error) => {
                println!("  {name} -> {} (unavailable: {error})", path.display());
            }
        }
    }

    Ok(())
}

fn remove(name: &str, force: bool) -> Result<()> {
    validate_clone_name(name)?;
    let source_repo_root = source_repo_root()?;
    let clone_path = managed_clone_path(&source_repo_root, name);

    if !clone_path.exists() {
        ensure_path_is_not_symlink(&clone_path, "managed clone")?;
        bail!(
            "no managed clone named `{}` exists under {}",
            name,
            managed_clone_root(&source_repo_root).display()
        );
    }

    let clone_path = existing_managed_clone_directory(&source_repo_root, &clone_path)?;
    if !force {
        let prompt = format!(
            "Delete managed clone `{}` at {}?",
            name,
            clone_path.display()
        );
        if !confirm(&prompt)? {
            println!("Aborted.");
            return Ok(());
        }
    }

    fs::remove_dir_all(&clone_path)
        .with_context(|| format!("failed to remove {}", clone_path.display()))?;
    println!(
        "Removed managed clone `{}` at {}.",
        name,
        clone_path.display()
    );
    Ok(())
}

fn create_managed_clone(
    source_repo_root: &Path,
    clone_path: &Path,
    options: &ShellCloneOptions,
) -> Result<()> {
    ensure_managed_clone_path_available(source_repo_root, clone_path)?;
    if let Some(branch) = effective_branch(options) {
        validate_git_branch_name(branch)?;
    }

    if let Some(parent) = clone_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    run_command("git", &build_git_clone_args(source_repo_root, clone_path))?;

    let branch = effective_branch(options);
    if let Some(args) = build_git_checkout_args(options.base.as_deref(), branch, options.detach) {
        run_command_in_dir("git", &args, Some(clone_path))?;
    }

    println!(
        "Created managed clone at {} from {}.",
        clone_path.display(),
        source_repo_root.display()
    );

    if options.no_template {
        return Ok(());
    }

    let preset = select_template_preset(options.preset, options.wizard, options.yes)?;
    bootstrap_workspace_template_if_missing(clone_path, preset)
}

fn prepare_clone_creation(source_repo_root: &Path, allow_dirty_source: bool) -> Result<()> {
    if allow_dirty_source || !source_repo_is_dirty(source_repo_root)? {
        return Ok(());
    }

    let prompt = "The source repository has uncommitted changes. Clone mode starts from committed Git state only. Continue?";
    if !is_interactive_terminal() {
        bail!(
            "{prompt} Re-run with `--allow-dirty-source` if you want to create the clone anyway."
        );
    }

    if confirm(prompt)? {
        return Ok(());
    }

    bail!("aborted because the source repository has uncommitted changes")
}

fn source_repo_is_dirty(source_repo_root: &Path) -> Result<bool> {
    Ok(
        !capture_stdout_in_dir("git", &["status", "--short"], Some(source_repo_root))?
            .trim()
            .is_empty(),
    )
}

fn inspect_managed_clone(clone_path: &Path) -> Result<ManagedCloneDetails> {
    ensure_git_repository(clone_path)?;

    let branch = command_stdout_in_dir(
        "git",
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        clone_path,
    )?
    .filter(|output| !output.trim().is_empty());

    let detached_head = if branch.is_none() {
        Some(capture_stdout_in_dir(
            "git",
            &["rev-parse", "--short", "HEAD"],
            Some(clone_path),
        )?)
    } else {
        None
    };

    let dirty = Some(
        !capture_stdout_in_dir("git", &["status", "--short"], Some(clone_path))?
            .trim()
            .is_empty(),
    );

    Ok(ManagedCloneDetails {
        branch,
        detached_head,
        dirty,
    })
}

fn source_repo_root() -> Result<PathBuf> {
    existing_directory(Some(PathBuf::from(
        capture_stdout("git", &["rev-parse", "--show-toplevel"])
            .context("`bulkhead clone` must be run inside a Git repository")?,
    )))
}

fn ensure_git_repository(path: &Path) -> Result<()> {
    capture_stdout_in_dir("git", &["rev-parse", "--show-toplevel"], Some(path)).map(|_| ())
}

fn managed_clone_root(source_repo_root: &Path) -> PathBuf {
    source_repo_root
        .join(BULKHEAD_DIR_NAME)
        .join(CLONES_DIR_NAME)
}

fn managed_clone_path(source_repo_root: &Path, name: &str) -> PathBuf {
    managed_clone_root(source_repo_root).join(name)
}

fn existing_managed_clone_directory(source_repo_root: &Path, clone_path: &Path) -> Result<PathBuf> {
    ensure_managed_clone_storage_is_not_symlinked(source_repo_root)?;
    ensure_path_is_not_symlink(clone_path, "managed clone")?;
    existing_directory(Some(clone_path.to_path_buf()))
}

fn ensure_managed_clone_path_available(source_repo_root: &Path, clone_path: &Path) -> Result<()> {
    ensure_managed_clone_storage_is_not_symlinked(source_repo_root)?;

    match fs::symlink_metadata(clone_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "managed clone must not be a symlink: {}",
                clone_path.display()
            )
        }
        Ok(_) => bail!(
            "managed clone path already exists: {}",
            clone_path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to inspect {}", clone_path.display()))
        }
    }
}

fn ensure_managed_clone_storage_is_not_symlinked(source_repo_root: &Path) -> Result<()> {
    ensure_path_is_not_symlink(
        &source_repo_root.join(BULKHEAD_DIR_NAME),
        "Bulkhead state directory",
    )?;
    ensure_path_is_not_symlink(&managed_clone_root(source_repo_root), "managed clone root")
}

fn ensure_path_is_not_symlink(path: &Path, description: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("{description} must not be a symlink: {}", path.display())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

fn validate_clone_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("managed clone name must be between 1 and 255 characters");
    }

    if name.len() > MAX_CLONE_NAME_LEN {
        bail!("managed clone name must be between 1 and 255 characters");
    }

    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        bail!("managed clone names may only use ASCII letters, digits, `-`, `_`, and `.`");
    }

    if name == "." || name == ".." {
        bail!("managed clone name must not be `.` or `..`");
    }

    if name.starts_with('.') {
        bail!("managed clone names must not start with `.`");
    }

    if name.contains("..") {
        bail!("managed clone names must not contain `..`");
    }

    let path = Path::new(name);
    if path.components().count() != 1
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("managed clone names must be a single path component");
    }

    Ok(())
}

fn effective_branch(options: &ShellCloneOptions) -> Option<&str> {
    if options.detach {
        None
    } else {
        options.branch.as_deref().or(Some(options.name.as_str()))
    }
}

fn validate_git_branch_name(branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["check-ref-format", "--branch", branch])
        .output()
        .with_context(|| format!("failed to validate Git branch name `{branch}`"))?;

    if output.status.success() {
        return Ok(());
    }

    bail!(
        "`{branch}` is not a valid Git branch name. Use a different managed clone name, pass `--branch <name>`, or use `--detach`."
    )
}

fn build_git_clone_args(source_repo_root: &Path, clone_path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("clone"),
        OsString::from("--no-local"),
        OsString::from("--no-hardlinks"),
        source_repo_root.as_os_str().to_os_string(),
        clone_path.as_os_str().to_os_string(),
    ]
}

fn build_git_checkout_args(
    base: Option<&str>,
    branch: Option<&str>,
    detach: bool,
) -> Option<Vec<OsString>> {
    if detach {
        let mut args = vec![OsString::from("checkout"), OsString::from("--detach")];
        if let Some(base) = base {
            args.push(OsString::from(base));
        }
        return Some(args);
    }

    if let Some(branch) = branch {
        let mut args = vec![
            OsString::from("checkout"),
            OsString::from("-B"),
            OsString::from(branch),
        ];
        if let Some(base) = base {
            args.push(OsString::from(base));
        }
        return Some(args);
    }

    base.map(|base| vec![OsString::from("checkout"), OsString::from(base)])
}

fn command_stdout_in_dir(program: &str, args: &[&str], directory: &Path) -> Result<Option<String>> {
    let output = Command::new(program)
        .args(args)
        .current_dir(directory)
        .output()
        .with_context(|| format!("failed to run {}", render_command(program, args)))?;

    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        ));
    }

    if output.stdout.is_empty() && output.stderr.is_empty() {
        return Ok(None);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.trim().is_empty() {
        return Ok(None);
    }

    bail!(
        "command failed: {}: {}",
        render_command(program, args),
        stderr.trim()
    )
}

fn render_command(program: &str, args: &[&str]) -> String {
    let mut rendered = vec![program.to_owned()];
    rendered.extend(args.iter().map(|arg| arg.to_string()));
    rendered.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        build_git_checkout_args, build_git_clone_args, existing_managed_clone_directory,
        managed_clone_path, validate_clone_name, validate_git_branch_name,
    };
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn clone_args_force_independent_local_clone() {
        let args = build_git_clone_args(
            Path::new("/repo"),
            Path::new("/repo/.bulkhead/clones/feature-x"),
        );

        assert_eq!(
            args,
            vec![
                OsString::from("clone"),
                OsString::from("--no-local"),
                OsString::from("--no-hardlinks"),
                OsString::from("/repo"),
                OsString::from("/repo/.bulkhead/clones/feature-x"),
            ]
        );
    }

    #[test]
    fn checkout_args_create_named_branch_from_base() {
        let args = build_git_checkout_args(Some("origin/main"), Some("feature-x"), false).unwrap();

        assert_eq!(
            args,
            vec![
                OsString::from("checkout"),
                OsString::from("-B"),
                OsString::from("feature-x"),
                OsString::from("origin/main"),
            ]
        );
    }

    #[test]
    fn checkout_args_support_detached_head() {
        let args = build_git_checkout_args(Some("HEAD~1"), None, true).unwrap();

        assert_eq!(
            args,
            vec![
                OsString::from("checkout"),
                OsString::from("--detach"),
                OsString::from("HEAD~1"),
            ]
        );
    }

    #[test]
    fn managed_clone_path_is_nested_under_bulkhead_root() {
        let path = managed_clone_path(Path::new("/repo"), "feature-x");

        assert_eq!(path, Path::new("/repo/.bulkhead/clones/feature-x"));
    }

    #[test]
    fn git_branch_validation_rejects_names_checkout_cannot_create() {
        assert!(validate_git_branch_name("feature-x").is_ok());
        assert!(validate_git_branch_name("-feature").is_err());
        assert!(validate_git_branch_name("feature.").is_err());
    }

    #[test]
    fn clone_name_validation_rejects_unsafe_values() {
        assert!(validate_clone_name("feature-x").is_ok());
        assert!(validate_clone_name("feature_x.1").is_ok());
        assert!(validate_clone_name(&"a".repeat(255)).is_ok());
        assert!(validate_clone_name("").is_err());
        assert!(validate_clone_name(&"a".repeat(256)).is_err());
        assert!(validate_clone_name(".").is_err());
        assert!(validate_clone_name("..").is_err());
        assert!(validate_clone_name(".feature-x").is_err());
        assert!(validate_clone_name("feature..x").is_err());
        assert!(validate_clone_name("../../../etc").is_err());
        assert!(validate_clone_name("../feature-x").is_err());
        assert!(validate_clone_name("feature/x").is_err());
        assert!(validate_clone_name("feature x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn managed_clone_directory_rejects_symlink_entries() {
        use std::os::unix::fs::symlink;

        let root = unique_test_dir("bulkhead-clone-symlink");
        let source_repo = root.join("source");
        let clone_root = source_repo.join(".bulkhead").join("clones");
        let external = root.join("external");
        fs::create_dir_all(&clone_root).unwrap();
        fs::create_dir_all(&external).unwrap();

        let clone_path = clone_root.join("feature-x");
        symlink(&external, &clone_path).unwrap();

        let error = existing_managed_clone_directory(&source_repo, &clone_path)
            .unwrap_err()
            .to_string();
        assert!(error.contains("managed clone must not be a symlink"));

        let _ = fs::remove_dir_all(root);
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }
}
