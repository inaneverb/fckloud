use anyhow::{Context, Error, Result, anyhow, bail};
use clap::{Args as ClapArgs, builder::PossibleValuesParser as ClapPossibleValues};
use duration_str::parse_chrono;
use ndhcp::HttpProvider;
use std::{collections::BTreeSet, str::FromStr};
use strum::VariantArray;
// use time::Duration;
use chrono::Duration;
use clokwerk::{AsyncScheduler, Interval};
use ekacore::traits::NotOk;
use std::{sync::Arc, time::Duration as StdDuration};
use tokio::{select, signal, spawn, sync::mpsc, time::sleep};
use tracing::{error, info, instrument, warn};

use crate::app::{self, Executable};

/// The list of options for the "run" command.
#[derive(ClapArgs)]
pub struct Args {
    /// The current node name the operator is running on
    #[arg(short, long, value_name("NAME"), env("FKCLOUD_NODE"))]
    node: String,

    /// The number of providers required for IP address to consider it public
    #[arg(
        short,
        long,
        value_name("NUMBER"),
        default_value_t = 1,
        alias("confirm"),
        alias("confirmation"),
        env("FKCLOUD_CONFIRM"),
        env("FKCLOUD_CONFIRMATION"),
        env("FKCLOUD_CONFIRMATIONS")
    )]
    confirmations: i32,

    /// Perform dry run (real node addresses will not be changed)
    #[arg(long)]
    dry_run: bool,

    /// How often the checks must happen; must be 30s or more
    #[arg(
        long,
        value_parser = Self::parse_flag_interval,
        default_value_t = Self::MIN_INTERVAL,
        env("FKCLOUD_INTERVAL"),
    )]
    interval: Duration,

    /// The list of providers that should be disabled (assuming enabled all)
    #[arg(
        long,
        value_name("PROVIDER"),
        value_parser = Self::parse_flag_disable,
        env("FKCLOUD_DISABLE")
    )]
    disable: Vec<HttpProvider>,

    /// The list of enabled providers.
    /// Computed lately based on all providers and given `disable`.
    #[arg(skip)]
    enable: Vec<HttpProvider>,

    /// Remove unmatched ExternalIP addresses from the node
    #[arg(long, default_value_t = false, env("FKCLOUD_STRICT"))]
    strict: bool,
}

impl Args {
    const MIN_INTERVAL: Duration = Duration::seconds(30);

    // Parser for "--interval" flag.
    fn parse_flag_interval(s: &str) -> Result<Duration> {
        match parse_chrono(s).map_err(Error::msg)? {
            v if v >= Self::MIN_INTERVAL => Ok(v),
            v => bail!("must be {} or greater, get: {}", Self::MIN_INTERVAL, v),
        }
    }

    // Parser for "--disable" flag.
    fn parse_flag_disable(s: &str) -> Result<HttpProvider> {
        Ok(HttpProvider::from_str(s)?)
    }

    // Entry point of operator's each cronjob iteration.
    //
    // Creates manager, connects to the Kubernetes, scans for IP addresses,
    // applies them to the current node and goes to sleep till the next iteration.
    #[instrument(skip_all)]
    async fn main(&self, global: &app::Args) -> Result<()> {
        assert!(self.interval >= Self::MIN_INTERVAL);
        assert!(!self.node.is_empty());

        let kube_manager = kubem::Manager::new(&self.node).await?;
        let addr_manager = ndhcp::Manager::new(&self.enable)?.run().await;

        for ip_addr in addr_manager.iter_errored() {
            // TODO: Add log
        }

        let mut node_handle = kube_manager.get_handle(self.dry_run).await?;
        for (ip_addr, providers) in addr_manager.iter_succeeded_threshold(self.confirmations) {
            node_handle.apply_addrs(ip_addr).await?;
            // TODO: Add log
        }

        if self.strict {
            for ip_addr in node_handle.remove_unapplied().await? {
                // TODO: Add log
            }
        }

        Ok(())
    }

    // Wrapper for [main] that calls it and sends an error to the `shutdown_tx`
    // if there is any.
    // Thus, an error happened during any operator's cronjob iteration
    // will lead to the stop of whole application.
    #[instrument(skip_all)]
    async fn main_wrapped(
        args: Arc<Self>,
        global: Arc<app::Args>,
        shutdown_tx: mpsc::UnboundedSender<Error>,
    ) {
        if let Err(err) = args.main(&global).await {
            shutdown_tx.send(err);
        }
    }
}

// ========================================================================== //

impl Executable for Args {
    // The preparation for [run], that adjusts some parameters if they had to.
    fn setup(mut self) -> Self {
        self.enable = HttpProvider::VARIANTS.to_vec();
        self.enable.retain(|e| !self.disable.contains(e));
        self
    }

    // The "main" function for the "run" command.
    // Prepares the Tokio runtime, scheduler and starts the operator.
    #[tokio::main]
    async fn run(self, global: app::Args) -> Result<()> {
        const DELAY_EXECUTION: Duration = Duration::seconds(1);

        let args = Arc::new(self);
        let global = Arc::new(global);

        let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel();
        let mut scheduler = AsyncScheduler::new();

        scheduler
            .every(Interval::Seconds(args.interval.num_seconds() as u32))
            .run(move || Self::main_wrapped(args.clone(), global.clone(), shutdown_tx.clone()));

        // The scheduler is going to execute each new queued cron job
        // in a separate coroutine endlessly.
        // TODO: Do we need a sleep here? What's the "poll" for scheduler?

        spawn(async move {
            loop {
                scheduler.run_pending().await;
                sleep(DELAY_EXECUTION.to_std().unwrap()).await;
            }
        });

        // The only way to shutdown is to stop the operator explicitly.
        // ctrl-c means stop is requested, so it's ().
        // Shutdown channel might contain only error, so treat it like this.

        select! {
            _ = signal::ctrl_c() => Ok(()),
            res = shutdown_rx.recv() => res.not_ok(),
        }
    }
}
