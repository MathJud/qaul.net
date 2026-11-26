// Copyright (c) 2026 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # DNS-SD Browsing for qaul Nodes
//!
//! Finds the other qaul nodes registered in the SRP zone, see
//! [`registration`](super::registration) for the records, and connects the
//! LAN swarm to them.
//!
//! The [`Browser`] task periodically looks up
//!
//! ```text
//! _qaul._udp.<zone>             PTR  <peer-id>._qaul._udp.<zone>
//! <peer-id>._qaul._udp.<zone>   TXT  "dnsaddr=<multiaddr>/p2p/<peer-id>" ...
//! ```
//!
//! and publishes the dialable addresses per peer. The [`LanPeers`] dialer,
//! driven by the event loop, adds these peers to the floodsub view of the
//! LAN swarm and dials the ones not connected yet.
//!
//! Anyone holding a key can register in the zone, so the data is not
//! trusted: an instance is only accepted if its name is a peer ID, every
//! address must end with that same peer ID, and the noise handshake of the
//! connection verifies it.
//!
//! Queries go through the system's resolvers. Where the system
//! configuration cannot be read, as on Android, public resolvers are used.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use hickory_proto::rr::{Name, RData, RecordType};
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::{ResolveError, TokioResolver};
use libp2p::multiaddr::Protocol;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::Swarm;
use libp2p::{Multiaddr, PeerId};
use tokio::sync::watch;

use super::registration::{self, SERVICE_TYPE};
use crate::connections::lan::QaulLanBehaviour;

/// How often the zone is browsed. The registered records have a TTL of
/// five minutes, so changes show up within that time at the latest.
const BROWSE_INTERVAL: Duration = Duration::from_secs(60);
/// First delay before browsing again after a failure. It doubles after each
/// consecutive failure, up to `BROWSE_INTERVAL`.
const BACKOFF_MIN: Duration = Duration::from_secs(5);
/// Upper limit of instances taken from one browse, as anybody can register.
const MAX_PEERS: usize = 64;
/// Upper limit of addresses taken per peer.
const MAX_ADDRESSES: usize = 8;
/// Minimum gap between dial attempts to the same peer. Registered addresses
/// are often not reachable from this network, so this is longer than the
/// redial interval of peers found via mDNS.
const DIAL_INTERVAL_MS: u64 = 30_000;

/// Prefix of the TXT entries carrying a libp2p multiaddress.
const DNSADDR: &str = "dnsaddr=";

/// Dialable addresses of the qaul nodes found in the zone, per peer.
pub type Peers = BTreeMap<PeerId, Vec<Multiaddr>>;

/// Background task browsing the zone for qaul nodes.
pub struct Browser {
    pub zone: Name,
    /// Our own peer ID, which is never reported.
    pub peer_id: PeerId,
    pub peers: watch::Sender<Peers>,
}

impl Browser {
    /// Runs until the runtime shuts down.
    pub async fn run(self) {
        let service = match service_name(&self.zone) {
            Ok(service) => service,
            Err(e) => {
                log::error!("SRP: cannot browse {}: {}", self.zone, e);
                return;
            }
        };
        let resolver = resolver();
        let mut backoff = BACKOFF_MIN;

        loop {
            let previous = self.peers.borrow().clone();
            let delay = match browse(&resolver, &service, &self.peer_id, &previous).await {
                Ok(peers) => {
                    backoff = BACKOFF_MIN;
                    self.peers.send_if_modified(|current| {
                        if *current == peers {
                            return false;
                        }
                        log::info!(
                            "SRP: {} qaul node(s) registered in {}",
                            peers.len(),
                            self.zone
                        );
                        log::debug!("SRP: registered qaul nodes {:?}", peers);
                        *current = peers;
                        true
                    });
                    BROWSE_INTERVAL
                }
                Err(e) => {
                    // keep the last known peers, the network may be down
                    log::warn!("SRP: browsing {} failed: {}", service, e);
                    let delay = backoff;
                    backoff = (backoff * 2).min(BROWSE_INTERVAL);
                    delay
                }
            };
            tokio::time::sleep(delay).await;
        }
    }
}

