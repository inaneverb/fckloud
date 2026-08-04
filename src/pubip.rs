mod address;
mod consensus;
mod error;
mod metrics;
mod provider;
mod trust;

pub use self::{consensus::Report, provider::HttpProvider, trust::TrustFactorAuthority};

use {
    self::error::FetchError,
    reqwest::Client,
    std::{
        net::IpAddr,
        sync::LazyLock,
        time::{Duration, Instant},
    },
    tokio::task::JoinSet,
    tracing::{Span, debug, error, field::Empty, instrument},
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
    #[instrument(name = "pubip.resolve", skip_all, fields(
        fckloud.consensus.threshold = self.confirmations,
        fckloud.consensus.confirmed = Empty,
        fckloud.consensus.unconfirmed = Empty,
    ))]
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

        // Counts, not addresses: what an address is belongs in the log line
        // below, where it is read once, not in a label kept forever.
        Span::current()
            .record("fckloud.consensus.confirmed", report.confirmed.len())
            .record("fckloud.consensus.unconfirmed", report.unconfirmed.len());

        metrics::record_consensus(report.confirmed.len(), report.unconfirmed.len());

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
///
/// The span is named for the HTTP method, as the semantic conventions have it
/// for client spans; `server.address` is what tells the providers apart.
#[instrument(name = "http.request", skip_all, fields(
    otel.kind = "client",
    otel.name = %provider.request_method(),
    otel.status_code = Empty,
    http.request.method = %provider.request_method(),
    http.response.status_code = Empty,
    server.address = %provider,
    url.full = provider.request_uri(),
    error.type = Empty,
))]
async fn get_public_ip(provider: HttpProvider) -> Result<IpAddr, FetchError> {
    let started = Instant::now();
    let result = fetch(provider).await;
    let elapsed = started.elapsed();

    if let Err(err) = &result {
        Span::current()
            .record("otel.status_code", "ERROR")
            .record("error.type", err.as_error_type());
    }

    metrics::record_request(provider, elapsed, result.as_ref().err());
    result
}

async fn fetch(provider: HttpProvider) -> Result<IpAddr, FetchError> {
    let response = CLIENT
        .request(provider.request_method(), provider.request_uri())
        .send()
        .await?;

    let status = response.status();
    Span::current().record("http.response.status_code", status.as_u16());

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
