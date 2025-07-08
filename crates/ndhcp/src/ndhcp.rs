mod manager;
mod providers;

pub mod address;
pub mod verifier;

use {
    anyhow::Result, 
    std::net::IpAddr, 
    strum::VariantArray,
};

pub use crate::{
    manager::{ItemErrored, ItemSucceeded, Manager, ManagerCompleted},
    providers::HttpProvider,
};

pub async fn resolve(threshold: i32) -> Result<Vec<IpAddr>> {
    resolve_by(HttpProvider::VARIANTS, threshold).await
}

pub async fn resolve_by(providers: &[HttpProvider], threshold: i32) -> Result<Vec<IpAddr>> {
    Ok(manager::Manager::new(providers)?
        .run()
        .await
        .iter_succeeded_threshold(threshold)
        .map(|(addr, _)| addr.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn print_reports() {}
}
