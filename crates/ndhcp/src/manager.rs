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
    tracing::{debug, error},
};

/// Manager accumulates all
pub struct Manager {
    providers: HttpProviders,
    tfa: TrustFactorAuthority,
    confirmations: usize,
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

        let confirmations = tfa.calc_confirmation_number(&providers);
        Self {
            providers,
            tfa,
            confirmations,
        }
    }

    /// Overwrites default confirmation number that would have been dynamically
    /// calculated basing on the enabled providers' trust factors
    /// with the statically provided.
    pub fn set_confirmations(&mut self, new_confirmation_number: usize) -> &mut Self {
        self.confirmations = new_confirmation_number;
        self
    }

    /// Runs all checks one by one for all the [HttpProvider], this [Manager]
    /// has been created with, writing logs and generating and returning over all report.
    pub async fn run(&self) -> Report {
        let mut rep = Report::default();
        rep.confirmations = self.confirmations;

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
