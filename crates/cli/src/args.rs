use {
    crate::build_info::ENV_PREFIX,
    anyhow::{Result, ensure},
    clap::{
        Args as ClapArgs,
        builder::{PossibleValuesParser, TypedValueParser},
    },
    const_format::concatcp,
    ndhcp::{HttpProvider, HttpProviders},
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
    #[arg(
        global=true,
        short,
        long,
        action=clap::ArgAction::Count,
        env(concatcp!(ENV_PREFIX, "VERBOSE")),
        hide_env=true,
    )]
    pub verbose: u8,

    /// Write logs in JSON instead of human-readable format
    #[arg(
        global = true,
        short,
        long,
        default_value_t=false,
        default_missing_value="true",
        num_args=0..=1,
        value_name="BOOL",
        hide_default_value=true,
        hide_possible_values=true,
        env(concatcp!(ENV_PREFIX, "JSON")),
        hide_env=true,
    )]
    pub json: bool,
}

#[derive(Clone, ClapArgs)]
pub struct OfProviders {
    /// The list of providers that should be disabled (assuming enabled all)
    #[arg(
        long,
        value_name("PROVIDER"),
        value_parser = clap_enum_variants!(HttpProvider),
        env(concatcp!(ENV_PREFIX, "DISABLE")),
        hide_env=true,
    )]
    pub disable: Vec<HttpProvider>,

    /// The list of enabled providers.
    /// Computed lately based on all providers and given `disable`.
    #[arg(skip)]
    pub enable: HttpProviders,
}

impl OfProviders {
    pub fn setup(&mut self) -> Result<()> {
        self.enable = <HttpProvider as VariantArray>::VARIANTS
            .iter()
            .filter(|provider| !self.disable.contains(*provider))
            .cloned()
            .collect();

        ensure!(
            !self.enable.is_empty(),
            "at least one provider must be enabled"
        );

        Ok(())
    }
}
