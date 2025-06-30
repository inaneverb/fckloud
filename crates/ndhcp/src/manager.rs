use std::{net::IpAddr, result::Result as StdResult};

use {
    anyhow::{Context, Result, bail},
    derive_more::{Debug, Display},
    getset::Getters,
    smallvec::SmallVec,
    strum::EnumCount,
    strum_macros::EnumIs,
    tokio::task::JoinSet,
    tracing::{error, instrument, warn},
};

use crate::{address, providers::HttpProvider, verifier};

/// Manager accumulates all
pub(crate) struct Manager {
    items: Vec<Item>,
    providers: SmallVec<[HttpProvider; HttpProvider::COUNT]>,
}

#[derive(Debug, Display, Getters)]
#[display("{}/{}", iface, addr)]
#[debug("{}/{}", iface, addr)]
pub struct Item {
    #[getset(get = "pub")]
    iface: String,

    #[getset(get = "pub")]
    addr: IpAddr,

    confirmations: i32,
}

#[derive(Debug, Display, EnumIs)]
pub enum Status {
    Confirmed,
    Candidate,
    Declined,
}

impl Manager {
    pub(crate) fn new<P>(providers: P) -> Result<Self>
    where
        P: IntoIterator<Item = HttpProvider>,
    {
        let items: Vec<_> = local_ip_address::list_afinet_netifas()
            .with_context(|| format!("cannot obtain network interfaces"))?
            .into_iter()
            .map(|(iface, addr)| Item::new(iface, addr))
            .collect();

        Self::new_with_items(items, providers)
    }

    pub(crate) fn new_with_items<V, P>(items: V, providers: P) -> Result<Self>
    where
        V: IntoIterator<Item = crate::manager::Item>,
        P: IntoIterator<Item = HttpProvider>,
    {
        let mut items = items.into_iter().peekable();
        let mut providers = providers.into_iter().peekable();

        if let None = items.peek() {
            bail!("non-empty list of candidates is required")
        }
        if let None = providers.peek() {
            bail!("non-empty list of providers is required")
        }

        Ok(Self {
            items: items.collect(),
            providers: providers.collect(),
        })
    }

    // Consumes the [Manager] and runs it, reporting for each [Item]
    // whether it can be used as a public IP for the current machine.
    //
    // Threshold defines how many confirmations an IP needs
    // to be considered public.
    // If it's negative or exceeds the number of providers, all must confirm it.
    pub(crate) async fn run(self, mut threshold: i32) -> Vec<(Item, Status)> {
        
        // Threshold may be negative or exceed the provider count, so clamp it.
        // Treat any zero or negative value as requiring all confirmations.

        let n = self.providers.len() as i32;
        if threshold <= 0 || threshold > n {
            threshold = n
        }

        self.items
            .into_iter()
            .map(|item| {
                let providers = self.providers.clone();
                async move { item.check_providers(providers).await }
            })
            .collect::<JoinSet<_>>()
            .join_all()
            .await
            .into_iter()
            .map(|item| (item.status_for(threshold), item))
            .map(|(status, item)| (item, status))
            .collect()
    }
}

impl Item {
    // Creates and returns a new [Item] based on given IP address and iface name.
    pub(crate) fn new(iface: String, addr: IpAddr) -> Self {
        Self {
            iface,
            addr,
            confirmations: 0,
        }
    }

    // // Checks the current [Item] against every [HttpProvider]
    // // yielded by a given iterator.
    // // Each that check is executed in a separated async task.
    // async fn check_providers<P>(mut self, providers: P) -> Self
    // where
    //     P: IntoIterator<Item = HttpProvider>,
    // {
    //     // Discard all previous confirmations if they were.
    //     self.confirmations = 0;

    //     providers
    //         .into_iter()
    //         .map(|provider| async move {
    //             let result = verifier::check_public_ip(provider, self.addr).await;
    //             (provider, result)
    //         })
    //         .collect::<JoinSet<_>>()
    //         .join_all()
    //         .await
    //         .into_iter()
    //         .for_each(|(provider, result)| {
    //             self.confirmations += 1;
    //             self.apply_check_result(result, provider);
    //         });

    //     self
    // }

    async fn check_providers<P>(mut self, providers: P) -> Self
    where
        P: IntoIterator<Item = HttpProvider>,
    {
        // Discard all previous confirmations if they were.
        self.confirmations = 0;

        let local_addr = self.addr;
        let local_addr_kind = address::kind(local_addr);

        // With confirmations starting at 0, this allows:
        // - "-1" for non-public addresses,
        // - "+1" early for valid ones, later adjusted.
        //
        // This models how many providers confirmed the address as public,
        // while enabling negative values for invalid ones
        // to lower the resolved status.
        //
        // Matches behavior of [check_provider].

        if !local_addr_kind.is_public() {
            let result = Err(verifier::Reason::InappropriateAddress(local_addr_kind));
            self.apply_check_result(result, None);
            return self;
        }

        providers
            .into_iter()
            .map(|provider| async move {
                let result = verifier::check_public_ip(provider, local_addr).await;
                (provider, result)
            })
            .collect::<JoinSet<_>>()
            .join_all()
            .await
            .into_iter()
            .for_each(|(provider, result)| {
                self.confirmations += 1;
                self.apply_check_result(result, Some(provider));
            });

        self
    }

    // Checks the current [Item] against the given [HttpProvider].
    #[allow(dead_code)]
    #[deprecated(note = "use check_providers() instead")]
    async fn check_provider<P>(mut self, provider: P) -> Self
    where
        P: Into<HttpProvider>,
    {
        let provider = provider.into();

        let local_addr = self.addr.clone();
        let local_addr_kind = address::kind(local_addr);

        // It's fine to increment confirmations early if the address is valid.
        // This allows a "-1" count for non-public addresses,
        // leading the status resolver to assign an even lower status.
        //
        // Matches behavior of [check_providers].

        let result = if !local_addr_kind.is_public() {
            self.confirmations = 1;
            verifier::check_public_ip(provider, local_addr).await
        } else {
            Err(verifier::Reason::InappropriateAddress(local_addr_kind))
        };

        self.apply_check_result(result, Some(provider));
        self
    }

    // Analyzes the result from [verifier::check_public_ip] and acts accordingly.
    // Decreases the confirmation counter if it is an error.
    #[instrument(name = "check_addr", skip(result))]
    fn apply_check_result(
        &mut self,
        result: StdResult<(), verifier::Reason>,
        provider: Option<HttpProvider>,
    ) {
        if result.is_ok() {
            return;
        }

        // Since confirmations are initially set to the number of providers,
        // we assume all confirmed it.
        // We only decrement if a provider did not actually confirm.
        self.confirmations -= 1;

        use verifier::Reason::*;
        match result.unwrap_err() {
            InappropriateAddress(kind) => {
                warn!("inappropriate (non public) address, kind: {}", kind)
            }
            RemoteUnmatched(real_remote_ip) => {
                warn!("unmatched address, provider returned: {}", real_remote_ip)
            }
            RemoteError(err) => {
                error!("provider cannot be used: {}", err)
            }
        }
    }

    // Reports the [Status] for the some [Item]
    // based on its accumulated confirmations relative to the given threshold.
    fn status_for(&self, threshold: i32) -> Status {
        assert!(threshold >= 1);
        let threshold_candidate = threshold / 2;

        if self.confirmations >= threshold {
            Status::Confirmed
        } else if self.confirmations >= threshold_candidate {
            Status::Candidate
        } else {
            Status::Declined
        }
    }
}
