use {
    crate::pubip::{HttpProvider, TrustFactorAuthority},
    std::{cmp::Ordering, fmt, str::FromStr},
    strum::VariantArray,
};

/// A `MAJOR.MINOR.PATCH` triple, ordered the way releases are.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version(u16, u16, u16);

/// The releases that changed which providers are asked by default.
///
/// Sparse on purpose: a version between two entries resolves to the one below
/// it, so v1.3 and v1.4 both answer what v1.2.0 answered. Once a row is here it
/// is frozen - that immutability is the whole point of pinning to a version.
const CHANGEPOINTS: [(Version, &[HttpProvider]); 3] = [
    (Version(1, 0, 0), &[HttpProvider::HttpBin]),
    (
        Version(1, 2, 0),
        &[HttpProvider::HttpBin, HttpProvider::MyIpWtf],
    ),
    (
        Version(1, 5, 0),
        &[
            HttpProvider::MyIpWtf,
            HttpProvider::SeeIp,
            HttpProvider::Ipify,
            HttpProvider::MyIpCom,
            HttpProvider::BigDataCloud,
            HttpProvider::MyIpLa,
        ],
    ),
];

/// Every version ever released, newest last. The last entry is this build, and
/// a test says so: bumping the version means adding a line here.
const RELEASES: [Version; 8] = [
    Version(1, 0, 0),
    Version(1, 1, 0),
    Version(1, 2, 0),
    Version(1, 3, 0),
    Version(1, 4, 0),
    Version(1, 5, 0),
    Version(1, 6, 0),
    Version(1, 7, 0),
];

/// A name `--providers` accepts that stands for more than one provider.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Set {
    /// A released version, reproducing what that version asked by default.
    /// The only kind that pins.
    Version(Version),
    /// Every provider there is, the ones off by default included.
    All,
    /// What this build asks when nothing is said.
    Default,
    /// Every provider on by default carrying at least this trust factor.
    Trust(usize),
}

/// Why a name could not be turned into a set of providers.
#[derive(Debug)]
pub enum Rejected {
    Unknown(String),
    FromTheFuture(Version),
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(name) => write!(f, "`{name}` is neither a provider nor a set"),
            Self::FromTheFuture(version) => write!(
                f,
                "provider set {version} is newer than this build knows; \
                 the image is older than the manifest that asked for it",
            ),
        }
    }
}

impl Set {
    /// Whether choosing this set means the same providers after an upgrade.
    pub const fn pins(self) -> bool {
        matches!(self, Self::Version(_))
    }

    /// The providers the set stands for.
    ///
    /// Only [`Set::All`] and a provider named outright reach one that is off by
    /// default. Every other set leaves it where the defaults put it, which is
    /// what stops a broad name from quietly re-enabling something withdrawn for
    /// misbehaving.
    pub fn members(self, tfa: &TrustFactorAuthority) -> Vec<HttpProvider> {
        let all = <HttpProvider as VariantArray>::VARIANTS;

        match self {
            Self::Version(version) => resolve(version).to_vec(),
            Self::All => all.to_vec(),
            Self::Default => all
                .iter()
                .copied()
                .filter(|p| p.enabled_by_default())
                .collect(),
            Self::Trust(least) => all
                .iter()
                .copied()
                .filter(|p| p.enabled_by_default() && tfa.trust_factor(*p) >= least)
                .collect(),
        }
    }

    /// The providers this set left behind only because they are off by default.
    /// Worth saying out loud: a broad name not covering everything surprises.
    pub fn skipped(self, tfa: &TrustFactorAuthority) -> Vec<HttpProvider> {
        if matches!(self, Self::All | Self::Version(_)) {
            return Vec::new();
        }

        let taken = self.members(tfa);
        <HttpProvider as VariantArray>::VARIANTS
            .iter()
            .copied()
            .filter(|p| !p.enabled_by_default() && !taken.contains(p))
            .collect()
    }
}

/// The newest changepoint at or below the given version.
fn resolve(version: Version) -> &'static [HttpProvider] {
    CHANGEPOINTS
        .iter()
        .rev()
        .find(|(at, _)| *at <= version)
        .map_or(&[], |(_, providers)| *providers)
}

/// Whether the version was ever released, and whether this build has heard of
/// it at all.
pub fn released(version: Version) -> bool {
    RELEASES.contains(&version)
}

pub fn newest_release() -> Version {
    *RELEASES.last().expect("there is always a release")
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(major, minor, patch) = self;
        write!(f, "v{major}.{minor}.{patch}")
    }
}

impl FromStr for Version {
    type Err = ();

    /// `v1.5` and `1.5.0` alike. A bare major is refused: `v1` would follow
    /// whatever the newest v1 happens to be, which is the opposite of a pin.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits = s
            .strip_prefix('v')
            .or_else(|| s.strip_prefix('V'))
            .ok_or(())?;
        let mut parts = digits.split('.');

        let major = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        let minor = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        let patch = match parts.next() {
            Some(patch) => patch.parse().map_err(|_| ())?,
            None => 0,
        };

        if parts.next().is_some() {
            return Err(());
        }

        Ok(Self(major, minor, patch))
    }
}

/// One name given to `--providers`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Token {
    Provider(HttpProvider),
    Set(Set),
}

/// Turns one `--providers` name into a provider or a set of them.
pub fn parse_token(name: &str) -> Result<Token, Rejected> {
    match HttpProvider::from_str(name) {
        Ok(provider) => Ok(Token::Provider(provider)),
        Err(_) => parse(name).map(Token::Set),
    }
}

