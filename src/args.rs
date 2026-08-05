use {
    crate::build_info::ENV_PREFIX,
    crate::pubip::{HttpProvider, TrustFactorAuthority},
    anyhow::{Error, Result, anyhow, bail, ensure},
    clap::{
        Args as ClapArgs,
        builder::{PossibleValuesParser, TypedValueParser},
    },
    const_format::concatcp,
    humantime::parse_duration,
    std::{str::FromStr, time::Duration as StdDuration},
    strum::{VariantArray, VariantNames},
    tracing::warn,
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

#[derive(Clone, Default, ClapArgs)]
pub struct OfProviders {
    /// List of providers to ask, replacing the default set entirely
    #[arg(
        long,
        value_name("PROVIDER"),
        value_delimiter = ',',
        ignore_case = true,
        value_parser = clap_enum_variants!(HttpProvider),
        env(concatcp!(ENV_PREFIX, "PROVIDERS")),
        hide_env=true,
    )]
    pub providers: Vec<HttpProvider>,

    /// Deprecated, use `--providers` instead
    #[arg(
        long,
        hide = true,
        value_name("PROVIDER"),
        value_delimiter = ',',
        ignore_case = true,
        value_parser = clap_enum_variants!(HttpProvider),
        env(concatcp!(ENV_PREFIX, "ENABLE")),
        hide_env=true,
    )]
    pub enable: Vec<HttpProvider>,

    /// Deprecated, use `--providers` instead
    #[arg(
        long,
        hide = true,
        value_name("PROVIDER"),
        value_delimiter = ',',
        ignore_case = true,
        value_parser = clap_enum_variants!(HttpProvider),
        env(concatcp!(ENV_PREFIX, "DISABLE")),
        hide_env=true,
    )]
    pub disable: Vec<HttpProvider>,

    /// The providers this run will actually ask.
    /// Computed lately by [`Self::setup`] from `enable` and `disable`.
    #[arg(skip)]
    pub enabled: Vec<HttpProvider>,

    /// Ask every provider every round, whatever rate limit it publishes
    #[arg(
        long,
        default_value_t=false,
        default_missing_value="true",
        num_args=0..=1,
        value_name="BOOL",
        hide_default_value=true,
        hide_possible_values=true,
        env(concatcp!(ENV_PREFIX, "IGNORE_RATE_LIMITS")),
        hide_env=true,
    )]
    pub ignore_rate_limits: bool,

    /// Custom rate limits of providers, zero to lift one entirely
    #[arg(
        short='r',
        long,
        value_name("KEY=DURATION"),
        value_delimiter=',',
        value_parser=Self::parse_rate_limit_pair,
        env(concatcp!(ENV_PREFIX, "RATE_LIMIT")),
        hide_env=true,
    )]
    pub rate_limit: Vec<(HttpProvider, StdDuration)>,

    /// Custom trust factors of providers (1 - low, 2 - medium, 3 - high)
    #[arg(
        short='f',
        long,
        value_name("KEY=VALUE"),
        value_delimiter=',',
        value_parser=Self::parse_trust_factor_pair,
        env(concatcp!(ENV_PREFIX, "TRUST_FACTOR")),
        hide_env=true,
    )]
    pub trust_factor: Vec<(HttpProvider, usize)>,
}

impl OfProviders {
    /// Works out which providers this run will ask.
    ///
    /// `--providers` names them outright, replacing the default set rather than
    /// adding to it, so that naming the two you want does not also mean naming
    /// the five you do not.
    ///
    /// ```text
    ///   nothing given    ->  the providers enabled by default
    ///   --providers A B  ->  exactly A and B
    ///   --enable A B     ->  exactly A and B, deprecated
    ///   --disable A      ->  the default set without A, deprecated
    /// ```
    pub fn setup(&mut self) -> Result<()> {
        let deprecated = !self.enable.is_empty() || !self.disable.is_empty();

        ensure!(
            !deprecated || self.providers.is_empty(),
            "--providers cannot be combined with --enable or --disable, which it replaces"
        );

        if deprecated {
            warn!("--enable and --disable are deprecated, --providers replaces both");
        }

        let by_default = self.providers.is_empty() && self.enable.is_empty();

        let base: &[HttpProvider] = if !self.providers.is_empty() {
            &self.providers
        } else if !self.enable.is_empty() {
            &self.enable
        } else {
            <HttpProvider as VariantArray>::VARIANTS
        };

        let mut enabled = Vec::with_capacity(base.len());
        for provider in base {
            let wanted = !by_default || provider.enabled_by_default();

            // The last condition keeps a provider named twice from paying its
            // trust factor twice into the same address' bucket.
            if wanted && !self.disable.contains(provider) && !enabled.contains(provider) {
                enabled.push(*provider);
            }
        }

        ensure!(!enabled.is_empty(), "at least one provider must be enabled");
        self.enabled = enabled;

        Ok(())
    }

