use std::sync::OnceLock;

// Utility application constants
pub const ENV_PREFIX: &str = "FCKLOUD_";

// Build information from vergen via cargo:rustc-env
const BUILD_DATE: &str = env!("VERGEN_BUILD_DATE");

// Git information from vergen-gitcl via cargo:rustc-env
const GIT_SHA: &str = env!("VERGEN_GIT_SHA");
const GIT_DESCRIBE: &str = env!("VERGEN_GIT_DESCRIBE");

// Standard Cargo package info
const CARGO_PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns the short Git SHA (first 7 characters).
fn git_sha_short() -> &'static str {
    match GIT_SHA.len() {
        7.. => &GIT_SHA[..7],
        _ => GIT_SHA,
    }
}

/// Checks if the build was made from uncommitted changes.
fn is_dirty_build() -> bool {
    GIT_DESCRIBE.contains("dirty")
}

/// Returns the string that is shown when CLI is invoked with "--version".
pub fn version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let dirty_flag = if is_dirty_build() { " (dirty)" } else { "" };
        format!(
            "v{}, git: {}{}, built: {}",
            CARGO_PKG_VERSION,
            git_sha_short(),
            dirty_flag,
            BUILD_DATE
        )
    })
}
