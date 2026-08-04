use {
    crate::{
        Executable, args,
        pubip::{Resolver, TrustFactorAuthority},
    },
    anyhow::Result,
    clap::Args as ClapArgs,
    tracing::info,
};

/// The list of options for the "test" command.
#[derive(ClapArgs)]
pub struct Args {
    #[command(flatten)]
    providers: args::OfProviders,
}

impl Executable for Args {
    // The preparation for [test], that adjusts some parameters if they had to.
    fn setup(mut self) -> Result<Self> {
        self.providers.setup()?;
        Ok(self)
    }

    // The "main" function for the "test" command.
    // Perpares the Tokio runtime, executes HTTP requests to IP resolvers.
    async fn run(self) -> Result<()> {
        Resolver::new(self.providers.enable, TrustFactorAuthority::default())
            .run()
            .await
            .confirmed
            .iter()
            .for_each(|ip_addr| info!(?ip_addr, "address has been confirmed"));

        Ok(())
    }
}
