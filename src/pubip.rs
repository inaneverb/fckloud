mod address;
mod consensus;
mod error;
mod provider;
mod trust;

pub use self::{consensus::Report, provider::HttpProvider, trust::TrustFactorAuthority};

use {
    self::error::FetchError,
    reqwest::Client,
    std::{net::IpAddr, sync::LazyLock, time::Duration},
    tokio::task::JoinSet,
    tracing::{debug, error},
};

const USER_AGENT: &str = concat!("fckloud/", env!("CARGO_PKG_VERSION"));

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

// One client, one connection pool, one place where a stalled provider dies.
static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("HTTP client with static settings must be constructible")
});

/// Asks every enabled provider where this machine lives and weighs the answers.
pub struct Resolver {
    providers: Vec<HttpProvider>,
    tfa: TrustFactorAuthority,
    confirmations: usize,
}

impl Resolver {
    pub fn new(providers: Vec<HttpProvider>, tfa: TrustFactorAuthority) -> Self {
        assert!(!providers.is_empty());

        let confirmations = tfa.calc_confirmation_number(&providers);
        Self {
            providers,
            tfa,
            confirmations,
        }
    }

    /// Overwrites the confirmation number that would otherwise be derived from
    /// the enabled providers' trust factors.
    pub fn set_confirmations(&mut self, confirmations: usize) -> &mut Self {
        self.confirmations = confirmations;
        self
    }

    /// Polls every provider in parallel, then hands what came back to
    /// [`consensus::decide`].
    pub async fn run(&self) -> Report {
        let reported: Vec<_> = self
            .providers
            .iter()
            .copied()
            .map(|provider| async move { (provider, get_public_ip(provider).await) })
            .collect::<JoinSet<_>>()
            .join_all()
            .await
            .into_iter()
            .filter_map(|(provider, result)| match result {
                Ok(ip_addr) => Some((provider, ip_addr)),
                Err(err) => {
                    error!(
                        %provider,
                        error.type = err.as_error_type(),
                        %err,
                        "provider cannot be used",
                    );
                    None
                }
            })
            .collect();

        let report = consensus::decide(&reported, &self.tfa, self.confirmations);

        for ip_addr in &report.confirmed {
            debug!(?ip_addr, report.confirmations, "address has been confirmed");
        }
        for (ip_addr, bucket) in &report.unconfirmed {
            debug!(
                ?ip_addr,
                bucket, report.confirmations, "address falls short"
            );
        }

        report
    }
}

/// Asks the given [`HttpProvider`] which public IP address it sees us as.
async fn get_public_ip(provider: HttpProvider) -> Result<IpAddr, FetchError> {
    let response = CLIENT
        .request(provider.request_method(), provider.request_uri())
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        return Err(FetchError::HttpStatus(status));
    }

    let body = response.bytes().await?;
    let ip_addr = provider.response_decode(&body)?;

    // A node's ExternalIP that is not routable on the Internet is a lie,
    // no matter how confidently a provider states it.
    if !address::is_public(&ip_addr) {
        return Err(FetchError::NotPublic(ip_addr));
    }

    Ok(ip_addr)
}
