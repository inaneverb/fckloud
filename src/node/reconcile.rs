use {
    k8s_openapi::api::core::v1::NodeAddress,
    std::{
        collections::{BTreeMap, BTreeSet},
        net::IpAddr,
        str::FromStr,
        time::{Duration, Instant},
    },
    strum::EnumIs,
    tracing::warn,
};

pub const TYPE_EXTERNAL_IP: &str = "ExternalIP";

#[derive(EnumIs, Debug, PartialEq, Eq)]
pub enum AddrStatus {
    New,
    Skipped,
    Removed,
}

/// What the node's addresses should become, and what changed getting there.
pub struct Outcome {
    pub addresses: Vec<NodeAddress>,
    pub report: BTreeMap<IpAddr, AddrStatus>,
    pub has_changes: bool,
    pub pending: BTreeMap<IpAddr, Pending>,
}

/// How long an address consensus stopped vouching for has been on notice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pending {
    pub since: Instant,
    pub misses: usize,
}

/// When an address consensus no longer vouches for may be torn off the node.
#[derive(Clone, Copy, Debug)]
pub enum Removal {
    /// Never. What the operator gets without asking for anything.
    Never,
    /// Once the grace has elapsed and enough well answered rounds have gone
    /// without mentioning it.
    After(Duration),
    /// The same round, as the deprecated `--strict` has always done.
    AtOnce,
}

/// Well answered rounds an address must be missing from before it goes, so
/// that one round nobody could corroborate cannot cost the node an address.
const MISSES: usize = 2;

/// Decides the node's new address list from its current one and the addresses
/// consensus vouched for.
///
/// Pure on purpose. Every branch that decides whether an address is added,
/// kept or torn off a live node is decided here, where a test can reach it.
pub fn reconcile(
    current: Vec<NodeAddress>,
    staged: &BTreeSet<IpAddr>,
    removal: Removal,
    well_answered: bool,
    was_pending: &BTreeMap<IpAddr, Pending>,
    now: Instant,
) -> Outcome {
    let mut report = BTreeMap::new();
    let mut addresses = Vec::with_capacity(current.len() + staged.len());
    let mut pending = BTreeMap::new();
    let mut has_changes = false;

    // The node keeps its Hostname and InternalIP untouched; only the
    // ExternalIPs are ours to decide upon. An unparsable one is somebody
    // else's business, so it is preserved and complained about.

    for address in current {
        let Some(external_ip) = parse_external_ip(&address) else {
            addresses.push(address);
            continue;
        };

        if staged.contains(&external_ip) {
            addresses.push(address);
            report.insert(external_ip, AddrStatus::Skipped);
            continue;
        }

        let verdict = condemn(
            removal,
            well_answered,
            was_pending.get(&external_ip).copied(),
            now,
        );

        match verdict {
            Verdict::Remove => {
                has_changes = true;
                report.insert(external_ip, AddrStatus::Removed);
            }
            Verdict::Keep(still) => {
                addresses.push(address);
                report.insert(external_ip, AddrStatus::Skipped);

                if let Some(still) = still {
                    pending.insert(external_ip, still);
                }
            }
        }
    }

    for external_ip in staged {
        if report.contains_key(external_ip) {
            continue;
        }

        addresses.push(new_external_ip(external_ip));
        report.insert(*external_ip, AddrStatus::New);
        has_changes = true;
    }

    Outcome {
        addresses,
        report,
        has_changes,
        pending,
    }
}

enum Verdict {
    Remove,
    Keep(Option<Pending>),
}

/// Whether an address consensus did not vouch for this round has run out of
/// rope, and what it still owes if it has not.
///
/// A degraded round is not evidence of anything: it neither condemns an address
/// nor lets one off, so the clock simply does not move.
fn condemn(
    removal: Removal,
    well_answered: bool,
    pending: Option<Pending>,
    now: Instant,
) -> Verdict {
    let grace = match removal {
        Removal::Never => return Verdict::Keep(None),
        Removal::AtOnce => return Verdict::Remove,
        Removal::After(grace) => grace,
    };

    if !well_answered {
        return Verdict::Keep(pending);
    }

    let noted = pending.unwrap_or(Pending {
        since: now,
        misses: 0,
    });

    let counted = Pending {
        since: noted.since,
        misses: noted.misses + 1,
    };

    let waited = now.saturating_duration_since(counted.since) >= grace;
    if waited && counted.misses >= MISSES {
        Verdict::Remove
    } else {
        Verdict::Keep(Some(counted))
    }
}

