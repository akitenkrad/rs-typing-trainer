//! Build metadata embedded by `build.rs`.

use std::sync::LazyLock;

use chrono::DateTime;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_HASH: &str = env!("RS_TT_GIT_HASH");
pub const BUILD_UNIX: &str = env!("RS_TT_BUILD_UNIX");
pub const TARGET: &str = env!("RS_TT_TARGET");
pub const PROFILE: &str = env!("RS_TT_PROFILE");

pub static BUILD_DATE: LazyLock<String> = LazyLock::new(|| {
    BUILD_UNIX
        .parse::<i64>()
        .ok()
        .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        .map(|date_time| date_time.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_owned())
});

static SHORT_VERSION: LazyLock<String> = LazyLock::new(|| short_version_for(GIT_HASH));
static LONG_VERSION: LazyLock<String> = LazyLock::new(|| {
    if GIT_HASH == "unknown" {
        format!("{VERSION} (built {})", build_date())
    } else {
        format!("{VERSION} ({GIT_HASH}, built {})", build_date())
    }
});

pub fn build_date() -> String {
    BUILD_DATE.clone()
}

pub fn short_version() -> &'static str {
    SHORT_VERSION.as_str()
}

pub fn short_version_for(git_hash: &str) -> String {
    if git_hash == "unknown" {
        format!("v{VERSION}")
    } else {
        format!("v{VERSION} · {git_hash}")
    }
}

pub fn long_version() -> &'static str {
    LONG_VERSION.as_str()
}
