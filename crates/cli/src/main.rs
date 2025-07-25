use {
    anyhow::Result,
    clap::{Parser as ClapParser, Subcommand as ClapSubcommand},
    ekacore::traits::Discard,
    std::{future::Future, process::exit},
    tokio::{select, signal, spawn, sync::mpsc},
    tracing::error,
    tracing_subscriber::{filter::EnvFilter, fmt::time::ChronoLocal as ChronoLocalTimeFormatter},
};

mod args;
mod cmd_run;
mod cmd_test;

// The application itself.
#[derive(ClapParser)]
#[command(version, about, long_about = None, disable_help_subcommand = true)]
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
}

// The interface must be implemented for a type to act as a CLI command.
pub trait Executable {
    fn setup(self) -> Self;
    fn run(self, global: args::Global) -> impl Future<Output = Result<()>> + Send;
}

impl App {
    fn setup(mut self) -> Self {
        self.setup_logging();
        self
    }

    fn setup_logging(&mut self) {
        // const CONSOLE_TIME_FORMAT: &'static str = "%R";
        const CONSOLE_TIME_FORMAT: &'static str = "%l:%M %p";

        // https://docs.rs/chrono/latest/chrono/format/strftime/index.html
        let traces_timer = ChronoLocalTimeFormatter::new(CONSOLE_TIME_FORMAT.into());

        // https://docs.rs/tracing-subscriber/latest/tracing_subscriber/?search=EnvFilter
        let env_filter = EnvFilter::new(match self.args.verbose {
            1 => "info,fckloud=debug",
            2 => "debug",
            3 => "trace",
            _ => "info",
        });
        
        tracing_subscriber::fmt()
            .compact()
            .with_timer(traces_timer)
            .with_target(false)
            .with_ansi(true)
            .with_env_filter(env_filter)
            .init();
    }
}

// The main function inside the Tokio runtime, returning an OS exit code.
// Executes the command handler and listens for SIGINT or similar signals.
#[tokio::main]
#[inline(never)]
async fn main_runtime(app: App) -> i32 {
    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel();

    spawn(async move {
        match app.command {
            Command::Run(run_args) => run_args.setup().run(app.args).await,
            Command::Test(test_args) => test_args.setup().run(app.args).await,
        }
        .unwrap_or_else(|err| shutdown_tx.send(err).discard())
    });

    // Any occurred error is to send to the `shutdown_tx`,
    // thus interrupting the workflow and the whole application itself.

    let mut err = None;
    select! {
        _ = signal::ctrl_c() => (),
        err_recv = shutdown_rx.recv() => err = err_recv,
    }
    if let Some(ref err) = err {
        error!(err = format!("{:#}", err), "critical error");
    }
    err.and_then(|_| Some(1)).unwrap_or(0)
}
// Executes the Tokio runtime main only if the application is provided
// with valid arguments thus parsing it at first.
fn main() {
    exit(main_runtime(App::parse().setup()));
}