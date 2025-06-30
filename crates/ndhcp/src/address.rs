use {
    std::{
        net::{IpAddr, Ipv4Addr, Ipv6Addr},
        str::FromStr,
        sync::LazyLock,
    },

    derive_more::{Display, Debug},
    pnet::ipnetwork::{Ipv4Network, Ipv6Network},
    strum_macros::{EnumIs, EnumIter, EnumCount},
};

#[derive(Clone, Copy, PartialEq)]
#[derive(Display, Debug)]
#[derive(EnumIs, EnumIter, EnumCount)]
pub enum Kind {
    Loopback,
    Private,
    Public,
    Multicast,
    Reserved,
}

/// https://en.wikipedia.org/wiki/Reserved_IP_addresses
pub fn kind_ipv4(addr: Ipv4Addr) -> Kind {
    static RESERVED_RAW: [(&str, Kind); 17] = [
        ("0.0.0.0/8", Kind::Loopback),
        ("10.0.0.0/8", Kind::Private),
        ("100.64.0.0/10", Kind::Reserved),
        ("127.0.0.0/8", Kind::Loopback),
        ("169.254.0.0/16", Kind::Private),
        ("172.16.0.0/12", Kind::Private),
        ("192.0.0.0/24", Kind::Reserved),
        ("192.0.2.0/24", Kind::Reserved),
        ("192.88.99.0/24", Kind::Reserved),
        ("192.168.0.0/16", Kind::Private),
        ("198.18.0.0/15", Kind::Reserved),
        ("198.51.100.0/24", Kind::Reserved),
        ("203.0.113.0/24", Kind::Reserved),
        ("224.0.0.0/4", Kind::Multicast),
        ("233.252.0.0/24", Kind::Reserved),
        ("240.0.0.0/4", Kind::Reserved),
        ("255.255.255.255/32", Kind::Multicast),
    ];

    static RESERVED: LazyLock<Vec<(Ipv4Network, Kind)>> = LazyLock::new(|| {
        RESERVED_RAW
            .iter()
            .map(|x| (Ipv4Network::from_str(x.0).unwrap(), x.1))
            .collect()
    });

    for (reserved_addr, kind) in RESERVED.iter() {
        if reserved_addr.contains(addr) {
            return *kind;
        }
    }

    Kind::Public
}

/// https://en.wikipedia.org/wiki/Reserved_IP_addresses
pub fn kind_ipv6(addr: Ipv6Addr) -> Kind {
    static RESERVED_RAW: [(&str, Kind); 16] = [
        ("::/128", Kind::Loopback),
        ("::1/128", Kind::Loopback),
        ("::ffff:0:0/96", Kind::Reserved),
        ("::ffff:0:0:0/96", Kind::Reserved),
        ("64:ff9b::/96", Kind::Reserved),
        ("64:ff9b:1::/4", Kind::Reserved),
        ("100::/64", Kind::Reserved),
        ("2001::/32", Kind::Reserved),
        ("2001:20::/28", Kind::Reserved),
        ("2001:db8::/32", Kind::Reserved),
        ("2002::/16", Kind::Reserved),
        ("3fff::/20", Kind::Reserved),
        ("5f00::/16", Kind::Reserved),
        ("fc00::/7", Kind::Private),
        ("fe80::/10", Kind::Private),
        ("ff00::/8", Kind::Multicast),
    ];

    static RESERVED: LazyLock<Vec<(Ipv6Network, Kind)>> = LazyLock::new(|| {
        RESERVED_RAW
            .iter()
            .map(|x| (Ipv6Network::from_str(x.0).unwrap(), x.1))
            .collect()
    });

    for (reserved_addr, kind) in RESERVED.iter() {
        if reserved_addr.contains(addr) {
            return *kind;
        }
    }

    Kind::Public
}

/// https://en.wikipedia.org/wiki/Reserved_IP_addresses
pub fn kind(addr: IpAddr) -> Kind {
    match addr {
        IpAddr::V4(addr_v4) => kind_ipv4(addr_v4),
        IpAddr::V6(addr_v6) => kind_ipv6(addr_v6),
    }
}

pub fn is_public_ipv4(addr: Ipv4Addr) -> bool {
    kind_ipv4(addr) == Kind::Public
}

pub fn is_public_ipv6(addr: Ipv6Addr) -> bool {
    kind_ipv6(addr) == Kind::Public
}

pub fn is_public(addr: IpAddr) -> bool {
    kind(addr) == Kind::Public
}
