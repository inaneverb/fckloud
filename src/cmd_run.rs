use {
    crate::{
        Executable, args,
        build_info::ENV_PREFIX,
        node::{AddrStatus, Manager as NodeManager},
        pubip::{Resolver, TrustFactorAuthority},
        telemetry::meter,
    },
    anyhow::{Context as _, Error, Result, bail},
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
        env(concatcp!(ENV_PREFIX, "NODE")),
        hide_env=true,
    )]
    node: String,

    /// Custom confirmation number each IP must reach to consider it confirmed
    #[arg(
        short,
        long,
        value_name("NUMBER"),
        alias("confirm"),
        alias("confirmation"),
        env(concatcp!(ENV_PREFIX, "CONFIRMATIONS")),
        hide_env=true,
    )]
    confirmations: Option<usize>,

    /// Perform dry run (real node addresses will not be changed)
    #[arg(long)]
    dry_run: bool,

    /// How often the checks must happen (must be 30s or more)
    #[arg(
        short = 't',
        long,
        value_parser = Self::parse_flag_interval,
        default_value_t = DisplayedDuration::from(Self::DEF_INTERVAL),
        env(concatcp!(ENV_PREFIX, "INTERVAL")),
        hide_env=true,
    )]
    interval: DisplayedDuration,

    #[command(flatten)]
    providers: args::OfProviders,

    /// Remove unmatched ExternalIP addresses from the node
    #[allow(clippy::doc_markdown, reason = "this doc comment is CLI help text")]
    #[arg(
        long,
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
}

impl Args {
    const DEF_INTERVAL: StdDuration = StdDuration::from_mins(1);
    const MIN_INTERVAL: StdDuration = StdDuration::from_secs(30);

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
        let confirmed = resolver.run().await.confirmed.into_iter().collect();

        node.apply(&confirmed)
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

        if let Some(confirmations) = self.confirmations {
            warn!(
                confirmations,
                concat!(
                    "custom confirmation number detected; ",
                    "unwise picked such a number may lead to either ",
                    "an inability to reach consensus for a single IP (if the threshold is too high) ",
                    "or result in falsely reported IPs being assigned to the node (if the threshold is too low)",
                ),
            );
        }

        Ok(self)
    }

    // The "main" function for the "run" command.
    // Prepares scheduler and starts the operator.
    async fn run(self) -> Result<()> {
        info!("welcome to fckloud");

        let mut tfa = TrustFactorAuthority::default();
        for (provider, trust_factor) in &self.providers.trust_factor {
            tfa.set_trust_factor(*provider, *trust_factor);
        }

        let mut node = NodeManager::new(&self.node).await?;
        let mut resolver = Resolver::new(self.providers.enabled.clone(), tfa);
        resolver
            .set_rate_limits(self.providers.rate_limit.iter().copied())
            .set_ignore_rate_limits(self.providers.ignore_rate_limits);

        node.current_external_ips()
            .await
            .context("cannot query the current ExternalIP addresses")?
            .iter()
            .for_each(|ip| debug!(?ip, "this ExternalIP is currently attached"));

        node.set_dry_run(self.dry_run)
            .set_remove_unstaged(self.strict);

        if let Some(confirmations) = self.confirmations {
            resolver.set_confirmations(confirmations);
        }

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

            let sleep_for = self.interval.saturating_sub(elapsed);

            debug!(
                elapsed = DisplayedDuration::from(elapsed).to_string(),
                sleep_for = DisplayedDuration::from(sleep_for).to_string(),
                "the job has been completed",
            );

            sleep(sleep_for).await;
        }
    }
}
