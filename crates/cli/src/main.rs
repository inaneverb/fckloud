use clap::{Args, Parser, Subcommand};
use local_ip_address;

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
struct DryArgs {}

impl Default for DryArgs {
    fn default() -> Self {
        Self {}
    }
}

trait Action {
    fn run(self, global: ArgsGlobal);
}

impl Action for DryArgs {
    fn run(self, global: ArgsGlobal) {
        println!(
            "Hello from `dry` command; global={:?}, cmd={:?}",
            global, &self
        );
        let ifaces = pnet::datalink::interfaces();
        ndhcp::iface_report(&ifaces)
            .iter()
            .for_each(|item| println!("{item}"));
        
        println!("From crate local_ip_addresses");
        
        let result = local_ip_address::list_afinet_netifas().unwrap();
        for (iface_name, addr) in result {
            println!("{}/{}", iface_name, addr)
        }
    }
}

impl Action for RunArgs {
    fn run(self, global: ArgsGlobal) {
        println!(
            "Hello from `run` command; global={:?}, cmd={:?}",
            global, &self
        )
    }
}

fn main() {
    let app = App::parse();
    match app.command {
        Some(Commands::Dry(args)) => args.run(app.args),
        Some(Commands::Run(args)) => args.run(app.args),
        None => RunArgs::default().run(app.args),
    }
}
