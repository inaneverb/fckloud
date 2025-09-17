mod manager;
mod providers;
mod trust_factor;

pub mod address;
pub mod verifier;

use smallvec::SmallVec;

use {std::net::IpAddr, strum::VariantArray};

pub use crate::{
    manager::Manager,
    providers::{HttpProvider, HttpProviders},
    trust_factor::TrustFactorAuthority,
};

pub async fn resolve() -> Vec<IpAddr> {
    resolve_by(HttpProvider::VARIANTS).await
}

pub async fn resolve_by(providers: &[HttpProvider]) -> Vec<IpAddr> {
    Manager::new(SmallVec::from_slice(providers))
        .run()
        .await
        .confirmed
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn print_reports() {}
}
