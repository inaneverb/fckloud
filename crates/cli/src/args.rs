use {
    clap::{
        Args as ClapArgs,
        builder::{PossibleValuesParser, TypedValueParser},
    },
    ndhcp::HttpProvider,
    strum::{VariantArray, VariantNames},
};

// Creds: https://github.com/clap-rs/clap/discussions/4264
macro_rules! clap_enum_variants {
    ($e: ty) => {{
        use TypedValueParser;
        use VariantNames;
        let parser = PossibleValuesParser::new(<$e as VariantNames>::VARIANTS);
        parser.map(|s| s.parse::<$e>().unwrap())
    }};
}

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
        value_parser = clap_enum_variants!(HttpProvider)
    )]
    pub disable: Vec<HttpProvider>,

    /// The list of enabled providers.
    /// Computed lately based on all providers and given `disable`.
    #[arg(skip)]
    pub enable: Vec<HttpProvider>,
}

impl OfProviders {
    pub fn setup(&mut self) {
        self.enable = <HttpProvider as VariantArray>::VARIANTS.to_vec();
        self.enable.retain(|e| !self.disable.contains(e));
    }
}