/// Turns one `--providers` name into a set, or reports why it cannot.
pub fn parse(name: &str) -> Result<Set, Rejected> {
    if let Ok(version) = Version::from_str(name) {
        return match version.cmp(&newest_release()) {
            Ordering::Greater => Err(Rejected::FromTheFuture(version)),
            _ => Ok(Set::Version(version)),
        };
    }

    // `all` and `trust1` name every trust factor there is and are still not the
    // same set: only `all` reaches a provider that is off by default.
    let set = match name.to_ascii_lowercase().as_str() {
        "all" => Set::All,
        "default" => Set::Default,
        "trust1" | "low" => Set::Trust(TrustFactorAuthority::LOW),
        "trust2" | "med" | "medium" => Set::Trust(TrustFactorAuthority::MED),
        "trust3" | "hig" | "high" => Set::Trust(TrustFactorAuthority::HIG),
        _ => return Err(Rejected::Unknown(name.to_owned())),
    };

    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(s: &str) -> Version {
        Version::from_str(s).unwrap_or_else(|()| panic!("`{s}` must parse as a version"))
    }

    fn members(name: &str) -> Vec<HttpProvider> {
        parse(name)
            .unwrap_or_else(|err| panic!("`{name}` must resolve: {err}"))
            .members(&TrustFactorAuthority::default())
    }

    #[test]
    fn the_newest_release_is_the_version_this_build_carries() {
        assert_eq!(
            newest_release().to_string(),
            format!("v{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn a_version_resolves_to_the_newest_set_at_or_below_it() {
        assert_eq!(resolve(version("v1.0")), &[HttpProvider::HttpBin]);
        assert_eq!(resolve(version("v1.1")), &[HttpProvider::HttpBin]);
        assert_eq!(resolve(version("v1.4")), resolve(version("v1.2")));
        assert_eq!(resolve(version("v1.7")), resolve(version("v1.5")));
    }

    #[test]
    fn a_bare_major_is_not_a_version() {
        assert!(Version::from_str("v1").is_err());
        assert!(Version::from_str("v1.2.3.4").is_err());
        assert!(Version::from_str("1.2").is_err());
    }

    #[test]
    fn a_version_this_build_has_never_heard_of_is_refused() {
        assert!(matches!(parse("v9.9"), Err(Rejected::FromTheFuture(_))));
        assert!(parse("v1.2").is_ok());
    }

    #[test]
    fn only_all_reaches_a_provider_that_is_off_by_default() {
        assert!(members("all").contains(&HttpProvider::HttpBin));

        for name in ["trust1", "low", "default", "trust2", "hig"] {
            assert!(
                !members(name).contains(&HttpProvider::HttpBin),
                "`{name}` must not enable a provider that is off by default",
            );
        }
    }

    #[test]
    fn a_trust_set_takes_that_factor_and_above() {
        assert_eq!(members("hig"), vec![HttpProvider::Ipify]);

        let med = members("med");
        assert!(med.contains(&HttpProvider::Ipify));
        assert!(med.contains(&HttpProvider::SeeIp));
        assert!(!med.contains(&HttpProvider::MyIpLa));
    }

    #[test]
    fn only_a_version_pins() {
        assert!(parse("v1.5").expect("a release must parse").pins());

        for name in ["all", "default", "trust1", "med", "hig"] {
            assert!(
                !parse(name).expect("a set must parse").pins(),
                "{name} pins"
            );
        }
    }

    #[test]
    fn a_name_that_is_neither_is_refused() {
        assert!(matches!(parse("nonsense"), Err(Rejected::Unknown(_))));
    }

    // What every release asked for, written down once. A row here is a promise
    // to everyone who pinned that version, so this test failing means either a
    // changepoint was edited - which is the promise broken - or a new release
    // changed the set and owes itself a line.
    const GOLDEN: [(&str, &[&str]); 8] = [
        ("v1.0.0", &["httpbin.org"]),
        ("v1.1.0", &["httpbin.org"]),
        ("v1.2.0", &["httpbin.org", "myip.wtf"]),
        ("v1.3.0", &["httpbin.org", "myip.wtf"]),
        ("v1.4.0", &["httpbin.org", "myip.wtf"]),
        ("v1.5.0", &SIX),
        ("v1.6.0", &SIX),
        ("v1.7.0", &SIX),
    ];

    const SIX: [&str; 6] = [
        "myip.wtf",
        "api.seeip.org",
        "api64.ipify.org",
        "api.myip.com",
        "api.bigdatacloud.net",
        "api.myip.la",
    ];

    #[test]
    fn every_released_set_is_still_what_it_was() {
        for (release, expected) in GOLDEN {
            let hosts: Vec<&str> = resolve(version(release))
                .iter()
                .map(|provider| provider.host())
                .collect();

            assert_eq!(hosts, expected, "the set {release} asked for has moved");
        }
    }

    #[test]
    fn every_release_has_a_golden_row() {
        for release in RELEASES {
            assert!(
                GOLDEN.iter().any(|(at, _)| version(at) == release),
                "{release} has no row saying what it asked for",
            );
        }
    }

    #[test]
    fn an_unreleased_version_below_this_build_still_resolves() {
        assert!(!released(version("v1.2.7")));
        assert!(parse("v1.2.7").is_ok());
    }
}
