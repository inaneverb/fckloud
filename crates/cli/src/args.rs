use {
    anyhow::Result, clap::Args as ClapArgs, ndhcp::HttpProvider, std::str::FromStr,
    strum::VariantArray,
};

// The global application options.
#[derive(ClapArgs)]
pub struct Global {
    /// Enable verbose output (up to 3 levels)
    #[arg(global=true, short, long, action=clap::ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Clone, ClapArgs)]
pub struct OfProviders {
    /// The list of providers that should be disabled (assuming enabled all)
    #[arg(
        long,
        value_name("PROVIDER"),
        value_parser = Self::parse_flag_disable,
    )]
    pub disable: Vec<HttpProvider>,

    /// The list of enabled providers.
    /// Computed lately based on all providers and given `disable`.
    #[arg(skip)]
    pub enable: Vec<HttpProvider>,
}

impl OfProviders {
    pub fn setup(&mut self) {
        self.enable = HttpProvider::VARIANTS.to_vec();
        self.enable.retain(|e| !self.disable.contains(e));
    }

    // Parser for "--disable" flag.
    fn parse_flag_disable(s: &str) -> Result<HttpProvider> {
        Ok(HttpProvider::from_str(s)?)
    }
}
