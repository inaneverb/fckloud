use {
    crate::{
        Executable, args,
        build_info::ENV_PREFIX,
        node::{AddrStatus, Manager as NodeManager, Removal},
        pubip::{Resolver, TrustShare},
        telemetry::meter,
    },
    anyhow::{Context as _, Error, Result, bail, ensure},
    clap::Args as ClapArgs,
    const_format::concatcp,
    humantime::{Duration as DisplayedDuration, parse_duration},
    opentelemetry::{KeyValue, metrics::Histogram},
    std::{sync::LazyLock, time::Duration as StdDuration},
    tokio::time::{Instant, sleep},
    tracing::{debug, error, info, instrument, warn},
};

// Milliseconds, matching the other two histograms and the SDK's default
// bucket boundaries. Read against the interval, this says how much of the
// budget a tick actually spends.
static TICK_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    meter()
        .f64_histogram("fckloud.tick.duration")
        .with_unit("ms")
        .with_description("How long a whole tick took, and whether it finished")
        .build()
});

/// The list of options for the "run" command.
#[derive(ClapArgs)]
pub struct Args {
    /// Node name the operator is controlling which
    #[arg(
        short,
        long,
        value_name("NAME"),
        help_heading = "Node",
        env(concatcp!(ENV_PREFIX, "NODE")),
        hide_env=true,
    )]
    node: String,

    /// How long an unconfirmed ExternalIP is left alone before it is removed,
    /// or "never" to leave it there
    #[allow(clippy::doc_markdown, reason = "this doc comment is CLI help text")]
    #[arg(
        long,
        value_name("DURATION"),
        help_heading = "Node",
        value_parser = Self::parse_flag_removal_grace,
        env(concatcp!(ENV_PREFIX, "REMOVAL_GRACE")),
        hide_env=true,
    )]
    removal_grace: Option<Removal>,

    /// Perform dry run (real node addresses will not be changed)
    #[arg(long, help_heading = "Node")]
    dry_run: bool,

    /// Deprecated, use `--removal-grace` instead
    #[arg(
        long,
        hide = true,
        default_value_t=false,
        default_missing_value="true",
        num_args=0..=1,
        value_name="BOOL",
        hide_default_value=true,
        hide_possible_values=true,
        env=concatcp!(ENV_PREFIX, "STRICT"),
        hide_env=true,
    )]
    strict: bool,

    #[command(flatten)]
    providers: args::OfProviders,

    /// Share of the enabled trust an address must gather: 2/3, 75% or 0.75
    #[arg(
        long,
        value_name("SHARE"),
        help_heading = "Consensus",
        env(concatcp!(ENV_PREFIX, "TRUST_SHARE")),
        hide_env=true,
    )]
    trust_share: Option<TrustShare>,

    /// Deprecated, use `--trust-share` instead
    #[arg(
        short,
        long,
        hide = true,
        value_name("NUMBER"),
        alias("confirm"),
        alias("confirmation"),
        env(concatcp!(ENV_PREFIX, "CONFIRMATIONS")),
        hide_env=true,
    )]
    confirmations: Option<usize>,

    /// How often the checks must happen (must be 30s or more)
    #[arg(
        short = 't',
        long,
        help_heading = "Scheduling",
        value_parser = Self::parse_flag_interval,
        default_value_t = DisplayedDuration::from(Self::DEF_INTERVAL),
        env(concatcp!(ENV_PREFIX, "INTERVAL")),
        hide_env=true,
    )]
    interval: DisplayedDuration,
}

impl Args {
    const DEF_INTERVAL: StdDuration = StdDuration::from_mins(1);
    const MIN_INTERVAL: StdDuration = StdDuration::from_secs(30);

    const DEF_REMOVAL_GRACE: Removal = Removal::After(StdDuration::from_mins(5));

    // While an address is waiting out its grace, the loop stops pacing itself
    // by the interval: a grace shorter than the interval would otherwise never
    // see the rounds it needs. The rate limiter still holds every provider to
    // the gap it asks for, so a shorter tick costs nobody anything.
    const PROBE_INTERVAL: StdDuration = StdDuration::from_secs(30);

    // Parser for "--removal-grace" flag. A grace of zero still waits for the
    // rounds that make absence evidence; only the deprecated "--strict" skips
    // them, which is the one thing it is kept around to keep doing.
    fn parse_flag_removal_grace(s: &str) -> Result<Removal> {
        if s.eq_ignore_ascii_case("never") {
            return Ok(Removal::Never);
        }

        Ok(Removal::After(parse_duration(s).map_err(Error::msg)?))
    }