    // https://docs.rs/clap/latest/clap/_derive/_cookbook/typed_derive/index.html
    // https://github.com/clap-rs/clap/blob/f45a32ec/examples/typed-derive.rs#L26
    pub fn parse_trust_factor_pair(s: &str) -> Result<(HttpProvider, usize)> {
        const MIN: usize = TrustFactorAuthority::LOW;
        const MAX: usize = TrustFactorAuthority::HIG;

        let (provider, value) = Self::split_pair(s)?;

        let trust_factor = match usize::from_str(value)? {
            v @ MIN..=MAX => v,
            v => bail!("incorrect trust factor {v}, must be in range [{MIN}..{MAX}]"),
        };

        Ok((provider, trust_factor))
    }

    /// A gap of zero is meaningful and kept: it lifts a published rate limit
    /// rather than restoring it, which is the escape hatch for a provider whose
    /// stated limit has moved on without this table.
    pub fn parse_rate_limit_pair(s: &str) -> Result<(HttpProvider, StdDuration)> {
        let (provider, value) = Self::split_pair(s)?;
        let gap = parse_duration(value).map_err(Error::msg)?;

        Ok((provider, gap))
    }

    fn split_pair(s: &str) -> Result<(HttpProvider, &str)> {
        let pos = s
            .find('=')
            .or_else(|| s.find(':'))
            .ok_or_else(|| anyhow!("invalid KEY=VALUE: no `=` found in `{s}`"))?;

        let name = &s[..pos];
        let provider =
            HttpProvider::from_str(name).map_err(|_| anyhow!("provider {name} not found"))?;

        Ok((provider, &s[pos + 1..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(of: OfProviders) -> Result<Vec<HttpProvider>> {
        let mut of = of;
        of.setup()?;
        Ok(of.enabled)
    }

    fn asked(enable: &[HttpProvider], disable: &[HttpProvider]) -> Result<Vec<HttpProvider>> {
        resolve(OfProviders {
            enable: enable.to_vec(),
            disable: disable.to_vec(),
            ..OfProviders::default()
        })
    }

    fn must_ask(enable: &[HttpProvider], disable: &[HttpProvider]) -> Vec<HttpProvider> {
        asked(enable, disable).expect("this combination must leave a provider standing")
    }

    fn named(providers: &[HttpProvider]) -> Result<Vec<HttpProvider>> {
        resolve(OfProviders {
            providers: providers.to_vec(),
            ..OfProviders::default()
        })
    }

    #[test]
    fn nothing_given_asks_the_providers_enabled_by_default() {
        let enabled = must_ask(&[], &[]);

        assert!(!enabled.contains(&HttpProvider::HttpBin));
        assert!(enabled.contains(&HttpProvider::Ipify));

        assert!(enabled.iter().all(|p| p.enabled_by_default()));
        assert_eq!(
            enabled.len(),
            <HttpProvider as VariantArray>::VARIANTS
                .iter()
                .filter(|p| p.enabled_by_default())
                .count(),
        );
    }

    #[test]
    fn enable_replaces_the_default_set_rather_than_adding_to_it() {
        let enabled = must_ask(&[HttpProvider::HttpBin], &[]);
        assert_eq!(enabled, vec![HttpProvider::HttpBin]);
    }

    #[test]
    fn disable_subtracts_from_the_default_set() {
        let enabled = must_ask(&[], &[HttpProvider::Ipify]);

        assert!(!enabled.contains(&HttpProvider::Ipify));
        assert!(enabled.contains(&HttpProvider::SeeIp));
    }

    #[test]
    fn disable_subtracts_from_an_explicit_set_too() {
        let enabled = must_ask(
            &[HttpProvider::HttpBin, HttpProvider::MyIpWtf],
            &[HttpProvider::HttpBin],
        );

        assert_eq!(enabled, vec![HttpProvider::MyIpWtf]);
    }

    #[test]
    fn a_provider_named_twice_is_asked_once() {
        let enabled = must_ask(&[HttpProvider::MyIpWtf, HttpProvider::MyIpWtf], &[]);
        assert_eq!(enabled, vec![HttpProvider::MyIpWtf]);
    }

    #[test]
    fn providers_replaces_the_default_set() {
        let enabled = named(&[HttpProvider::HttpBin, HttpProvider::SeeIp])
            .expect("two named providers must resolve");

        assert_eq!(enabled, vec![HttpProvider::HttpBin, HttpProvider::SeeIp]);
    }

    #[test]
    fn providers_named_twice_is_asked_once() {
        let enabled = named(&[HttpProvider::SeeIp, HttpProvider::SeeIp])
            .expect("a repeated provider must resolve");

        assert_eq!(enabled, vec![HttpProvider::SeeIp]);
    }

    #[test]
    fn providers_refuses_to_share_the_run_with_the_flags_it_replaces() {
        let with_enable = OfProviders {
            providers: vec![HttpProvider::SeeIp],
            enable: vec![HttpProvider::Ipify],
            ..OfProviders::default()
        };

        let with_disable = OfProviders {
            providers: vec![HttpProvider::SeeIp],
            disable: vec![HttpProvider::Ipify],
            ..OfProviders::default()
        };

        assert!(resolve(with_enable).is_err());
        assert!(resolve(with_disable).is_err());
    }

    #[test]
    fn leaving_nothing_enabled_is_an_error() {
        let disable: Vec<_> = <HttpProvider as VariantArray>::VARIANTS.to_vec();
        assert!(asked(&[], &disable).is_err());
    }

    #[test]
    fn a_list_of_providers_may_be_one_flag_or_several() {
        #[derive(clap::Parser)]
        struct Wrapper {
            #[command(flatten)]
            providers: OfProviders,
        }

        let parse = |args: &[&str]| {
            <Wrapper as clap::Parser>::try_parse_from(args)
                .expect("these arguments must parse")
                .providers
        };

        let commas = parse(&["fckloud", "--enable", "HttpBin,MyIpWtf"]);
        let repeats = parse(&["fckloud", "--enable", "HttpBin", "--enable", "MyIpWtf"]);

        let both = vec![HttpProvider::HttpBin, HttpProvider::MyIpWtf];
        assert_eq!(commas.enable, both);
        assert_eq!(commas.enable, repeats.enable);

        let factors = parse(&["fckloud", "--trust-factor", "HttpBin=3,MyIpWtf=1"]);
        assert_eq!(
            factors.trust_factor,
            vec![(HttpProvider::HttpBin, 3), (HttpProvider::MyIpWtf, 1)],
        );

        let gaps = parse(&["fckloud", "--rate-limit", "MyIpWtf=90s,Ipify=0s"]);
        assert_eq!(
            gaps.rate_limit,
            vec![
                (HttpProvider::MyIpWtf, StdDuration::from_secs(90)),
                (HttpProvider::Ipify, StdDuration::ZERO),
            ],
        );
    }

    #[test]
    fn a_rate_limit_needs_a_known_provider_and_a_duration() {
        assert!(OfProviders::parse_rate_limit_pair("MyIpWtf=1m").is_ok());
        assert!(OfProviders::parse_rate_limit_pair("MyIpWtf:1m").is_ok());

        assert!(OfProviders::parse_rate_limit_pair("MyIpWtf").is_err());
        assert!(OfProviders::parse_rate_limit_pair("Nope=1m").is_err());
        assert!(OfProviders::parse_rate_limit_pair("MyIpWtf=soon").is_err());
    }
}
