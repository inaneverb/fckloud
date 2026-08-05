use {
    anyhow::Result,
    clap::{Parser as ClapParser, Subcommand as ClapSubcommand},
    std::{future::Future, process::exit},
    tokio::{select, signal, spawn, sync::mpsc},
    tracing::error,
    tracing_subscriber::{
        Layer, filter::EnvFilter, fmt::time::ChronoLocal as ChronoLocalTimeFormatter,
        layer::SubscriberExt, util::SubscriberInitExt,
    },
};

#[cfg(unix)]
use tokio::signal::unix::SignalKind;

mod args;
mod build_info;
mod cmd_providers;
mod cmd_run;
mod cmd_test;
mod node;
mod pubip;
mod telemetry;

// The application itself.
#[derive(ClapParser)]
#[command(version = build_info::version())]
#[command(author = build_info::authors(), about, long_about = None)]
#[command(disable_help_subcommand = true)]
// clap 4 dropped the author from its default template, so the template says
// where it goes. Only the root command carries it; a subcommand repeating the
// authors on every `--help` would be noise.
#[command(help_template = "\
{before-help}{about-with-newline}
Authors:
{author}

{usage-heading} {usage}

{all-args}{after-help}
")]
struct App {
    #[command(subcommand)]
    command: Command,

    #[command(flatten)]
    args: args::Global,
}

// CLI commands application does support.
#[derive(ClapSubcommand)]
pub enum Command {
    /// Starts the operator
    Run(cmd_run::Args),
    /// Test what IP would be assigned to the machine (node)
    Test(cmd_test::Args),
    /// List the known providers and what is known about them
    Providers(cmd_providers::Args),
}

// The interface must be implemented for a type to act as a CLI command.
pub trait Executable
where
    Self: Sized,
{
    fn setup(self) -> Result<Self>;
    fn run(self) -> impl Future<Output = Result<()>> + Send;
}

impl App {
    fn setup(mut self) -> (Self, telemetry::Guard) {
        let telemetry = self.setup_logging();
        (self, telemetry)
    }

    fn setup_logging(&mut self) -> telemetry::Guard {
        // https://docs.rs/chrono/latest/chrono/format/strftime/index.html
        const CONSOLE_TIME_FORMAT: &str = "%l:%M %p";

        enum LogKind {
            HumanReadable,
            Json,
        }

        let log_kind = if self.args.json {
            LogKind::Json
        } else {
            LogKind::HumanReadable
        };

        let log_layer = match log_kind {
            LogKind::Json => tracing_subscriber::fmt::layer()
                .json()
                .with_timer(ChronoLocalTimeFormatter::rfc_3339())
                .boxed(),

            LogKind::HumanReadable => tracing_subscriber::fmt::layer()
                .with_timer(ChronoLocalTimeFormatter::new(CONSOLE_TIME_FORMAT.into()))
                .compact()
                .with_target(false)
                .with_ansi(true)
                .boxed(),
        };

        // https://docs.rs/tracing-subscriber/latest/tracing_subscriber/?search=EnvFilter
        let env_filter_layer = EnvFilter::new(match self.args.verbose {
            1 => "info,fckloud=debug",
            2 => "debug",
            3 => "trace",
            _ => "info",
        });

        let (otel_layer, telemetry) = telemetry::install();

        tracing_subscriber::registry()
            .with(log_layer)
            .with(otel_layer)
            .with(env_filter_layer)
            .init();

        telemetry.announce();
        telemetry
    }

    async fn run(self) -> Result<()> {
        match self.command {
            Command::Run(run_args) => run_args.setup()?.run().await,
            Command::Test(test_args) => test_args.setup()?.run().await,
            Command::Providers(list_args) => list_args.setup()?.run().await,
        }
    }
}

// Resolves once the orchestrator asks us to leave. Kubernetes says SIGTERM
// first and SIGKILL later; ignoring the polite one only wastes the grace period.
#[cfg(unix)]
async fn shutdown_requested() {
    let mut terminate =
        signal::unix::signal(SignalKind::terminate()).expect("cannot subscribe to SIGTERM");

    select! {
        _ = signal::ctrl_c() => (),
        _ = terminate.recv() => (),
    }
}

#[cfg(not(unix))]
async fn shutdown_requested() {
    let _ = signal::ctrl_c().await;
}

// The main function inside the Tokio runtime, returning an OS exit code.
// Executes the command handler and listens for the termination signals.
#[tokio::main]
#[inline(never)]
async fn main_runtime(app: App) -> i32 {
    // Any occurred error is to send to the `shutdown_tx`,
    // thus interrupting the workflow and the whole application itself.

    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel();

    spawn(async move {
        if let Err(err) = app.run().await {
            let _ = shutdown_tx.send(err);
        }
    });

    let mut err = None;
    select! {
        () = shutdown_requested() => (),
        err_recv = shutdown_rx.recv() => err = err_recv,
    }

    match err {
        Some(err) => {
            error!(err = format!("{err:#}"), "critical error");
            1
        }
        None => 0,
    }
}

// Executes the Tokio runtime main only if the application is provided
// with valid arguments thus parsing it at first.
fn main() {
    let (app, telemetry) = App::parse().setup();
    let exit_code = main_runtime(app);

    // The runtime is gone by the time we get here, which is where the last
    // flush belongs: it blocks, and `exit` below runs no destructors.
    telemetry.shutdown();

    exit(exit_code);
}
