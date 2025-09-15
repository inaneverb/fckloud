#![allow(dead_code)]

use std::sync::OnceLock;

// Utility application constants
pub const ENV_PREFIX: &str = "FCKLOUD_";

// Build infromation from vergen via cargo:rustc-env
pub const BUILD_DATE: &str = env!("VERGEN_BUILD_DATE");
pub const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");

// Git information from vergen-gitcl via cargo:rustc-env
pub const GIT_SHA: &str = env!("VERGEN_GIT_SHA");
pub const GIT_BRANCH: &str = env!("VERGEN_GIT_BRANCH");
pub const GIT_COMMIT_TIMESTAMP: &str = env!("VERGEN_GIT_COMMIT_TIMESTAMP");
pub const GIT_DESCRIBE: &str = env!("VERGEN_GIT_DESCRIBE");

// Standard Cargo package info
pub const CARGO_PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CARGO_PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// Returns the short Git SHA (first 7 characters).
pub fn git_sha_short() -> &'static str {
    if GIT_SHA.len() >= 7 {
        &GIT_SHA[..7]
    } else {
        GIT_SHA
    }
}

/// Checks if the build was made from uncommitted changes.
pub fn is_dirty_build() -> bool {
    GIT_DESCRIBE.contains("dirty")
}

/// Returns the string that is shown when CLI is invoked with "--version".
pub fn version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let dirty_flag = match is_dirty_build() {
            true => " (dirty)",
            false => "",
        };
        format!(
            "v{}, git: {}{}, built: {}",
            CARGO_PKG_VERSION,
            git_sha_short(),
            dirty_flag,
            BUILD_DATE
        )
    })
}
