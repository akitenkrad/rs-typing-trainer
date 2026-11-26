use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    emit_git_rerun_paths(&manifest_dir);

    println!("cargo:rustc-env=RS_TT_GIT_HASH={}", git_hash(&manifest_dir));
    println!("cargo:rustc-env=RS_TT_BUILD_UNIX={}", build_unix());
    println!(
        "cargo:rustc-env=RS_TT_TARGET={}",
        env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "cargo:rustc-env=RS_TT_PROFILE={}",
        env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned())
    );
}

fn git_hash(manifest_dir: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(manifest_dir)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let hash = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if hash.is_empty() { "unknown".to_owned() } else { hash }
        }
        _ => "unknown".to_owned(),
    }
}

fn build_unix() -> String {
    if let Ok(epoch) = env::var("SOURCE_DATE_EPOCH")
        && let Ok(epoch) = epoch.trim().parse::<i64>()
    {
        return epoch.to_string();
    }

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

fn emit_git_rerun_paths(manifest_dir: &Path) {
    let Some(git_dir) = resolve_git_dir(manifest_dir) else {
        return;
    };
    let common_dir = resolve_common_dir(&git_dir);
    let head = git_dir.join("HEAD");

    println!("cargo:rerun-if-changed={}", head.display());
    if let Ok(contents) = fs::read_to_string(&head)
        && let Some(reference) = contents.trim().strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed={}", common_dir.join(reference).display());
    }
    println!("cargo:rerun-if-changed={}", common_dir.join("packed-refs").display());
}

fn resolve_git_dir(manifest_dir: &Path) -> Option<PathBuf> {
    let dot_git = manifest_dir.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }

    let contents = fs::read_to_string(&dot_git).ok()?;
    let path = contents.trim().strip_prefix("gitdir: ")?;
    let path = PathBuf::from(path);
    Some(if path.is_absolute() { path } else { manifest_dir.join(path) })
}

fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let Ok(contents) = fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_owned();
    };
    let common_dir = PathBuf::from(contents.trim());
    if common_dir.is_absolute() {
        common_dir
    } else {
        git_dir.join(common_dir)
    }
}
