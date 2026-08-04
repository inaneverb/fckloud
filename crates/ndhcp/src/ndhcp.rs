mod manager;
mod providers;
mod trust_factor;

pub mod address;
pub mod verifier;

use {smallvec::SmallVec, std::net::IpAddr};

pub use crate::{
    manager::{Manager, Report},
    providers::{HttpProvider, HttpProviders},
    trust_factor::TrustFactorAuthority,
};

/// Asks the given providers where this machine lives, and returns only the
/// addresses enough of them agreed upon.
pub async fn resolve_by(providers: &[HttpProvider]) -> Vec<IpAddr> {
    Manager::new(SmallVec::from_slice(providers))
        .run()
        .await
        .confirmed
        .into_iter()
        .collect()
}
