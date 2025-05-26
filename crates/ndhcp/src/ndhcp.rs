use anyhow::{Context, bail};
use pnet::{self, datalink::NetworkInterface, ipnetwork::IpNetwork};
use std::fmt::Display;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
// use derive_more::Display;

mod is_reserved;
// mod verifier;

pub enum FilteredReason {
    IsDown,
    IsNotGlobal,
    IsLoopback,
}

pub struct Filtered<T>(T, FilteredReason);

impl<T> Filtered<T> {
    fn new(v: T, reason: FilteredReason) -> Self {
        Self(v, reason)
    }
}

type NI = NetworkInterface;

// struct PartitionTask {
//     input: Vec<NetworkInterface>,
//     reasons: Vec<FilteredReason>,
// }

// impl PartitionTask {
//     fn try_filter_in(&mut self, item: &NetworkInterface) -> bool {
//         if !item.is_up() || !item.is_running() {
//             self.reasons.push(FilteredReason::IsDown);
//             return false;
//         }
//         true
//     }

//     fn partition(mut self) -> (Vec<NetworkInterface>, Vec<Filtered<NetworkInterface>>) {
//         let input = self.input;
//         let &mut self_ref = &mut self;

//         let (ok, filtered): (Vec<_>, Vec<_>) = input.
//             into_iter().
//             partition(|e| (&self).try_filter_in(e));

//         let filtered_with_reasons = filtered.
//             into_iter().
//             zip(self.reasons).
//             map(|(iface, reason)| Filtered::new(iface, reason)).
//             collect();

//         (ok, filtered_with_reasons)
//     }

//     fn new(input: Vec<NetworkInterface>) -> Self {
//         let n = input.len();
//         Self { input, reasons: Vec::with_capacity(n) }
//     }
// }

fn iface_filter(ifaces: Vec<NI>) -> (Vec<NI>, Vec<Filtered<NI>>) {
    let mut reasons: Vec<FilteredReason> = Vec::with_capacity(ifaces.len());

    let (ok, filtered): (Vec<NI>, Vec<NI>) = ifaces.into_iter().partition(|iface| {
        let len_before = reasons.len();

        if !iface.is_up() || !iface.is_running() {
            reasons.push(FilteredReason::IsDown);
        } else if iface.is_loopback() {
            reasons.push(FilteredReason::IsLoopback);
        }

        // Size has not been changed => No reason was inserted =>
        // This NetworkInterface is good and should be added to the left.
        len_before == reasons.len()
    });

    let filtered_with_reasons = filtered
        .into_iter()
        .zip(reasons)
        .map(|(iface, reason)| Filtered::new(iface, reason))
        .collect();

    (ok, filtered_with_reasons)
}

pub struct IpReport<'a> {
    iface: &'a NetworkInterface,
    ip: IpAddr,
    check: Result<(), IpReportError>,
}

pub enum IpReportError {
    IfaceIsDown,
    IfaceIsNotRunning,
    IfaceIsLoopback,
    AddrIsReserved,
}

impl<'a> IpReport<'a> {
    fn good(iface: &'a NetworkInterface, ip: IpAddr) -> Self {
        Self {
            iface,
            ip,
            check: Ok(()),
        }
    }
    fn bad(iface: &'a NetworkInterface, ip: IpAddr, reason: IpReportError) -> Self {
        Self {
            iface,
            ip,
            check: Err(reason),
        }
    }
}

impl Display for IpReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::IfaceIsDown => "iface is down",
                Self::IfaceIsNotRunning => "iface is not running",
                Self::IfaceIsLoopback => "iface is loopback",
                Self::AddrIsReserved => "addr is reserved",
            }
        )
    }
}

impl<'a> Display for IpReport<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{} -> {}",
            &self.iface.name,
            self.ip,
            match self.check {
                Ok(()) => format!("OK!"),
                Err(ref reason) => format!("Declined, {reason}"),
            }
        )
    }
}

