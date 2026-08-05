use {
    crate::build_info::ENV_PREFIX,
    crate::pubip::{self, HttpProvider, Set, Token, TrustFactorAuthority},
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

/// Keeps a provider named twice from paying its trust factor twice into the
/// same address' bucket.
fn push_once(providers: &mut Vec<HttpProvider>, provider: HttpProvider) {
    if !providers.contains(&provider) {
        providers.push(provider);
    }
}

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
    /// Providers to ask: their names, a version to pin (v1.5), or a set
    /// (all, default, trust1, trust2, trust3, also low, med, hig)
    #[arg(
        long,
        value_name("NAME"),
        value_delimiter = ',',
        value_parser = Self::parse_provider_token,
        env(concatcp!(ENV_PREFIX, "PROVIDERS")),
        hide_env=true,
    )]
    pub providers: Vec<Token>,

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
    ///   nothing given      ->  the providers enabled by default
    ///   --providers A,med  ->  A and everything at medium trust or above
    ///   --providers v1.5   ->  exactly what v1.5 asked, and nothing beside it
    ///   --enable A B       ->  exactly A and B, deprecated
    ///   --disable A        ->  the default set without A, deprecated
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

        let enabled = if self.providers.is_empty() {
            self.resolve_deprecated()
        } else {
            self.resolve_tokens()?
        };

        ensure!(!enabled.is_empty(), "at least one provider must be enabled");
        self.enabled = enabled;

        Ok(())
    }

    /// The trust factors this run works with, the operator's overrides applied.
    pub fn trust_authority(&self) -> TrustFactorAuthority {
        let mut tfa = TrustFactorAuthority::default();
        for (provider, trust_factor) in &self.trust_factor {
            tfa.set_trust_factor(*provider, *trust_factor);
        }

        tfa
    }

    /// Unions what every name stands for, in the order they were given.
    ///
    /// A version is exclusive: pinning means reproducing exactly what that
    /// release asked, and a pin that can be amended is not one.
    fn resolve_tokens(&self) -> Result<Vec<HttpProvider>> {
        let tfa = self.trust_authority();
        let pinned = self.providers.iter().find(|token| match token {
            Token::Set(set) => set.pins(),
            Token::Provider(_) => false,
        });

        if let Some(pin) = pinned {
            ensure!(
                self.providers.len() == 1,
                "a pinned provider set stands alone and cannot be combined with anything else",
            );

            if let Token::Set(Set::Version(version)) = pin
                && !pubip::released(*version)
            {
                warn!(%version, "no such release; the newest set at or below it is used instead");
            }
        }

        let mut enabled = Vec::new();
        for token in &self.providers {
            match token {
                Token::Provider(provider) => push_once(&mut enabled, *provider),
                Token::Set(set) => {
                    for provider in set.members(&tfa) {
                        push_once(&mut enabled, provider);
                    }

                    for skipped in set.skipped(&tfa) {
                        warn!(
                            provider = %skipped,
                            "provider is off by default and no set enables it, name it outright",
                        );
                    }
                }
            }
        }

        Ok(enabled)
    }

    fn resolve_deprecated(&self) -> Vec<HttpProvider> {
        let by_default = self.enable.is_empty();

        let base: &[HttpProvider] = if by_default {
            <HttpProvider as VariantArray>::VARIANTS
        } else {
            &self.enable
        };

        let mut enabled = Vec::with_capacity(base.len());
        for provider in base {
            let wanted = !by_default || provider.enabled_by_default();

            if wanted && !self.disable.contains(provider) {
                push_once(&mut enabled, *provider);
            }
        }

        enabled
    }

    pub fn parse_provider_token(s: &str) -> Result<Token> {
        pubip::parse_provider_token(s).map_err(|err| anyhow!("{err}"))
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

    fn tokens(names: &[&str]) -> Vec<Token> {
        names
            .iter()
            .map(|name| {
                OfProviders::parse_provider_token(name)
                    .unwrap_or_else(|err| panic!("`{name}` must parse: {err}"))
            })
            .collect()
    }

    fn named(names: &[&str]) -> Result<Vec<HttpProvider>> {
        resolve(OfProviders {
            providers: tokens(names),
            ..OfProviders::default()
        })
    }

    fn must_name(names: &[&str]) -> Vec<HttpProvider> {
        named(names).unwrap_or_else(|err| panic!("{names:?} must resolve: {err}"))
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
        assert_eq!(
            must_name(&["HttpBin", "SeeIp"]),
            vec![HttpProvider::HttpBin, HttpProvider::SeeIp],
        );
    }

    #[test]
    fn providers_named_twice_is_asked_once() {
        assert_eq!(must_name(&["SeeIp", "seeip"]), vec![HttpProvider::SeeIp]);
    }

    #[test]
    fn a_set_and_a_provider_beside_it_are_unioned() {
        let enabled = must_name(&["hig", "HttpBin"]);
        assert_eq!(enabled, vec![HttpProvider::Ipify, HttpProvider::HttpBin]);
    }

    #[test]
    fn a_set_never_reaches_a_provider_that_is_off_by_default() {
        assert!(!must_name(&["trust1"]).contains(&HttpProvider::HttpBin));
        assert!(must_name(&["all"]).contains(&HttpProvider::HttpBin));
    }

    #[test]
    fn a_pinned_version_stands_alone() {
        assert_eq!(must_name(&["v1.2"]).len(), 2);

        assert!(named(&["v1.2", "Ipify"]).is_err());
        assert!(named(&["v1.2", "v1.5"]).is_err());
        assert!(named(&["v1.2", "all"]).is_err());
    }

    #[test]
    fn a_version_newer_than_this_build_is_refused_outright() {
        assert!(OfProviders::parse_provider_token("v9.9").is_err());
    }

    #[test]
    fn providers_refuses_to_share_the_run_with_the_flags_it_replaces() {
        let with_enable = OfProviders {
            providers: tokens(&["SeeIp"]),
            enable: vec![HttpProvider::Ipify],
            ..OfProviders::default()
        };

        let with_disable = OfProviders {
            providers: tokens(&["SeeIp"]),
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