/// `_qaul._udp.<zone>`
fn service_name(zone: &Name) -> Result<Name, String> {
    Name::from_ascii(SERVICE_TYPE)
        .and_then(|name| name.append_domain(zone))
        .map_err(|e| e.to_string())
}

/// A resolver with the system configuration, or with public resolvers if
/// it cannot be read.
fn resolver() -> TokioResolver {
    let builder = match TokioResolver::builder_tokio() {
        Ok(builder) => builder,
        Err(e) => {
            log::info!(
                "SRP: no system DNS configuration ({}), browsing via Quad9",
                e
            );
            TokioResolver::builder_with_config(
                ResolverConfig::quad9(),
                TokioConnectionProvider::default(),
            )
        }
    };
    builder.build()
}

/// Looks up the qaul instances of `service` and their addresses.
///
/// When the addresses of a peer cannot be looked up, its addresses from
/// `previous` are kept.
async fn browse(
    resolver: &TokioResolver,
    service: &Name,
    own: &PeerId,
    previous: &Peers,
) -> Result<Peers, ResolveError> {
    let instances = match resolver.lookup(service.clone(), RecordType::PTR).await {
        Ok(lookup) => lookup
            .iter()
            .filter_map(|rdata| match rdata {
                RData::PTR(ptr) => Some(ptr.0.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        Err(e) if e.is_no_records_found() => Vec::new(),
        Err(e) => return Err(e),
    };

    let mut candidates: Vec<(PeerId, Name)> = instances
        .into_iter()
        .filter_map(|name| Some((instance_peer(&name, service)?, name)))
        .filter(|(peer, _)| peer != own)
        .collect();
    candidates.sort();
    candidates.dedup_by(|a, b| a.0 == b.0);
    if candidates.len() > MAX_PEERS {
        log::warn!(
            "SRP: {} instances in {}, using the first {}",
            candidates.len(),
            service,
            MAX_PEERS
        );
        candidates.truncate(MAX_PEERS);
    }

    let lookups = candidates.into_iter().map(|(peer, name)| async move {
        let addresses = match resolver.lookup(name.clone(), RecordType::TXT).await {
            Ok(lookup) => Some(addresses(
                &peer,
                lookup.iter().flat_map(|rdata| match rdata {
                    RData::TXT(txt) => txt.txt_data().to_vec(),
                    _ => Vec::new(),
                }),
            )),
            Err(e) if e.is_no_records_found() => Some(Vec::new()),
            Err(e) => {
                log::debug!("SRP: TXT lookup of {} failed: {}", name, e);
                None
            }
        };
        (peer, addresses)
    });

    let mut peers = Peers::new();
    for (peer, addresses) in futures::future::join_all(lookups).await {
        let addresses = match addresses {
            Some(addresses) => addresses,
            None => previous.get(&peer).cloned().unwrap_or_default(),
        };
        if !addresses.is_empty() {
            peers.insert(peer, addresses);
        }
    }
    Ok(peers)
}

/// The peer ID of the service instance `name`, if `name` is an instance of
/// `service` labelled with a peer ID.
fn instance_peer(name: &Name, service: &Name) -> Option<PeerId> {
    if name.num_labels() != service.num_labels() + 1 || !service.zone_of(name) {
        return None;
    }
    let label = name.iter().next()?;
    std::str::from_utf8(label).ok()?.parse().ok()
}

/// The dialable addresses of `peer` among the TXT entries.
///
/// Only `dnsaddr=` entries with an IP address usable in DNS, followed by TCP
/// or QUIC, and ending with `/p2p/<peer>` are taken. The `/p2p` component is
/// removed.
fn addresses<T: AsRef<[u8]>>(
    peer: &PeerId,
    entries: impl IntoIterator<Item = T>,
) -> Vec<Multiaddr> {
    let mut addresses: Vec<Multiaddr> = entries
        .into_iter()
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry.as_ref()).ok()?;
            let addr: Multiaddr = entry.strip_prefix(DNSADDR)?.parse().ok()?;
            dialable(peer, addr)
        })
        .collect();
    addresses.sort();
    addresses.dedup();
    addresses.truncate(MAX_ADDRESSES);
    addresses
}

