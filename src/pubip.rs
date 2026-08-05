mod address;
mod consensus;
mod error;
mod metrics;
mod provider;
mod ratelimit;
mod set;
mod share;
mod trust;

pub use self::{
    consensus::Report,
    provider::HttpProvider,
    set::{Set, Token, parse_token as parse_provider_token, released},
    share::TrustShare,
    trust::TrustFactorAuthority,
};

use {
    self::error::FetchError,
    anyhow::{Result, ensure},
    humantime::Duration as DisplayedDuration,
    reqwest::Client,
    std::{
        collections::HashMap,
        net::IpAddr,
        sync::{LazyLock, Mutex, MutexGuard, PoisonError},
        time::{Duration, Instant},
    },
    tokio::task::JoinSet,
    tracing::{Span, debug, error, field::Empty, info, instrument, warn},
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

    /// Set only by the deprecated `--confirmations`. Left alone, every round
    /// works out its own threshold from the providers that answered it.
    confirmations: Option<usize>,

    gaps: HashMap<HttpProvider, Duration>,
    honour: ratelimit::Honour,
    asked: Mutex<HashMap<HttpProvider, Instant>>,
}

impl Resolver {
    /// Fails when the share and the enabled providers between them ask for a
    /// threshold nobody could clear, or for one an empty round would clear.
    pub fn new(providers: Vec<HttpProvider>, tfa: TrustFactorAuthority) -> Result<Self> {
        assert!(!providers.is_empty());
        metrics::register(&providers);

        let share = tfa.trust_share();
        ensure!(
            share.is_valid(),
            "a trust share of {share} asks for none of the trust, or for more than all of it",
        );

        let total: usize = providers
            .iter()
            .map(|provider| tfa.trust_factor(*provider))
            .sum();

        let everyone = tfa.calc_confirmation_number(&providers);
        ensure!(
            everyone > 0,
            "a trust share of {share} over a total trust of {total} confirms an address nobody reported",
        );

        Ok(Self {
            providers,
            tfa,
            confirmations: None,
            gaps: HashMap::new(),
            honour: ratelimit::Honour::Limits,
            asked: Mutex::new(HashMap::new()),
        })
    }

    /// Stops honouring the gaps providers ask for. The operator's call, and
    /// their responsibility to whoever is on the other end.
    pub fn set_ignore_rate_limits(&mut self, ignore: bool) -> &mut Self {
        self.honour = if ignore {
            ratelimit::Honour::Nothing
        } else {
            ratelimit::Honour::Limits
        };
        self
    }

    // A poisoned lock here costs one provider one round of pacing, which is
    // cheaper than refusing to resolve at all.
    fn asked(&self) -> MutexGuard<'_, HashMap<HttpProvider, Instant>> {
        self.asked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// States the pool this run will actually ask and what it takes to confirm
    /// an address with it.
    ///
    /// One line at `info` for the pool as a whole, because how much trust an
    /// address needs is the first thing anyone asks when a round did not
    /// confirm what they expected. The providers themselves go one per line,
    /// which is a screenful before anything has happened yet, so they wait for
    /// `debug` and for somebody actually looking for them.
    pub fn announce(&self, pinned: bool) {
        for provider in &self.providers {
            let gap = ratelimit::gap_of(*provider, &self.gaps);

            debug!(
                %provider,
                trust_factor = self.tfa.trust_factor(*provider),
                rate_limit = gap.map(|gap| DisplayedDuration::from(gap).to_string()),
                "provider takes part in consensus",
            );
        }

        let enrolled = self.total_trust();

        info!(
            providers = self.providers.len(),
            trust_total = enrolled,
            trust_share = %self.tfa.trust_share(),
            confirmations = self
                .confirmations
                .unwrap_or_else(|| self.tfa.calc_confirmation_number(&self.providers)),
            confirmations_pinned = self.confirmations.is_some(),
            trust_floor = consensus::floor(enrolled),
            pinned,
            "consensus is set",
        );

        if !pinned {
            debug!(concat!(
                "this selection is not pinned, so an upgrade may add a provider to it; ",
                "name a version instead to keep the pool and its egress fixed",
            ));
        }
    }

    fn total_trust(&self) -> usize {
        self.providers
            .iter()
            .map(|provider| self.tfa.trust_factor(*provider))
            .sum()
    }

