use std::{collections::HashMap, net::IpAddr};

use {
    anyhow::{Error, Result, bail},
    ekacore::traits::Discard,
    smallvec::SmallVec,
    strum::EnumCount,
    tokio::task::JoinSet,
};

use crate::{providers::HttpProvider, verifier};

pub type HttpProviders = SmallVec<[HttpProvider; HttpProvider::COUNT]>;

pub type ItemSucceeded<'a> = (&'a IpAddr, &'a HttpProviders);
pub type ItemErrored<'a> = (&'a HttpProvider, &'a Error);

/// Manager accumulates all
pub struct Manager {
    providers: HttpProviders,
}

pub struct ManagerCompleted {
    providers_num: usize,
    succeeded: HashMap<IpAddr, HttpProviders>,
    errored: HashMap<HttpProvider, Error>,
}

impl Manager {
    pub fn new(providers: &[HttpProvider]) -> Result<Self> {
        if providers.is_empty() {
            bail!("non-empty list of HttpProvider is required")
        }

        Ok(Self {
            providers: SmallVec::from_slice(providers),
        })
    }

    // Consumes the [Manager] and runs it, reporting for each [Item]
    // whether it can be used as a public IP for the current machine.
    //
    // Threshold defines how many confirmations an IP needs
    // to be considered public.
    // If it's negative or exceeds the number of providers, all must confirm it.
    pub async fn run(self) -> ManagerCompleted {
        let mut out = ManagerCompleted::new(self.providers.len());

        self.providers
            .into_iter()
            .map(|provider| async move {
                let result = verifier::get_public_ip(provider).await;
                (provider, result)
            })
            .collect::<JoinSet<_>>()
            .join_all()
            .await
            .into_iter()
            .for_each(|(provider, result)| match result {
                Ok(ip_addr) => out.succeeded.entry(ip_addr).or_default().push(provider),
                Err(err) => out.errored.insert(provider, err).discard(),
            });

        out
    }
}

impl ManagerCompleted {
    fn new(providers_num: usize) -> Self {
        Self {
            providers_num,
            succeeded: HashMap::new(),
            errored: HashMap::new(),
        }
    }

    /// Returns an iterator over successful pairs of IP addresses
    /// and their reporting providers. Panics if [run] has never been called.
    pub fn iter_succeeded(&self) -> impl Iterator<Item = ItemSucceeded> {
        self.succeeded.iter()
    }

    pub fn iter_succeeded_threshold(&self, threshold: i32) -> impl Iterator<Item = ItemSucceeded> {
        let threshold = threshold.clamp(0, self.providers_num as i32);
        self.iter_succeeded()
            .filter(move |(_, providers)| providers.len() >= threshold as usize)
    }

    pub fn iter_errored(&self) -> impl Iterator<Item = ItemErrored> {
        self.errored.iter()
    }
}