/// `addr` without its `/p2p` component, if it is a dialable address of
/// `peer`: `/ip4|ip6/<ip>/tcp/<port>/p2p/<peer>` or
/// `/ip4|ip6/<ip>/udp/<port>/quic-v1/p2p/<peer>`.
fn dialable(peer: &PeerId, mut addr: Multiaddr) -> Option<Multiaddr> {
    match addr.pop()? {
        Protocol::P2p(id) if id == *peer => {}
        _ => return None,
    }
    let parts: Vec<Protocol> = addr.iter().collect();
    let ip = match parts.first()? {
        Protocol::Ip4(ip) => std::net::IpAddr::V4(*ip),
        Protocol::Ip6(ip) => std::net::IpAddr::V6(*ip),
        _ => return None,
    };
    if !registration::is_usable(ip) {
        return None;
    }
    match parts[1..] {
        [Protocol::Tcp(port)] | [Protocol::Udp(port), Protocol::QuicV1] if port != 0 => Some(addr),
        _ => None,
    }
}

/// Connects the LAN swarm to the qaul nodes found by the [`Browser`].
pub struct LanPeers {
    peers: watch::Receiver<Peers>,
    /// The peers added to the floodsub view.
    known: Peers,
    /// When each peer may be dialed again.
    dial_at: HashMap<PeerId, u64>,
}

impl LanPeers {
    pub fn new(peers: watch::Receiver<Peers>) -> Self {
        Self {
            peers,
            known: Peers::new(),
            dial_at: HashMap::new(),
        }
    }

    /// Takes over changes of the found peers and dials the ones that are
    /// not connected. `now` is the current time in milliseconds.
    pub fn connect(&mut self, swarm: &mut Swarm<QaulLanBehaviour>, now: u64) {
        if self.peers.has_changed().unwrap_or(false) {
            let peers = self.peers.borrow_and_update().clone();
            self.update(swarm, peers);
        }

        for (peer, addresses) in &self.known {
            if swarm.is_connected(peer) {
                continue;
            }
            if self.dial_at.get(peer).is_some_and(|due| now < *due) {
                continue;
            }
            self.dial_at
                .insert(*peer, now.saturating_add(DIAL_INTERVAL_MS));
            log::debug!(
                "SRP: dialing registered qaul node {} at {:?}",
                peer,
                addresses
            );
            let opts = DialOpts::peer_id(*peer)
                .condition(PeerCondition::DisconnectedAndNotDialing)
                .addresses(addresses.clone())
                .build();
            if let Err(e) = swarm.dial(opts) {
                log::trace!("SRP: dialing {} failed: {}", peer, e);
            }
        }
    }

