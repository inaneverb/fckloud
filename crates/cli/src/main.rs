use anyhow::{Context, Result};
use clap::{builder::Str, Args, Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct App {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    args: ArgsGlobal,
}

#[derive(Args, Debug)]
struct ArgsGlobal {
    /// Enable verbose output (up to 3 levels)
    #[arg(global=true, short, long, action=clap::ArgAction::Count)]
    verbose: u8,
}

impl Default for ArgsGlobal {
    fn default() -> Self {
        Self { verbose: 0 }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Starts the operator
    Run(RunArgs),
    /// Test what IP would be assigned to machine
    Dry(DryArgs),
}

#[derive(Args, Debug)]
struct RunArgs {}

impl Default for RunArgs {
    fn default() -> Self {
        Self {}
    }
}

#[derive(Args, Debug)]
struct DryArgs {
    /// The current node name the controller is running on
    #[arg(short='n', long="node")]
    node: String
}

// impl Default for DryArgs {
//     fn default() -> Self {
//         Self {}
//     }
// }

impl DryArgs {
    #[tokio::main]
    async fn run(self, global: ArgsGlobal) -> Result<()> {
        println!(
            "Hello from `dry` command; global={:?}, cmd={:?}",
            global, &self
        );
        // let ifaces = pnet::datalink::interfaces();
        ndhcp::resolve(-1)
            .await?
            .into_iter()
            .for_each(|(item, status)| println!("{}: {}", item, status));

        // let result = local_ip_address::list_afinet_netifas().unwrap();
        // for (iface_name, addr) in result {
        //     println!("{}/{}", iface_name, addr)
        // }

        let kube_manager = kubem::Manager::new(self.node).await?;
        _ = kube_manager;
        // println!("{}", kube_manager.version().await?);

        Ok(())
    }
}

impl RunArgs {
    fn run(self, global: ArgsGlobal) -> Result<()> {
        println!(
            "Hello from `run` command; global={:?}, cmd={:?}",
            global, &self
        );

        Ok(())
    }
}

fn main_wrapped() -> Result<()> {
    const LTF_KITCHEN: &'static str =
        "[hour padding:none repr:12]:[minute padding:zero] [period case:upper]";

    let parsed_time_format = time::format_description::parse(LTF_KITCHEN)
        .with_context(|| "BUG: Cannot parse static time format")?;

    let traces_timer = tracing_subscriber::fmt::time::OffsetTime::new(
        time::UtcOffset::current_local_offset()
            .with_context(|| "BUG: Cannot obtain current UTC offset")?,
        parsed_time_format,
    );

    tracing_subscriber::fmt()
        .compact()
        .with_timer(traces_timer)
        .with_ansi(true)
        .init();

    let app = App::parse();

    match app.command {
        Some(Commands::Dry(args)) => args.run(app.args),
        Some(Commands::Run(args)) => args.run(app.args),
        None => RunArgs::default().run(app.args),
    }
}

fn main() {
    if let Err(err) = main_wrapped() {
        let err = format!("{}, because {}", err.to_string(), err.root_cause());
        tracing::error!(err = err, "critical error");
    }
}
