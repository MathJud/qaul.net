// Copyright (c) 2026 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # SRP Registration of the LAN Swarm
//!
//! Builds the DNS-SD registration of this node from the listen addresses of
//! the LAN swarm:
//!
//! ```text
//! _qaul._udp.<domain>             PTR  <peer-id>._qaul._udp.<domain>
//! <peer-id>._qaul._udp.<domain>   SRV  0 0 <port> <host>.<domain>
//! <peer-id>._qaul._udp.<domain>   TXT  "dnsaddr=/ip4/<ip4-address>/tcp/<port>/p2p/<peer-id>"
//!                                      "dnsaddr=/ip4/<ip4-address>/udp/<port>/quic-v1/p2p/<peer-id>"
//!                                      "dnsaddr=/ip6/<ip6-address>/tcp/<port>/p2p/<peer-id>"
//!                                      "dnsaddr=/ip6/<ip6-address>/udp/<port>/quic-v1/p2p/<peer-id>"
//! <host>.<domain>                 A    <ip4-address>
//! <host>.<domain>                 AAAA <ip6-address>
//! <host>.<domain>                 KEY  <SRP public key>
//! ```
//!
//! The host label is the peer ID in lower case, since registrars refuse
//! host names with upper-case letters. The instance name and the `dnsaddr`
//! entries keep the exact, case-sensitive peer ID. The KEY record is added by the
//! `srp-client` crate. The `dnsaddr` entries are the listen addresses as the
//! swarm reports them, so UDP addresses keep their `/quic-v1` component and
//! stay dialable. SRP allows a single TXT record per service instance, so the
//! entries are the strings of that one record.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::time::Duration;

use hickory_proto::rr::Name;
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use srp_client::{Host, Registration, ServiceInstance};

/// DNS-SD service type of qaul.
pub const SERVICE_TYPE: &str = "_qaul._udp";

/// Lease of the registered records. The TTL of the records is the same.
/// The KEY lease, which keeps the claim on the name, stays at the
/// `srp-client` default.
pub const LEASE: Duration = Duration::from_secs(5 * 60);

/// Prefix of the TXT entries carrying a libp2p multiaddress.
const DNSADDR: &str = "dnsaddr=";

/// One usable listen address of the LAN swarm.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Listener {
    /// IPv4 sorts before IPv6, TCP before UDP, as in the registry scheme.
    is_ipv6: bool,
    ip: IpAddr,
    is_udp: bool,
    port: u16,
    /// The full address, without a `/p2p` component.
    addr: Multiaddr,
}

impl Listener {
    fn parse(addr: &Multiaddr) -> Option<Self> {
        let mut iter = addr.iter();
        let ip = match iter.next()? {
            Protocol::Ip4(ip) => IpAddr::V4(ip),
            Protocol::Ip6(ip) => IpAddr::V6(ip),
            _ => return None,
        };
        let (is_udp, port) = match iter.next()? {
            Protocol::Tcp(port) => (false, port),
            Protocol::Udp(port) => (true, port),
            _ => return None,
        };
        if port == 0 {
            return None;
        }
        let addr = addr
            .iter()
            .filter(|p| !matches!(p, Protocol::P2p(_)))
            .collect();
        Some(Self {
            is_ipv6: ip.is_ipv6(),
            ip,
            is_udp,
            port,
            addr,
        })
    }
}

/// Builds the registration of the LAN swarm in `zone`.
///
/// Only listen addresses whose IP is usable in DNS and accepted by
/// `allowed` are registered. Returns `None` when no address is left.
pub fn build(
    zone: &Name,
    peer_id: &PeerId,
    listeners: &[Multiaddr],
    allowed: impl Fn(IpAddr) -> bool,
) -> Option<Registration> {
    let listeners: BTreeSet<Listener> = listeners
        .iter()
        .filter_map(Listener::parse)
        .filter(|l| is_usable(l.ip) && allowed(l.ip))
        .collect();

    // The service is `_udp`: advertise the QUIC port, IPv4 first. Fall back
    // to TCP if the swarm only listens on TCP.
    let port = listeners
        .iter()
        .find(|l| l.is_udp)
        .or_else(|| listeners.iter().next())?
        .port;

    let mut addresses: Vec<IpAddr> = Vec::new();
    for listener in &listeners {
        if !addresses.contains(&listener.ip) {
            addresses.push(listener.ip);
        }
    }

    let peer = peer_id.to_base58();
    let txt = listeners
        .iter()
        .map(|l| {
            let addr = l.addr.clone().with(Protocol::P2p(*peer_id));
            format!("{DNSADDR}{addr}").into_bytes()
        })
        .collect();

    let mut service = ServiceInstance::new(peer.clone(), SERVICE_TYPE, port);
    service.txt = txt;

    let mut registration = Registration::new(Host {
        name: peer.to_ascii_lowercase(),
        addresses,
    });
    registration.zone = zone.clone();
    registration.lease = LEASE;
    registration.ttl = LEASE;
    registration.services = vec![service];
    Some(registration)
}

/// Whether `ip` makes sense in a DNS record: not loopback, unspecified,
/// multicast or link-local, since link-local addresses need a scope.
pub fn is_usable(ip: IpAddr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    match ip {
        IpAddr::V4(ip) => !ip.is_link_local() && !ip.is_broadcast(),
        IpAddr::V6(ip) => !ip.is_unicast_link_local(),
    }
}

