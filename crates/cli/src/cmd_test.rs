use {
    crate::{Executable, args},
    anyhow::Result,
    clap::Args as ClapArgs,
    ndhcp,
    tracing::info,
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
        ndhcp::resolve_by(&self.providers.enable)
            .await
            .iter()
            .for_each(|ip_addr| info!(?ip_addr, "address has been confirmed"));

        Ok(())
    }
}
