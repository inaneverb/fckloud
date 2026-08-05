use {
    crate::{Executable, args, pubip::Resolver},
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
        let pinned = self.providers.pins();
        let tfa = self.providers.trust_authority();

        let mut resolver = Resolver::new(self.providers.enabled.clone(), tfa)?;
        resolver
            .set_rate_limits(self.providers.rate_limit.iter().copied())
            .set_ignore_rate_limits(self.providers.ignore_rate_limits);

        resolver.announce(pinned);

        resolver
            .run()
            .await
            .confirmed
            .iter()
            .for_each(|ip_addr| info!(?ip_addr, "address has been confirmed"));

        Ok(())
    }
}
