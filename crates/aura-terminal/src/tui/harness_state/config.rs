use aura_app::harness_mode_enabled;
use aura_app::ui_contract::HARNESS_AUTH_TOKEN_MIN_LEN;
use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

const HARNESS_RUN_TOKEN_ENV: &str = "AURA_HARNESS_RUN_TOKEN";
const HARNESS_TRANSIENT_ROOT_ENV: &str = "AURA_HARNESS_INSTANCE_TRANSIENT_ROOT";
const COMMAND_SOCKET_ENV: &str = "AURA_TUI_COMMAND_SOCKET";
const UI_STATE_FILE_ENV: &str = "AURA_TUI_UI_STATE_FILE";
const UI_STATE_SOCKET_ENV: &str = "AURA_TUI_UI_STATE_SOCKET";

#[derive(Debug, Clone)]
struct HarnessRuntimeContext {
    token: String,
    transient_root: PathBuf,
}

fn normalize_absolute_path(raw: &Path) -> io::Result<PathBuf> {
    if !raw.is_absolute() {
        return Err(io::Error::other(format!(
            "harness path must be absolute: {}",
            raw.display()
        )));
    }

    let mut normalized = PathBuf::new();
    for component in raw.components() {
        match component {
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(io::Error::other(format!(
                    "harness path may not contain parent traversal: {}",
                    raw.display()
                )));
            }
            Component::Prefix(_) => {
                return Err(io::Error::other(format!(
                    "unsupported harness path prefix: {}",
                    raw.display()
                )));
            }
        }
    }
    Ok(normalized)
}

fn ensure_existing_path_chain_safe(
    root: &Path,
    target: &Path,
    target_must_be_dir: bool,
) -> io::Result<()> {
    if !target.starts_with(root) {
        return Err(io::Error::other(format!(
            "harness path escaped transient root {}: {}",
            root.display(),
            target.display()
        )));
    }

    let mut current = root.to_path_buf();
    let relative = target.strip_prefix(root).map_err(|error| {
        io::Error::other(format!(
            "failed to derive harness-relative path for {} under {}: {error}",
            target.display(),
            root.display()
        ))
    })?;
    if relative.as_os_str().is_empty() {
        let metadata = fs::symlink_metadata(root).map_err(|error| {
            io::Error::other(format!(
                "failed to inspect harness root {}: {error}",
                root.display()
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::other(format!(
                "harness transient root must be a real directory: {}",
                root.display()
            )));
        }
        return Ok(());
    }

    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(io::Error::other(format!(
                    "failed to inspect harness path {}: {error}",
                    current.display()
                )));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(io::Error::other(format!(
                "harness path may not traverse symlinks: {}",
                current.display()
            )));
        }
        let should_be_dir = current != target || target_must_be_dir;
        if should_be_dir && !metadata.is_dir() {
            return Err(io::Error::other(format!(
                "harness path ancestor must be a directory: {}",
                current.display()
            )));
        }
    }
    Ok(())
}

fn harness_runtime_context() -> io::Result<Option<HarnessRuntimeContext>> {
    if !harness_mode_enabled() {
        return Ok(None);
    }

    let token = env::var(HARNESS_RUN_TOKEN_ENV).map_err(|_| {
        io::Error::other(format!(
            "native harness mode requires {HARNESS_RUN_TOKEN_ENV}"
        ))
    })?;
    if token.len() < HARNESS_AUTH_TOKEN_MIN_LEN {
        return Err(io::Error::other(format!(
            "native harness token must be at least {HARNESS_AUTH_TOKEN_MIN_LEN} bytes"
        )));
    }

    let transient_root = env::var_os(HARNESS_TRANSIENT_ROOT_ENV)
        .ok_or_else(|| {
            io::Error::other(format!(
                "native harness mode requires {HARNESS_TRANSIENT_ROOT_ENV}"
            ))
        })
        .and_then(|value| normalize_absolute_path(&PathBuf::from(value)))?;
    let metadata = fs::symlink_metadata(&transient_root).map_err(|error| {
        io::Error::other(format!(
            "failed to inspect harness transient root {}: {error}",
            transient_root.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "harness transient root must be a real directory: {}",
            transient_root.display()
        )));
    }

    Ok(Some(HarnessRuntimeContext {
        token,
        transient_root,
    }))
}

fn configured_harness_path(env_key: &str, target_must_be_dir: bool) -> io::Result<Option<PathBuf>> {
    let Some(context) = harness_runtime_context()? else {
        return Ok(None);
    };
    let Some(raw_path) = env::var_os(env_key) else {
        return Ok(None);
    };

    let path = normalize_absolute_path(&PathBuf::from(raw_path))?;
    if path == context.transient_root || !path.starts_with(&context.transient_root) {
        return Err(io::Error::other(format!(
            "{env_key} must stay under {}",
            context.transient_root.display()
        )));
    }
    ensure_existing_path_chain_safe(&context.transient_root, &path, target_must_be_dir)?;
    Ok(Some(path))
}

pub(crate) fn harness_bridge_enabled() -> io::Result<bool> {
    Ok(harness_runtime_context()?.is_some())
}

pub(crate) fn configured_harness_command_token() -> io::Result<Option<String>> {
    Ok(harness_runtime_context()?.map(|context| context.token))
}

pub(crate) fn configured_command_socket() -> io::Result<Option<PathBuf>> {
    configured_harness_path(COMMAND_SOCKET_ENV, false)
}

pub(crate) fn configured_ui_state_socket() -> io::Result<Option<PathBuf>> {
    configured_harness_path(UI_STATE_SOCKET_ENV, false)
}

pub(crate) fn configured_ui_state_file() -> io::Result<Option<PathBuf>> {
    configured_harness_path(UI_STATE_FILE_ENV, false)
}

pub(crate) fn ensure_private_parent_dir(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::other(format!(
            "harness target must have a parent directory: {}",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(crate) fn reject_symlink_target(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::other(format!(
            "harness target may not be a symlink: {}",
            path.display()
        ))),
        Ok(_) | Err(_) => Ok(()),
    }
}

pub(crate) fn set_private_permissions(path: &Path, mode: u32) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}