/// Returns the parsed address if it is an `ExternalIP`, [`None`] otherwise.
/// An `ExternalIP` that fails to parse is reported and treated as not ours.
pub fn parse_external_ip(node_address: &NodeAddress) -> Option<IpAddr> {
    if node_address.type_ != TYPE_EXTERNAL_IP {
        return None;
    }

    IpAddr::from_str(&node_address.address)
        .inspect_err(|err| warn!(address = node_address.address, %err, "unparsable ExternalIP"))
        .ok()
}

pub fn new_external_ip(ip: &IpAddr) -> NodeAddress {
    NodeAddress {
        address: ip.to_string(),
        type_: TYPE_EXTERNAL_IP.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).expect("test address must parse")
    }

    fn addr(type_: &str, address: &str) -> NodeAddress {
        NodeAddress {
            address: address.into(),
            type_: type_.into(),
        }
    }

    fn staged(addresses: &[&str]) -> BTreeSet<IpAddr> {
        addresses.iter().map(|a| ip(a)).collect()
    }

    fn node() -> Vec<NodeAddress> {
        vec![
            addr("InternalIP", "192.168.1.1"),
            addr("Hostname", "some-node"),
        ]
    }

    const GRACE: Duration = Duration::from_mins(5);

    fn reconcile(
        current: Vec<NodeAddress>,
        staged: &BTreeSet<IpAddr>,
        remove_unstaged: bool,
    ) -> Outcome {
        let removal = if remove_unstaged {
            Removal::AtOnce
        } else {
            Removal::Never
        };

        super::reconcile(
            current,
            staged,
            removal,
            true,
            &BTreeMap::new(),
            Instant::now(),
        )
    }

    /// Runs one round in grace mode and hands back what it decided.
    fn round(
        stale: &str,
        well_answered: bool,
        pending: &BTreeMap<IpAddr, Pending>,
        now: Instant,
    ) -> Outcome {
        let mut current = node();
        current.push(addr("ExternalIP", stale));

        super::reconcile(
            current,
            &staged(&["1.1.1.1"]),
            Removal::After(GRACE),
            well_answered,
            pending,
            now,
        )
    }

    #[test]
    fn grace_alone_does_not_reap_before_the_rounds_are_in() {
        let start = Instant::now();
        let late = start + GRACE + Duration::from_secs(1);

        // Time is up on the first sighting, but one round is not evidence.
        let first = round("9.9.9.9", true, &BTreeMap::new(), late);
        assert_eq!(first.report[&ip("9.9.9.9")], AddrStatus::Skipped);
        assert_eq!(first.pending[&ip("9.9.9.9")].misses, 1);
    }

    #[test]
    fn an_address_goes_once_the_grace_and_the_rounds_are_both_spent() {
        let start = Instant::now();
        let late = start + GRACE + Duration::from_secs(1);

        let first = round("9.9.9.9", true, &BTreeMap::new(), start);
        let second = round("9.9.9.9", true, &first.pending, late);

        assert!(second.has_changes);
        assert_eq!(second.report[&ip("9.9.9.9")], AddrStatus::Removed);
        assert!(!second.addresses.contains(&addr("ExternalIP", "9.9.9.9")));
    }

    #[test]
    fn rounds_alone_do_not_reap_before_the_grace_is_spent() {
        let now = Instant::now();

        let first = round("9.9.9.9", true, &BTreeMap::new(), now);
        let second = round("9.9.9.9", true, &first.pending, now);
        let third = round("9.9.9.9", true, &second.pending, now);

        assert_eq!(third.report[&ip("9.9.9.9")], AddrStatus::Skipped);
        assert_eq!(third.pending[&ip("9.9.9.9")].misses, 3);
    }

    #[test]
    fn a_degraded_round_neither_condemns_nor_forgives() {
        let start = Instant::now();
        let late = start + GRACE + Duration::from_secs(1);

        let first = round("9.9.9.9", true, &BTreeMap::new(), start);
        let quiet = round("9.9.9.9", false, &first.pending, late);

        assert_eq!(quiet.report[&ip("9.9.9.9")], AddrStatus::Skipped);
        assert_eq!(quiet.pending[&ip("9.9.9.9")], first.pending[&ip("9.9.9.9")]);
    }

    #[test]
    fn an_address_confirmed_again_is_off_the_hook() {
        let start = Instant::now();
        let first = round("9.9.9.9", true, &BTreeMap::new(), start);

        let mut current = node();
        current.push(addr("ExternalIP", "9.9.9.9"));

        let back = super::reconcile(
            current,
            &staged(&["9.9.9.9"]),
            Removal::After(GRACE),
            true,
            &first.pending,
            start + GRACE,
        );

        assert!(back.pending.is_empty());
        assert_eq!(back.report[&ip("9.9.9.9")], AddrStatus::Skipped);
    }

    #[test]
    fn never_removing_is_what_asking_for_nothing_gets() {
        let start = Instant::now();
        let mut current = node();
        current.push(addr("ExternalIP", "9.9.9.9"));

        let out = super::reconcile(
            current,
            &staged(&["1.1.1.1"]),
            Removal::Never,
            true,
            &BTreeMap::new(),
            start + GRACE * 100,
        );

        assert!(out.addresses.contains(&addr("ExternalIP", "9.9.9.9")));
        assert!(out.pending.is_empty());
    }

    #[test]
    fn a_staged_address_missing_from_the_node_is_added() {
        let out = reconcile(node(), &staged(&["1.1.1.1"]), false);

        assert!(out.has_changes);
        assert_eq!(out.report[&ip("1.1.1.1")], AddrStatus::New);
        assert!(out.addresses.contains(&addr("ExternalIP", "1.1.1.1")));
    }

    #[test]
    fn hostname_and_internal_ip_are_never_touched() {
        let out = reconcile(node(), &staged(&["1.1.1.1"]), true);

        assert!(out.addresses.contains(&addr("InternalIP", "192.168.1.1")));
        assert!(out.addresses.contains(&addr("Hostname", "some-node")));
    }

    #[test]
    fn an_address_already_there_produces_no_patch() {
        let mut current = node();
        current.push(addr("ExternalIP", "1.1.1.1"));

        let out = reconcile(current, &staged(&["1.1.1.1"]), true);

        assert!(!out.has_changes);
        assert_eq!(out.report[&ip("1.1.1.1")], AddrStatus::Skipped);
    }

    #[test]
    fn strict_mode_tears_off_an_address_consensus_did_not_vouch_for() {
        let mut current = node();
        current.push(addr("ExternalIP", "9.9.9.9"));

        let out = reconcile(current, &staged(&["1.1.1.1"]), true);

        assert!(out.has_changes);
        assert_eq!(out.report[&ip("9.9.9.9")], AddrStatus::Removed);
        assert_eq!(out.report[&ip("1.1.1.1")], AddrStatus::New);
        assert!(!out.addresses.contains(&addr("ExternalIP", "9.9.9.9")));
    }

    #[test]
    fn without_strict_mode_a_stranger_address_is_left_alone() {
        let mut current = node();
        current.push(addr("ExternalIP", "9.9.9.9"));

        let out = reconcile(current, &staged(&["1.1.1.1"]), false);

        assert_eq!(out.report[&ip("9.9.9.9")], AddrStatus::Skipped);
        assert!(out.addresses.contains(&addr("ExternalIP", "9.9.9.9")));
    }

    #[test]
    fn an_unparsable_external_ip_is_preserved_rather_than_dropped() {
        let mut current = node();
        current.push(addr("ExternalIP", "not-an-address"));

        let out = reconcile(current, &staged(&["1.1.1.1"]), true);

        assert!(
            out.addresses
                .contains(&addr("ExternalIP", "not-an-address"))
        );
    }

    #[test]
    fn several_staged_addresses_all_land() {
        let out = reconcile(node(), &staged(&["1.1.1.1", "2606:4700::1111"]), true);

        assert_eq!(out.report.len(), 2);
        assert!(out.report.values().all(AddrStatus::is_new));
    }
}