pub fn iface_report(ifaces: &Vec<NetworkInterface>) -> Vec<IpReport<'_>> {
    let mut out = Vec::new();

    for iface in ifaces {
        for addr_cidr in &iface.ips {
            let mut check: Result<(), IpReportError> = Ok(());

            if !iface.is_up() {
                check = Err(IpReportError::IfaceIsDown);
            } else if !iface.is_running() {
                check = Err(IpReportError::IfaceIsNotRunning)
            } else if iface.is_loopback() {
                check = Err(IpReportError::IfaceIsLoopback)
            } else if is_reserved::check(addr_cidr.ip()) {
                check = Err(IpReportError::AddrIsReserved)
            }
            
            out.push(match check {
                Ok(()) => IpReport::good(iface, addr_cidr.ip()),
                Err(reason) => IpReport::bad(iface, addr_cidr.ip(), reason),
            });
        }
    }

    out
}

impl From<Filtered<i32>> for i32 {
    fn from(value: Filtered<i32>) -> Self {
        value.0
    }
}
impl From<Filtered<Ipv4Addr>> for FilteredReason {
    fn from(value: Filtered<Ipv4Addr>) -> Self {
        value.1
    }
}

// impl From<Filtered<Ipv6Addr>> for Ipv6Addr {
//     fn from(value: Filtered<Ipv6Addr>) -> Self { value.0 }
// }
impl From<Filtered<Ipv6Addr>> for FilteredReason {
    fn from(value: Filtered<Ipv6Addr>) -> Self {
        value.1
    }
}

// pub fn get_assigned_ipv4() -> (Vec<Ipv4Addr>, Vec<Filtered<Ipv4Addr>>) {
//     filter_ips(get_assigned_ip(), IpAddr::to_ipv4)
// }

// pub fn get_assigned_ipv6() -> (Vec<Ipv6Addr>, Vec<Filtered<Ipv6Addr>>) {
//     filter_ips(get_assigned_ip(), IpAddr::to_ipv6)
// }

// fn get_assigned_ip() -> (Vec<IpAddr>, Vec<Filtered<IpAddr>>) {
//     use pnet::datalink::{NetworkInterface, interfaces};

//     let filtered_reasons: Vec<FilteredReason> = Vec::new();

//     fn is_suitable_iface(i: &NetworkInterface) -> bool {
//         i.is_up() && i.is_running() &&
//         (!i.is_broadcast() && !i.is)
//     }

//     let (ok_ifaces, filtered_ifaces): (Vec<NetworkInterface>, Vec<NetworkInterface>) = interfaces().into_iter().partition(|e| {
//         if !e.is_up() || !e.is_running() {
//             filtered_reasons.push(FilteredReason::IsDown);
//             return false;
//         }

//         true
//     });

//     let filtered_ifaces_with_reasons = filtered_ifaces
//         .into_iter()
//         .zip(filtered_reasons)
//         .map(|(iface, reason)| )
// }

pub fn ip_to_v4(ip: IpAddr) -> Option<Ipv4Addr> {
    match ip {
        IpAddr::V4(ip) => Some(ip),
        _ => None,
    }
}

pub fn ip_to_v6(ip: IpAddr) -> Option<Ipv6Addr> {
    match ip {
        IpAddr::V6(ip) => Some(ip),
        _ => None,
    }
}

pub trait ToIpv4: Sized {
    fn to_ipv4(self) -> Option<Ipv4Addr>;
}
impl ToIpv4 for IpAddr {
    fn to_ipv4(self) -> Option<Ipv4Addr> {
        ip_to_v4(self)
    }
}

pub trait ToIpv6: Sized {
    fn to_ipv6(self) -> Option<Ipv6Addr>;
}
impl ToIpv6 for IpAddr {
    fn to_ipv6(self) -> Option<Ipv6Addr> {
        ip_to_v6(self)
    }
}

fn filter_ips<F, V>(input: (Vec<IpAddr>, Vec<Filtered<IpAddr>>), f: F) -> (Vec<V>, Vec<Filtered<V>>)
where
    F: Fn(IpAddr) -> Option<V>,
{
    (
        input.0.into_iter().filter_map(&f).collect(),
        input
            .1
            .into_iter()
            .filter_map(|e| f(e.0).and_then(|ip| Filtered(ip, e.1).into()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_reports() {

    }
}