    // Parser for "--interval" flag.
    fn parse_flag_interval(s: &str) -> Result<DisplayedDuration> {
        match parse_duration(s).map_err(Error::msg)? {
            v if v >= Self::MIN_INTERVAL => Ok(v.into()),
            v => {
                let want_at_least: DisplayedDuration = Self::MIN_INTERVAL.into();
                let have: DisplayedDuration = v.into();
                bail!("must be {want_at_least} or greater, get: {have}")
            }
        }
    }

    // One tick: ask the world where we are, then tell Kubernetes.
    // Its own root span: nothing upstream hands this loop a trace context.
    #[instrument(name = "fckloud.tick", skip_all)]
    async fn job(&self, node: &mut NodeManager, resolver: &Resolver) -> Result<()> {
        let report = resolver.run().await;
        let confirmed = report.confirmed.into_iter().collect();

        node.apply(&confirmed, report.well_answered)
            .await
            .context("cannot apply the patch")?
            .into_iter()
            .for_each(|(ip_addr, status)| match status {
                AddrStatus::New => info!(?ip_addr, "new ExternalIP has been added"),
                AddrStatus::Skipped => debug!(?ip_addr, "old ExternalIP is left intact"),
                AddrStatus::Removed => warn!(?ip_addr, "old ExternalIP has been removed"),
            });

        Ok(())
    }
}

impl Executable for Args {
    // The preparation for [run], that adjusts some parameters if they had to.
    fn setup(mut self) -> Result<Self> {
        self.providers.setup()?;

        assert!(*self.interval >= Self::MIN_INTERVAL);
        assert!(!self.node.is_empty());

        ensure!(
            self.confirmations.is_none() || self.trust_share.is_none(),
            "--confirmations cannot be combined with --trust-share, which it replaces"
        );

        ensure!(
            !self.strict || self.removal_grace.is_none(),
            "--strict cannot be combined with --removal-grace, which it replaces"
        );

        if self.strict {
            warn!(concat!(
                "--strict is deprecated, --removal-grace replaces it; ",
                "it still tears off an unconfirmed address the same round, ",
                "on the word of whichever providers happened to answer",
            ));
        }

        if let Some(confirmations) = self.confirmations {
            warn!(
                confirmations,
                concat!(
                    "--confirmations is deprecated, --trust-share replaces it; ",
                    "an absolute number keeps its value while the providers around it change, ",
                    "so it quietly stops meaning what it meant when it was chosen",
                ),
            );
        }

        Ok(self)
    }

    // The "main" function for the "run" command.
    // Prepares scheduler and starts the operator.
    async fn run(self) -> Result<()> {
        info!("welcome to fckloud");

        let mut tfa = self.providers.trust_authority();

        if let Some(share) = self.trust_share {
            tfa.set_trust_share(share);
        }

        let mut node = NodeManager::new(&self.node).await?;
        let mut resolver = Resolver::new(self.providers.enabled.clone(), tfa)?;
        resolver
            .set_rate_limits(self.providers.rate_limit.iter().copied())
            .set_ignore_rate_limits(self.providers.ignore_rate_limits);

        node.current_external_ips()
            .await
            .context("cannot query the current ExternalIP addresses")?
            .iter()
            .for_each(|ip| debug!(?ip, "this ExternalIP is currently attached"));

        let removal = if self.strict {
            Removal::AtOnce
        } else {
            self.removal_grace.unwrap_or(Self::DEF_REMOVAL_GRACE)
        };

        node.set_dry_run(self.dry_run).set_removal(removal);
        info!(?removal, "unconfirmed addresses are removed");

        if let Some(confirmations) = self.confirmations {
            resolver.set_confirmations(confirmations);
        }

        resolver.announce(self.providers.pins());

        loop {
            let now = Instant::now();
            debug!("the time has come, executing job...");

            // An operator that dies on a hiccup stops operating. The next tick
            // is a better answer to a flaky network than a container restart.
            let outcome = self.job(&mut node, &resolver).await;
            let elapsed = now.elapsed();

            // The semantic conventions' catch-all: which way a tick failed is
            // in the log and in the two histograms below it, and duplicating
            // that taxonomy here would only give it a second place to drift.
            let failed = [KeyValue::new("error.type", "_OTHER")];
            let attributes: &[KeyValue] = if outcome.is_ok() { &[] } else { &failed };
            TICK_DURATION.record(elapsed.as_secs_f64() * 1000.0, attributes);

            if let Err(err) = outcome {
                error!(err = format!("{err:#}"), "the job execution is failed");
            }

            let until_next = if node.has_pending() {
                (*self.interval).min(Self::PROBE_INTERVAL)
            } else {
                *self.interval
            };

            let sleep_for = until_next.saturating_sub(elapsed);

            debug!(
                elapsed = DisplayedDuration::from(elapsed).to_string(),
                sleep_for = DisplayedDuration::from(sleep_for).to_string(),
                "the job has been completed",
            );

            sleep(sleep_for).await;
        }
    }
}
