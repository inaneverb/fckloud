use {
    crate::pubip::{HttpProvider, TrustFactorAuthority},
    std::{
        collections::{HashMap, HashSet},
        net::IpAddr,
    },
};

/// What a round of asking the providers came to.
#[derive(Default, Debug)]
pub struct Report {
    pub confirmations: usize,
    pub confirmed: HashSet<IpAddr>,
    pub unconfirmed: HashMap<IpAddr, usize>,
}

/// Weighs what the providers reported and decides which addresses carry enough
/// trust to be believed.
///
/// Pure on purpose: every subtlety of the consensus lives here, and none of it
/// needs a network to exercise.
pub fn decide(
    reported: &[(HttpProvider, IpAddr)],
    tfa: &TrustFactorAuthority,
    confirmations: usize,
) -> Report {
    let mut buckets: HashMap<IpAddr, usize> = HashMap::new();

    for (provider, ip_addr) in reported {
        *buckets.entry(*ip_addr).or_default() += tfa.trust_factor(*provider);
    }

    let (confirmed, unconfirmed) = buckets
        .into_iter()
        .partition::<HashMap<_, _>, _>(|(_, bucket)| *bucket >= confirmations);

    Report {
        confirmations,
        confirmed: confirmed.into_keys().collect(),
        unconfirmed,
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::str::FromStr};

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).expect("test address must parse")
    }

    #[test]
    fn a_lone_low_trust_provider_does_not_reach_the_default_threshold() {
        let tfa = TrustFactorAuthority::default();
        let report = decide(&[(HttpProvider::HttpBin, ip("1.1.1.1"))], &tfa, 2);

        assert!(report.confirmed.is_empty());
        assert_eq!(report.unconfirmed[&ip("1.1.1.1")], 1);
    }

    #[test]
    fn a_lone_medium_trust_provider_does() {
        let tfa = TrustFactorAuthority::default();
        let report = decide(&[(HttpProvider::MyIpWtf, ip("1.1.1.1"))], &tfa, 2);

        assert!(report.confirmed.contains(&ip("1.1.1.1")));
        assert!(report.unconfirmed.is_empty());
    }

    #[test]
    fn trust_accumulates_across_providers_reporting_the_same_address() {
        let tfa = TrustFactorAuthority::default();
        let report = decide(
            &[
                (HttpProvider::HttpBin, ip("1.1.1.1")),
                (HttpProvider::MyIpWtf, ip("1.1.1.1")),
            ],
            &tfa,
            3,
        );

        assert!(report.confirmed.contains(&ip("1.1.1.1")));
    }

    #[test]
    fn providers_that_disagree_split_the_trust_and_neither_wins() {
        let tfa = TrustFactorAuthority::default();
        let report = decide(
            &[
                (HttpProvider::HttpBin, ip("1.1.1.1")),
                (HttpProvider::MyIpWtf, ip("2.2.2.2")),
            ],
            &tfa,
            3,
        );

        assert!(report.confirmed.is_empty());
        assert_eq!(report.unconfirmed[&ip("1.1.1.1")], 1);
        assert_eq!(report.unconfirmed[&ip("2.2.2.2")], 2);
    }

    #[test]
    fn nothing_reported_confirms_nothing() {
        let tfa = TrustFactorAuthority::default();
        let report = decide(&[], &tfa, 2);

        assert!(report.confirmed.is_empty());
        assert!(report.unconfirmed.is_empty());
    }

    #[test]
    fn a_custom_trust_factor_changes_the_verdict() {
        let mut tfa = TrustFactorAuthority::default();
        tfa.set_trust_factor(HttpProvider::HttpBin, TrustFactorAuthority::HIG);

        let report = decide(&[(HttpProvider::HttpBin, ip("1.1.1.1"))], &tfa, 2);
        assert!(report.confirmed.contains(&ip("1.1.1.1")));
    }
}
