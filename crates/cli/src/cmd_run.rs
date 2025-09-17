use {
    crate::{Executable, args, build_info::ENV_PREFIX},
    anyhow::{Context, Error, Result, bail},
    clap::Args as ClapArgs,
    const_format::concatcp,
    humantime::{Duration as DisplayedDuration, parse_duration},
    kubem::{AddrStatus, Manager as KubeManager},
    ndhcp::Manager as AddrManager,
    std::time::Duration as StdDuration,
    strum::EnumCount,
    tokio::time::{Instant, sleep},
    tracing::{debug, info, warn},
};

/// The list of options for the "run" command.
#[derive(ClapArgs)]
pub struct Args {
    /// The current node name the operator is running on
    #[arg(
        short,
        long,
        value_name("NAME"),
        env(concatcp!(ENV_PREFIX, "NODE")),
        hide_env=true,
    )]
    node: String,

    /// The number of providers required for IP address to consider it public
    #[arg(
        short,
        long,
        value_name("NUMBER"),
        default_value_t = 1,
        alias("confirm"),
        alias("confirmation"),
        env(concatcp!(ENV_PREFIX, "CONFIRMATIONS")),
        hide_env=true,
    )]
    confirmations: i32,

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
    const DEF_INTERVAL: StdDuration = StdDuration::from_secs(60);
    const MIN_INTERVAL: StdDuration = StdDuration::from_secs(30);

    const MIN_CONFIRMATIONS: i32 = 1;
    const MAX_CONFIRMATIONS: i32 = ndhcp::HttpProvider::COUNT as i32;

    // Parser for "--interval" flag.
    fn parse_flag_interval(s: &str) -> Result<DisplayedDuration> {
        match parse_duration(s).map_err(Error::msg)? {
            v if v >= Self::MIN_INTERVAL => Ok(v.into()),
            v => {
                let want_at_least: DisplayedDuration = Self::MIN_INTERVAL.into();
                let have: DisplayedDuration = v.into();
                bail!("must be {} or greater, get: {}", want_at_least, have)
            }
        }
    }

    // Entry point of operator's each cronjob iteration.
    //
    // Creates manager, connects to the Kubernetes, scans for IP addresses,
    // applies them to the current node and goes to sleep till the next iteration.
    async fn job(
        &self,
        _: &args::Global,
        kube_manager: &mut KubeManager,
        addr_manager: &AddrManager,
    ) -> Result<()> {
        addr_manager
            .run()
            .await
            .confirmed
            .iter()
            .for_each(|ip_addr| {
                kube_manager.stage_address(ip_addr);
            });

        kube_manager
            .apply()
            .await
            .with_context(|| format!("cannot apply the patch"))?
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

        self.confirmations = self
            .confirmations
            .clamp(Self::MIN_CONFIRMATIONS, Self::MAX_CONFIRMATIONS);

        assert!(*self.interval >= Self::MIN_INTERVAL);
        assert!(!self.node.is_empty());
        assert!(self.confirmations >= Self::MIN_CONFIRMATIONS);
        assert!(self.confirmations <= Self::MAX_CONFIRMATIONS);

        if *self.interval < Self::DEF_INTERVAL {
            warn!(
                given_interval = self.interval.to_string(),
                safe_min_interval = DisplayedDuration::from(Self::DEF_INTERVAL).to_string(),
                concat!(
                    "specified interval could be too short, ",
                    "many providers discourage you from using <= 1m one per IP per machine",
                ),
            )
        }

        Ok(self)
    }

    // The "main" function for the "run" command.
    // Prepares scheduler and starts the operator.
    async fn run(self, global: args::Global) -> Result<()> {
        info!("welcome to fckloud");

        let mut kube_manager = kubem::Manager::new(&self.node).await?;
        let addr_manager = ndhcp::Manager::new(self.providers.enable.clone());

        kube_manager
            .query_current_addresses()
            .await
            .with_context(|| format!("cannot query the current ExternalIP addresses"))?
            .for_each(|ip| debug!(?ip, "this ExternalIP is currently attached"));

        kube_manager
            .set_dry_run(self.dry_run)
            .set_remove_unstaged(self.strict);

        loop {
            let now = Instant::now();
            debug!("the time has come, executing job...");

            self.job(&global, &mut kube_manager, &addr_manager)
                .await
                .with_context(|| format!("the job execution is failed"))?;

            let elapsed = now.elapsed();
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
