use {
    crate::{args, Executable},
    anyhow::Result,
    clap::Args as ClapArgs
};

/// The list of options for the "test" command.
#[derive(ClapArgs)]
pub struct Args {
    
}

impl Args {
    
}

impl Executable for Args {
    fn setup(self) -> Self {
        self
    }
    
    // The "main" function for the "test" command.
    // Perpares the Tokio runtime, executes HTTP requests to IP resolvers.
    async fn run(self, global: args::Global) -> Result<()> {
        Ok(())
    }
}