/// The local IPs that may be registered.
///
/// On Linux these are the usable IPs of the first active network interface:
/// the operationally up, non-loopback interface with the lowest index that
/// has a usable IP. `None` means no restriction, which is the case on other
/// platforms and when the interfaces cannot be read.
pub fn allowed_ips() -> Option<BTreeSet<IpAddr>> {
    #[cfg(target_os = "linux")]
    {
        match if_addrs::get_if_addrs() {
            Ok(interfaces) => {
                let candidates = interfaces
                    .into_iter()
                    .filter(|i| !i.is_loopback() && i.is_oper_up() && is_usable(i.ip()))
                    .map(|i| {
                        let ip = i.ip();
                        (i.index.unwrap_or(u32::MAX), i.name, ip)
                    });
                return Some(first_interface_ips(candidates));
            }
            Err(e) => {
                log::warn!("SRP: cannot read network interfaces: {}", e);
            }
        }
    }
    None
}

/// The IPs of the interface with the lowest index among `(index, name, ip)`
/// entries. Empty if there are none.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn first_interface_ips(
    candidates: impl Iterator<Item = (u32, String, IpAddr)>,
) -> BTreeSet<IpAddr> {
    let candidates: Vec<_> = candidates.collect();
    let Some((_, first, _)) = candidates.iter().min_by_key(|(index, _, _)| *index) else {
        return BTreeSet::new();
    };
    candidates
        .iter()
        .filter(|(_, name, _)| name == first)
        .map(|(_, _, ip)| *ip)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone() -> Name {
        srp_client::model::parse_zone("dev.zenr.io").unwrap()
    }

    fn addrs(list: &[&str]) -> Vec<Multiaddr> {
        list.iter().map(|a| a.parse().unwrap()).collect()
    }

    fn txt(reg: &Registration) -> Vec<String> {
        reg.services[0]
            .txt
            .iter()
            .map(|t| String::from_utf8(t.clone()).unwrap())
            .collect()
    }

    #[test]
    fn follows_registry_scheme() {
        let peer = PeerId::random();
        let listeners = addrs(&[
            "/ip6/2001:db8::1/udp/9002/quic-v1",
            "/ip4/127.0.0.1/udp/9001/quic-v1",
            "/ip4/192.168.1.10/udp/9001/quic-v1",
            "/ip6/fe80::1/tcp/9003",
            "/ip4/192.168.1.10/tcp/9000",
            "/ip6/2001:db8::1/tcp/9003",
        ]);
        let reg = build(&zone(), &peer, &listeners, |_| true).unwrap();

        let host = peer.to_base58().to_ascii_lowercase();
        assert_eq!(reg.host.name, host);
        assert_eq!(
            reg.host_name().unwrap().to_ascii(),
            format!("{host}.dev.zenr.io.")
        );
        assert_eq!(
            reg.host.addresses,
            vec![
                "192.168.1.10".parse::<IpAddr>().unwrap(),
                "2001:db8::1".parse().unwrap()
            ]
        );

        let service = &reg.services[0];
        assert_eq!(service.instance, peer.to_base58());
        assert_eq!(service.service_type, "_qaul._udp");
        assert_eq!(service.port, 9001);
        assert_eq!(reg.lease, Duration::from_secs(300));
        assert_eq!(reg.ttl, Duration::from_secs(300));
        assert_eq!(
            txt(&reg),
            vec![
                format!("dnsaddr=/ip4/192.168.1.10/tcp/9000/p2p/{peer}"),
                format!("dnsaddr=/ip4/192.168.1.10/udp/9001/quic-v1/p2p/{peer}"),
                format!("dnsaddr=/ip6/2001:db8::1/tcp/9003/p2p/{peer}"),
                format!("dnsaddr=/ip6/2001:db8::1/udp/9002/quic-v1/p2p/{peer}"),
            ]
        );
    }

    #[test]
    fn order_of_listeners_does_not_matter() {
        let peer = PeerId::random();
        let mut listeners = addrs(&[
            "/ip4/10.0.0.2/udp/9001/quic-v1",
            "/ip4/10.0.0.2/tcp/9000",
            "/ip6/2001:db8::2/tcp/9003",
        ]);
        let a = build(&zone(), &peer, &listeners, |_| true);
        listeners.reverse();
        assert_eq!(a, build(&zone(), &peer, &listeners, |_| true));
    }

    #[test]
    fn only_allowed_ips() {
        let peer = PeerId::random();
        let listeners = addrs(&["/ip4/10.0.0.2/tcp/9000", "/ip4/172.17.0.1/tcp/9000"]);
        let reg = build(&zone(), &peer, &listeners, |ip| {
            ip.to_string() == "10.0.0.2"
        })
        .unwrap();
        assert_eq!(
            reg.host.addresses,
            vec!["10.0.0.2".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(reg.services[0].port, 9000);
    }

    #[test]
    fn nothing_to_register() {
        let peer = PeerId::random();
        let listeners = addrs(&["/ip4/127.0.0.1/tcp/9000", "/ip4/0.0.0.0/tcp/0"]);
        assert!(build(&zone(), &peer, &listeners, |_| true).is_none());
        assert!(build(&zone(), &peer, &[], |_| true).is_none());
    }

    #[test]
    fn picks_lowest_interface_index() {
        let ips = first_interface_ips(
            vec![
                (5, "docker0".to_string(), "172.17.0.1".parse().unwrap()),
                (2, "eth0".to_string(), "10.0.0.2".parse().unwrap()),
                (2, "eth0".to_string(), "2001:db8::2".parse().unwrap()),
            ]
            .into_iter(),
        );
        assert_eq!(
            ips,
            ["10.0.0.2", "2001:db8::2"]
                .iter()
                .map(|ip| ip.parse::<IpAddr>().unwrap())
                .collect()
        );
    }
}