    /// Overwrites the gap a provider asks between two requests. Zero lifts the
    /// published limit rather than restoring it.
    pub fn set_rate_limits(
        &mut self,
        gaps: impl IntoIterator<Item = (HttpProvider, Duration)>,
    ) -> &mut Self {
        self.gaps = gaps.into_iter().collect();
        self
    }

    /// Pins the threshold to one number for every round, whoever answered.
    /// Only the deprecated `--confirmations` does this.
    pub fn set_confirmations(&mut self, confirmations: usize) -> &mut Self {
        self.confirmations = Some(confirmations);
        self
    }

    /// Polls every provider in parallel, then hands what came back to
    /// [`consensus::decide`].
    #[instrument(name = "pubip.resolve", skip_all, fields(
        fckloud.consensus.threshold = Empty,
        fckloud.consensus.confirmed = Empty,
        fckloud.consensus.unconfirmed = Empty,
        fckloud.consensus.well_answered = Empty,
    ))]
    pub async fn run(&self) -> Report {
        let now = Instant::now();
        let split = {
            let mut asked = self.asked();
            let split = ratelimit::split(&self.providers, &self.gaps, &asked, now, self.honour);

            for provider in &split.allowed {
                asked.insert(*provider, now);
            }

            split
        };

        for (provider, left) in &split.holding {
            metrics::record_rate_limited(*provider);
            debug!(
                %provider,
                left = DisplayedDuration::from(*left).to_string(),
                "provider is still serving out the gap it asks for",
            );
        }

        let answers = split
            .allowed
            .iter()
            .copied()
            .map(|provider| async move { (provider, get_public_ip(provider).await) })
            .collect::<JoinSet<_>>()
            .join_all()
            .await;

        let mut reported = Vec::with_capacity(answers.len());
        let mut failed = Vec::new();

        for (provider, answer) in answers {
            match answer {
                Ok(ip_addr) => reported.push((provider, ip_addr)),
                Err(err) => failed.push((provider, err)),
            }
        }

        let enrolled = self.total_trust();
        let answered: Vec<HttpProvider> = reported.iter().map(|(provider, _)| *provider).collect();
        let answered_trust: usize = answered
            .iter()
            .map(|provider| self.tfa.trust_factor(*provider))
            .sum();

        let confirmations = self
            .confirmations
            .unwrap_or_else(|| consensus::confirmations_for(&answered, &self.tfa, enrolled));

        let mut report = consensus::decide(&reported, &self.tfa, confirmations);
        report.well_answered = consensus::well_answered(answered_trust, enrolled, &self.tfa);

        if !report.well_answered {
            debug!(
                answered_trust,
                enrolled, "round is degraded, its silence says nothing about the node",
            );
        }

        self.complain_about(&failed, &split.holding, &report);

        // Counts, not addresses: what an address is belongs in the log line
        // below, where it is read once, not in a label kept forever.
        Span::current()
            .record("fckloud.consensus.threshold", report.confirmations)
            .record("fckloud.consensus.confirmed", report.confirmed.len())
            .record("fckloud.consensus.unconfirmed", report.unconfirmed.len())
            .record("fckloud.consensus.well_answered", report.well_answered);

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

    /// Reports the providers that could not answer, at the severity their
    /// silence deserves.
    ///
    /// A provider being unreachable is routine and says nothing on its own:
    /// what matters is whether consensus needed it. If the round confirmed
    /// everything it was going to confirm anyway, the failure cost the node
    /// nothing and an error only teaches the reader to ignore errors.
    /// A provider held back by its own rate limit is not a failure and is never
    /// complained about, but its trust is as absent as a failed one's, so it
    /// counts towards whether the silence cost the round anything.
    fn complain_about(
        &self,
        failed: &[(HttpProvider, FetchError)],
        holding: &[(HttpProvider, Duration)],
        report: &Report,
    ) {
        let trust_of = |provider: &HttpProvider| self.tfa.trust_factor(*provider);

        let missing: usize = failed
            .iter()
            .map(|(provider, _)| trust_of(provider))
            .chain(holding.iter().map(|(provider, _)| trust_of(provider)))
            .sum();

        let mattered = consensus::missing_trust_mattered(report, missing);

        for (provider, err) in failed {
            if mattered {
                error!(%provider, error.type = err.as_error_type(), %err, "provider cannot be used");
            } else {
                warn!(
                    %provider,
                    error.type = err.as_error_type(),
                    %err,
                    "provider cannot be used, consensus did not need it",
                );
            }
        }
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
