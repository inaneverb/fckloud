use {
    crate::{cmd_run, cmd_test},
    anyhow::{Result, Context},
    clap::{Args as ClapArgs, Parser as ClapParser, Subcommand as ClapSubcommand},
    std::process::exit,
    tracing::Event,
    tracing::error,
    tracing_forest::{ForestLayer, PrettyPrinter, Tag},
    tracing_subscriber::{Registry, layer::SubscriberExt, util::SubscriberInitExt},
};

// The application itself.
#[derive(ClapParser)]
#[command(version, about, long_about = None)]
struct App {
    #[command(subcommand)]
    command: Command,

    #[command(flatten)]
    args: Args,
}

// The global application options.
#[derive(ClapArgs)]
pub struct Args {
    /// Enable verbose output (up to 3 levels)
    #[arg(global=true, short, long, action=clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(ClapSubcommand)]
pub enum Command {
    /// Starts the operator
    Run(cmd_run::Args),
    /// Test what IP would be assigned to the machine (node)
    Test(cmd_test::Args),
}

// The interface must been implemented for the type to be treated as CLI command.
//
// For now it implements by the specific CLI command's options type,
// thus representing the CLI command abstraction themself.
pub trait Executable {
    fn setup(self) -> Self;
    fn run(self, global: Args) -> Result<()>;
}

// ========================================================================== //

impl App {
    fn setup(mut self) -> Self {
        self.setup_logging();
        self
    }

    fn setup_logging(&mut self) {
        Registry::default();

        const LTF_KITCHEN: &'static str =
            "[hour padding:none repr:12]:[minute padding:zero] [period case:upper]";

        let parsed_time_format = time::format_description::parse(LTF_KITCHEN)
            .with_context(|| "BUG: Cannot parse static time format").unwrap();

        let traces_timer = tracing_subscriber::fmt::time::OffsetTime::new(
            time::UtcOffset::current_local_offset()
                .with_context(|| "BUG: Cannot obtain current UTC offset").unwrap(),
            parsed_time_format,
        );

        let max_log_level = match self.args.verbose {
            0 => tracing::Level::INFO,
            1 => tracing::Level::DEBUG,
            _ => tracing::Level::TRACE,
        };

        tracing_subscriber::fmt()
            .compact()
            .with_timer(traces_timer)
            .with_ansi(true)
            .with_max_level(max_log_level)
            .init();
    }

    // The finalizer of [App] initializer. Explodes the current object
    // and returns the global arguments along with the CLI command to execute.
}

// pub fn setup() {
//     let proc = PrettyPrinter::new();
//     let layer = ForestLayer::new(proc, extract_tag);

// }

// Tag extractor. Extracts the [Tag] from the given [Event].
// https://docs.rs/tracing-forest/0.1.6/tracing_forest/tag/index.html
fn extract_tag(ev: &Event) -> Option<Tag> {
    None
}

// ========================================================================== //

// The "setup" function that initializes logging, metrics, whatever else,
// parses CLI, ENV parameters and returns the CLI command that must be executed
// along with the global app parameters.

pub fn exec() {
    let app = App::parse().setup();

    if let Err(err) = match app.command {
        Command::Run(cmd_args) => cmd_args.setup().run(app.args),
        Command::Test(cmd_args) => cmd_args.setup().run(app.args),
    } {
        let err = format!("{}, because {}", err.to_string(), err.root_cause());
        error!(err = err, "critical error");
        exit(1);
    }
}
