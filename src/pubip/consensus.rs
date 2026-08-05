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

    /// Whether enough of the enrolled trust answered for this round's silence
    /// to mean anything. A degraded round may add and keep addresses; only a
    /// well answered one may be read as evidence that an address is gone.
    pub well_answered: bool,
}

/// The least trust an address must gather however few providers answered.
///
/// Two, so that a single lowest-trust provider left standing cannot decide on
/// its own, and no higher, so that a medium or high one still can - which is
/// the whole point of weighting them. Capped by the enrolled trust, or asking
/// for one low provider alone would be asking for the impossible.
pub fn floor(enrolled_trust: usize) -> usize {
    const LEAST: usize = 2;
    LEAST.min(enrolled_trust)
}

/// The threshold for one round, taken from the trust that answered it.
///
/// A provider that could not be reached is absent from both sides of the sum
/// rather than only from the numerator, so enrolling a provider an egress
/// policy blocks costs nothing instead of quietly raising the bar forever.
pub fn confirmations_for(
    answered: &[HttpProvider],
    tfa: &TrustFactorAuthority,
    enrolled_trust: usize,
) -> usize {
    let need = if answered.is_empty() {
        0
    } else {
        tfa.calc_confirmation_number(answered)
    };

    need.max(floor(enrolled_trust))
}

/// Whether enough of the enrolled trust turned up to read anything into what
/// this round did not say.
pub fn well_answered(
    answered_trust: usize,
    enrolled_trust: usize,
    tfa: &TrustFactorAuthority,
) -> bool {
    answered_trust >= tfa.trust_share().floor_of(enrolled_trust)
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
        well_answered: false,
    }
}

/// Whether the trust that never arrived could have changed this round.
///
/// True when some address that fell short would have cleared the threshold
/// with it, or when the silent providers could have carried one between them.
/// False means consensus reached the same verdict it would have anyway, and
/// the providers that failed cost the node nothing.
///
/// Pure on purpose, like [`decide`]: this is the whole difference between a
/// log line worth waking up for and one worth noting.
pub fn missing_trust_mattered(report: &Report, missing: usize) -> bool {
    if missing == 0 {
        return false;
    }

    missing >= report.confirmations
        || report
            .unconfirmed
            .values()
            .any(|bucket| bucket + missing >= report.confirmations)
}

#[cfg(test)]
mod tests {
    use {super::*, std::str::FromStr};

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).expect("test address must parse")
    }

    fn trust() -> TrustFactorAuthority {
        TrustFactorAuthority::default()
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
    fn nothing_failed_never_matters() {
        let report = decide(&[(HttpProvider::MyIpWtf, ip("1.1.1.1"))], &trust(), 2);
        assert!(!missing_trust_mattered(&report, 0));
    }

    #[test]
    fn a_failure_beside_a_confirmed_address_did_not_cost_anything() {
        let report = decide(&[(HttpProvider::MyIpWtf, ip("1.1.1.1"))], &trust(), 2);

        assert!(report.confirmed.contains(&ip("1.1.1.1")));
        assert!(!missing_trust_mattered(&report, 1));
    }

    #[test]
    fn a_failure_that_would_have_confirmed_an_address_did() {
        let report = decide(&[(HttpProvider::HttpBin, ip("1.1.1.1"))], &trust(), 2);

        assert_eq!(report.unconfirmed[&ip("1.1.1.1")], 1);
        assert!(missing_trust_mattered(&report, 1));
    }

    #[test]
    fn a_failure_too_small_to_reach_the_threshold_did_not() {
        let report = decide(&[(HttpProvider::HttpBin, ip("1.1.1.1"))], &trust(), 5);
        assert!(!missing_trust_mattered(&report, 1));
    }

    #[test]
    fn every_provider_failing_always_matters() {
        let report = decide(&[], &trust(), 2);

        assert!(report.confirmed.is_empty());
        assert!(report.unconfirmed.is_empty());
        assert!(missing_trust_mattered(&report, 3));
    }

    #[test]
    fn a_second_address_left_short_matters_even_beside_a_confirmed_one() {
        let mut authority = TrustFactorAuthority::default();
        authority.set_trust_factor(HttpProvider::HttpBin, TrustFactorAuthority::HIG);

        let report = decide(
            &[
                (HttpProvider::HttpBin, ip("1.1.1.1")),
                (HttpProvider::MyIpWtf, ip("2606:4700::1111")),
            ],
            &authority,
            3,
        );

        assert!(report.confirmed.contains(&ip("1.1.1.1")));
        assert_eq!(report.unconfirmed[&ip("2606:4700::1111")], 2);
        assert!(missing_trust_mattered(&report, 1));
    }

    #[test]
    fn a_blocked_provider_no_longer_raises_the_bar_it_cannot_help_clear() {
        let tfa = trust();
        let enrolled = 11;

        // All six answer: two thirds of eleven, exactly as before.
        let all = [
            HttpProvider::MyIpWtf,
            HttpProvider::SeeIp,
            HttpProvider::Ipify,
            HttpProvider::MyIpCom,
            HttpProvider::BigDataCloud,
            HttpProvider::MyIpLa,
        ];
        assert_eq!(confirmations_for(&all, &tfa, enrolled), 7);

        // One survivor of six decides for itself, and is believed if its own
        // trust is worth believing.
        let alone = [HttpProvider::MyIpWtf];
        assert_eq!(confirmations_for(&alone, &tfa, enrolled), 2);
    }

    #[test]
    fn the_floor_stops_the_weakest_survivor_from_deciding_alone() {
        let tfa = trust();

        let weakest = [HttpProvider::MyIpLa];
        assert_eq!(confirmations_for(&weakest, &tfa, 11), 2);

        let report = decide(&[(HttpProvider::MyIpLa, ip("1.1.1.1"))], &tfa, 2);
        assert!(report.confirmed.is_empty());
    }

    #[test]
    fn the_floor_never_asks_more_than_was_enrolled() {
        let tfa = trust();

        // One low provider enabled on purpose: it must still be able to work.
        let only = [HttpProvider::MyIpLa];
        assert_eq!(confirmations_for(&only, &tfa, 1), 1);
    }

    #[test]
    fn nobody_answering_confirms_nothing_whatever_the_threshold() {
        let tfa = trust();
        let confirmations = confirmations_for(&[], &tfa, 11);

        assert_eq!(confirmations, 2);
        assert!(decide(&[], &tfa, confirmations).confirmed.is_empty());
    }

    #[test]
    fn a_round_is_well_answered_once_two_thirds_of_the_trust_turned_up() {
        let tfa = trust();

        assert!(well_answered(11, 11, &tfa));
        assert!(well_answered(7, 11, &tfa));
        assert!(!well_answered(6, 11, &tfa));

        // A single enabled provider answering is never a degraded round.
        assert!(well_answered(3, 3, &tfa));
    }

    #[test]
    fn a_custom_trust_factor_changes_the_verdict() {
        let mut tfa = TrustFactorAuthority::default();
        tfa.set_trust_factor(HttpProvider::HttpBin, TrustFactorAuthority::HIG);

        let report = decide(&[(HttpProvider::HttpBin, ip("1.1.1.1"))], &tfa, 2);
        assert!(report.confirmed.contains(&ip("1.1.1.1")));
    }
}
