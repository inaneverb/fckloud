use {
    crate::{Executable, args},
    anyhow::Result,
    clap::Args as ClapArgs,
    ndhcp,
    tracing::{error, info},
};

/// The list of options for the "test" command.
#[derive(ClapArgs)]
pub struct Args {
    #[command(flatten)]
    providers: args::OfProviders,
}

impl Args {}

impl Executable for Args {
    // The preparation for [test], that adjusts some parameters if they had to.
    fn setup(mut self) -> Self {
        self.providers.setup();
        self
    }

    // The "main" function for the "test" command.
    // Perpares the Tokio runtime, executes HTTP requests to IP resolvers.
    async fn run(self, _: args::Global) -> Result<()> {
        let addr_manager = ndhcp::Manager::new(&self.providers.enable)?.run().await;

        // Log these providers that were unable to confirm.
        addr_manager
            .iter_errored()
            .map(|(provider, err)| (provider, format!("{:#}", err)))
            .for_each(|(provider, err)| error!(?provider, err, "provider cannot be used"));

        // We got some IP addresses confirmed.
        addr_manager
            .iter_succeeded()
            .for_each(|(ip_addr, providers)| {
                info!(?ip_addr, ?providers, "address has been confirmed")
            });

        Ok(())
    }
}
