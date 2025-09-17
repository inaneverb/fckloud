use {
    crate::build_info::ENV_PREFIX,
    anyhow::{Result, anyhow, bail, ensure},
    clap::{
        Args as ClapArgs,
        builder::{PossibleValuesParser, TypedValueParser},
    },
    const_format::concatcp,
    ndhcp::{HttpProvider, HttpProviders, TrustFactorAuthority},
    std::str::FromStr,
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
    /// List of providers that should be disabled (assuming enabled all)
    #[arg(
        long,
        value_name("PROVIDER"),
        value_parser = clap_enum_variants!(HttpProvider),
        env(concatcp!(ENV_PREFIX, "DISABLE")),
        hide_env=true,
    )]
    pub disable: Vec<HttpProvider>,

    /// List of enabled providers.
    /// Computed lately based on all providers and given `disable`.
    #[arg(skip)]
    pub enable: HttpProviders,

    /// Custom trust factors of providers (1 - low, 2 - medium, 3 - high)
    #[arg(
        short='f',
        long,
        value_name("KEY=VALUE"),
        value_parser=Self::parse_trust_factor_pair,
        env(concatcp!(ENV_PREFIX, "TRUST_FACTOR")),
        hide_env=true,
    )]
    pub trust_factor: Vec<(HttpProvider, usize)>,
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

    // https://docs.rs/clap/latest/clap/_derive/_cookbook/typed_derive/index.html
    // https://github.com/clap-rs/clap/blob/f45a32ec/examples/typed-derive.rs#L26
    pub fn parse_trust_factor_pair(s: &str) -> Result<(HttpProvider, usize)> {
        const MIN: usize = TrustFactorAuthority::LOW;
        const MAX: usize = TrustFactorAuthority::HIG;

        let pos = s
            .find('=')
            .or_else(|| s.find(':'))
            .ok_or_else(|| anyhow!("invalid KEY=VALUE: no `=` found in `{s}`"))?;

        let provider_str = &s[..pos];
        let trust_factor_str = &s[pos + 1..];

        let provider = HttpProvider::from_str(provider_str)
            .map_err(|_| anyhow!("provider {} not found", provider_str))?;

        let trust_factor = match usize::from_str(trust_factor_str)? {
            v @MIN..=MAX => v,
            v => bail!(
                "incorrect trust factor {}, must be in range [{}..{}]",
                v,
                MIN,
                MAX
            ),
        };

        Ok((provider, trust_factor))
    }
}