    fn update(&mut self, swarm: &mut Swarm<QaulLanBehaviour>, peers: Peers) {
        let behaviour = swarm.behaviour_mut();
        for peer in self.known.keys().filter(|p| !peers.contains_key(p)) {
            self.dial_at.remove(peer);
            // keep peers that mDNS still sees
            if !behaviour.mdns.discovered_nodes().any(|p| p == peer) {
                log::trace!("SRP: peer {:?} from floodsub removed", peer);
                behaviour.floodsub.remove_node_from_partial_view(peer);
            }
        }
        for (peer, addresses) in &peers {
            if self.known.get(peer) != Some(addresses) {
                // dial changed addresses right away
                self.dial_at.remove(peer);
            }
            if !self.known.contains_key(peer) {
                log::trace!("SRP: peer {:?} to floodsub added", peer);
                behaviour.floodsub.add_node_to_partial_view(*peer);
            }
        }
        self.known = peers;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone() -> Name {
        srp_client::model::parse_zone("srp.dev.zenr.io").unwrap()
    }

    #[test]
    fn instance_names() {
        let service = service_name(&zone()).unwrap();
        assert_eq!(service.to_ascii(), "_qaul._udp.srp.dev.zenr.io.");

        let peer = PeerId::random();
        let name = Name::from_ascii(format!("{peer}._qaul._udp.srp.dev.zenr.io.")).unwrap();
        assert_eq!(instance_peer(&name, &service), Some(peer));

        // not a peer ID
        let name = Name::from_ascii("printer._qaul._udp.srp.dev.zenr.io.").unwrap();
        assert_eq!(instance_peer(&name, &service), None);
        // other service
        let name = Name::from_ascii(format!("{peer}._http._tcp.srp.dev.zenr.io.")).unwrap();
        assert_eq!(instance_peer(&name, &service), None);
        // not directly below the service
        let name = Name::from_ascii(format!("{peer}.x._qaul._udp.srp.dev.zenr.io.")).unwrap();
        assert_eq!(instance_peer(&name, &service), None);
    }

    #[test]
    fn txt_addresses() {
        let peer = PeerId::random();
        let other = PeerId::random();
        let entries = [
            format!("dnsaddr=/ip4/192.168.1.10/udp/9001/quic-v1/p2p/{peer}"),
            format!("dnsaddr=/ip4/192.168.1.10/tcp/9000/p2p/{peer}"),
            format!("dnsaddr=/ip6/2001:db8::1/tcp/9003/p2p/{peer}"),
            format!("dnsaddr=/ip4/192.168.1.10/tcp/9000/p2p/{peer}"),
            // other peer
            format!("dnsaddr=/ip4/192.168.1.11/tcp/9000/p2p/{other}"),
            // no peer ID
            "dnsaddr=/ip4/192.168.1.10/tcp/9000".to_string(),
            // not usable
            format!("dnsaddr=/ip4/127.0.0.1/tcp/9000/p2p/{peer}"),
            format!("dnsaddr=/ip6/fe80::1/tcp/9000/p2p/{peer}"),
            format!("dnsaddr=/ip4/192.168.1.10/tcp/0/p2p/{peer}"),
            // not TCP or QUIC
            format!("dnsaddr=/ip4/192.168.1.10/udp/9001/p2p/{peer}"),
            format!("dnsaddr=/dns4/example.org/tcp/9000/p2p/{peer}"),
            // no dnsaddr, garbage
            format!("/ip4/192.168.1.12/tcp/9000/p2p/{peer}"),
            "dnsaddr=garbage".to_string(),
        ];
        let found: Vec<String> = addresses(&peer, entries.iter().map(|e| e.as_bytes()))
            .iter()
            .map(|a| a.to_string())
            .collect();
        let mut expected = vec![
            "/ip4/192.168.1.10/tcp/9000".to_string(),
            "/ip4/192.168.1.10/udp/9001/quic-v1".to_string(),
            "/ip6/2001:db8::1/tcp/9003".to_string(),
        ];
        expected.sort_by_key(|a| a.parse::<Multiaddr>().unwrap());
        assert_eq!(found, expected);
    }

    #[test]
    fn registered_addresses_are_found() {
        let peer = PeerId::random();
        let listeners: Vec<Multiaddr> = [
            "/ip4/192.168.1.10/udp/9001/quic-v1",
            "/ip4/192.168.1.10/tcp/9000",
            "/ip6/2001:db8::1/tcp/9003",
        ]
        .iter()
        .map(|a| a.parse().unwrap())
        .collect();
        let reg = registration::build(&zone(), &peer, &listeners, |_| true).unwrap();
        let service = &reg.services[0];

        let instance = Name::from_ascii(format!("{}.{}", service.instance, service.service_type))
            .unwrap()
            .append_domain(&zone())
            .unwrap();
        assert_eq!(
            instance_peer(&instance, &service_name(&zone()).unwrap()),
            Some(peer)
        );

        let mut found = addresses(&peer, &service.txt);
        let mut expected = listeners;
        found.sort();
        expected.sort();
        assert_eq!(found, expected);
    }
}
