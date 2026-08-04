use {
    k8s_openapi::api::core::v1::NodeAddress,
    std::{
        collections::{BTreeMap, BTreeSet},
        net::IpAddr,
        str::FromStr,
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
}

/// Decides the node's new address list from its current one and the addresses
/// consensus vouched for.
///
/// Pure on purpose. Every branch that decides whether an address is added,
/// kept or torn off a live node is decided here, where a test can reach it.
pub fn reconcile(
    current: Vec<NodeAddress>,
    staged: &BTreeSet<IpAddr>,
    remove_unstaged: bool,
) -> Outcome {
    let mut report = BTreeMap::new();
    let mut addresses = Vec::with_capacity(current.len() + staged.len());
    let mut has_changes = false;

    // The node keeps its Hostname and InternalIP untouched; only the
    // ExternalIPs are ours to decide upon. An unparsable one is somebody
    // else's business, so it is preserved and complained about.

    for address in current {
        let Some(external_ip) = parse_external_ip(&address) else {
            addresses.push(address);
            continue;
        };

        let status = if remove_unstaged && !staged.contains(&external_ip) {
            has_changes = true;
            AddrStatus::Removed
        } else {
            addresses.push(address);
            AddrStatus::Skipped
        };

        report.insert(external_ip, status);
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
