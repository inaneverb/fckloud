use {
    crate::{
        TrustFactorAuthority,
        providers::{HttpProvider, HttpProviders},
        verifier,
    },
    anyhow::Error,
    std::{
        collections::{HashMap, HashSet},
        net::IpAddr,
    },
    tokio::task::JoinSet,
    tracing::{debug, error, warn},
};

/// Manager accumulates all
pub struct Manager {
    providers: HttpProviders,
    tfa: TrustFactorAuthority,
    confirmations: Option<usize>,
}

#[derive(Default)]
pub struct Report {
    pub confirmations: usize,
    pub confirmed: HashSet<IpAddr>,
    pub unconfirmed: HashMap<IpAddr, usize>,
    pub failed: HashMap<HttpProvider, Error>,
}

impl Manager {
    pub fn new(providers: HttpProviders) -> Self {
        Self::new_with_tfa(providers, TrustFactorAuthority::default())
    }

    pub fn new_with_tfa(providers: HttpProviders, tfa: TrustFactorAuthority) -> Self {
        assert!(!providers.is_empty());
        Self {
            providers,
            tfa,
            confirmations: None,
        }
    }

    /// Overwrites default confirmation number that would have been dynamically
    /// calculated basing on the enabled providers' trust factors
    /// with the statically provided.
    pub fn set_confirmations(&mut self, new_confirmation_number: usize) -> &mut Self {
        self.confirmations = Some(new_confirmation_number);
        self
    }

    /// Runs all checks one by one for all the [HttpProvider], this [Manager]
    /// has been created with, writing logs and generating and returning over all report.
    pub async fn run(&self) -> Report {
        let mut rep = Report::default();

        // We are going to calculate default confirmation number
        // even if we have provided by the user.

        rep.confirmations = self.tfa.calc_confirmation_number(&self.providers);

        // We will warn if it's different, because picking
        // the bad confirmation number may lead to disasterous consequences.

        if let Some(provided_confirmations) = self.confirmations
            && provided_confirmations != rep.confirmations
        {
            warn!(
                have = provided_confirmations,
                would_be = rep.confirmations,
                concat!(
                    "custom confirmation number detected; ",
                    "unwise picked such a number may lead to either ",
                    "an inability to reach consensus for a single IP (if the threshold is too high) ",
                    "or result in falsely reported IPs being assigned to the node (if the threshold is too low)",
                ),
            );

            rep.confirmations = provided_confirmations;
        }

        // Do the job, tracking all obtained IP addresses and their buckets
        // in the "unconfirmed" collection. Lately, we will move confirmed IPs out.

        self.providers
            .iter()
            .cloned()
            .map(|provider| async move {
                let result = verifier::get_public_ip(provider).await;
                (provider, result.map_err(|err| (format!("{:#}", err), err)))
            })
            .collect::<JoinSet<_>>()
            .join_all()
            .await
            .into_iter()
            .for_each(|(provider, result)| match result {
                Ok(ip_addr) => {
                    let trust_factor = self.tfa.trust_factor(provider);
                    let bucket = rep.unconfirmed.entry(ip_addr).or_default();

                    *bucket += trust_factor;
                    debug!(
                        ?ip_addr,
                        trust_factor,
                        bucket,
                        ?provider,
                        "confirmation bucket has been increased"
                    );
                }
                Err((err_str, err)) => {
                    rep.failed.insert(provider, err);
                    error!(?provider, err = err_str, "provider cannot be used");
                }
            });

        // Move these addresses that has been confirmed to their corresponding
        // collection also writing logs about it.

        rep.unconfirmed
            .iter()
            .filter(|(_, bucket)| **bucket >= rep.confirmations)
            .for_each(|(ip_addr, bucket)| {
                rep.confirmed.insert(*ip_addr);
                debug!(
                    ?ip_addr,
                    bucket, rep.confirmations, "address has been confirmed!"
                );
            });

        rep.unconfirmed
            .retain(|ip_addr, _| !rep.confirmed.contains(ip_addr));

        rep
    }
}
