mod manager;
mod providers;

pub mod address;
pub mod verifier;

use {anyhow::Result, strum::IntoEnumIterator};

pub use crate::{
    manager::{Item, Status},
    providers::HttpProvider,
};

pub async fn resolve<N>(threshold: N) -> Result<Vec<(Item, Status)>>
where
    N: Into<i32>,
{
    resolve_by(threshold, HttpProvider::iter()).await
}

pub async fn resolve_by<N, P>(threshold: N, providers: P) -> Result<Vec<(Item, Status)>>
where
    N: Into<i32>,
    P: IntoIterator<Item = HttpProvider>,
{
    let out = manager::Manager::new(providers)?
        .run(threshold.into())
        .await;
    Ok(out)
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn print_reports() {}
}
