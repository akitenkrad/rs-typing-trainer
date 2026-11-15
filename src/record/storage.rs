//! Storage-path selection and one-time copying from the v1 location.

use std::path::{Path, PathBuf};

use crate::config::Config;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoragePlan {
    pub path: PathBuf,
    pub legacy_path: Option<PathBuf>,
    pub copy_legacy: bool,
}

/// Resolve paths without touching the filesystem.
pub fn storage_plan(home: &Path, config: &Config) -> StoragePlan {
    if let Some(path) = config.storage.db_path.as_ref().or(config.metrics.db_path.as_ref()) {
        return StoragePlan {
            path: path.clone(),
            legacy_path: None,
            copy_legacy: false,
        };
    }
    StoragePlan {
        path: home.join(".local").join("share").join("rs-typing-trainer").join("sessions.db"),
        legacy_path: Some(home.join(".cache").join("rs-typing-trainer").join("sessions.db")),
        copy_legacy: true,
    }
}

/// Copy the old database when and only when the plan selects the default new
/// location, the old file exists, and the new file does not.
pub fn prepare_storage(plan: &StoragePlan) -> std::io::Result<bool> {
    let Some(old) = plan.legacy_path.as_ref().filter(|_| plan.copy_legacy) else {
        return Ok(false);
    };
    if plan.path.exists() || !old.exists() {
        return Ok(false);
    }
    if let Some(parent) = plan.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(old, &plan.path)?;
    Ok(true)
}
