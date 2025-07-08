use clap::Args as ClapArgs;
use anyhow::Result;

use crate::app::{self, Executable};

/// The list of options for the "test" command.
#[derive(ClapArgs)]
pub struct Args {
    
}

impl Executable for Args {
    fn setup(self) -> Self {
        self
    }
    
    // The "main" function for the "test" command.
    // Perpares the Tokio runtime, executes HTTP requests to IP resolvers.
    #[tokio::main]
    async fn run(self, global: app::Args) -> Result<()> {
        Ok(())
    }
}