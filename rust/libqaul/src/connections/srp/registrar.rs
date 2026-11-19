// Copyright (c) 2026 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # SRP Registrar Task
//!
//! Background task that keeps the LAN swarm registered at the SRP registrar.
//!
//! Only the configured SRP server is used. Its host name is resolved by the
//! operating system; there is no DNS-SD discovery of the registrar and no
//! reading of `/etc/resolv.conf`. The task registers as soon as the LAN
//! swarm has a usable listen address, refreshes on the granted lease, and
//! sends a new update whenever the listen addresses or the selected network
//! interface change. When no usable address is left, the registration is
//! removed while the key keeps its claim on the name.

use std::time::Duration;

use hickory_proto::rr::Name;
use libp2p::{Multiaddr, PeerId};
use srp_client::{
    Endpoint, Event, Registration, Session, SessionConfig, SrpClient, SrpError, SrpKey,
};
use tokio::sync::watch;

use super::registration;

/// How often the network interfaces are checked for changes.
const INTERFACE_POLL: Duration = Duration::from_secs(30);
/// First delay before retrying after a failure. It doubles after each
/// consecutive failure, up to `BACKOFF_MAX`.
const BACKOFF_MIN: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(5 * 60);

/// Port of the SRP server when the configuration names none.
pub const DEFAULT_PORT: u16 = 53;

/// What the task registers, and with which key.
pub struct Registrar {
    /// Host name or IP address of the SRP server.
    pub host: String,
    pub port: u16,
    pub zone: Name,
    pub peer_id: PeerId,
    /// SRP host key, PKCS#8 DER.
    pub key: Vec<u8>,
    /// Listen addresses of the LAN swarm.
    pub listeners: watch::Receiver<Vec<Multiaddr>>,
}

impl Registrar {
    /// Runs until the sender of the listen addresses is dropped.
    pub async fn run(mut self) {
        let mut session: Option<Session> = None;
        let mut registered = false;
        let mut backoff = BACKOFF_MIN;

        loop {
            let desired = self.desired();

            let Some(reg) = desired.clone() else {
                if registered {
                    if let Some(s) = session.as_mut() {
                        match s.remove(false).await {
                            Ok(_) => log::info!("SRP: no usable LAN address, registration removed"),
                            Err(e) => log::warn!("SRP: removing registration failed: {}", e),
                        }
                    }
                    registered = false;
                }
                log::debug!("SRP: no usable LAN address to register");
                if !self.wait_for_change(&desired).await {
                    return;
                }
                continue;
            };

            log::debug!("SRP: registering {:?}", reg);
            let result = match session.as_mut() {
                Some(s) => s.update(reg, &mut log_event).await.map(|_| ()),
                None => match self.connect(reg).await {
                    Ok(mut s) => {
                        let result = s.register(&mut log_event).await.map(|_| ());
                        session = Some(s);
                        result
                    }
                    Err(e) => Err(e),
                },
            };

            match result {
                Ok(()) => {
                    registered = true;
                    backoff = BACKOFF_MIN;
                    let s = session.as_mut().expect("set when registered");
                    let shutdown =
                        wait_for_change(&mut self.listeners, &self.zone, &self.peer_id, &desired);
                    // `Ok` when the registration changed, which the next
                    // round sends.
                    if let Err(e) = s.run(shutdown, &mut log_event).await {
                        log::error!("SRP: registration failed: {}", e);
                        session = None;
                        if !self.sleep_or_change(&mut backoff, &desired).await {
                            return;
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "SRP: registration at {} in {} failed: {}",
                        self.host,
                        self.zone,
                        e
                    );
                    if !e.is_transient() {
                        // find the registrar again on the next try
                        session = None;
                    }
                    if !self.sleep_or_change(&mut backoff, &desired).await {
                        return;
                    }
                }
            }
            if self.listeners.has_changed().is_err() {
                return;
            }
        }
    }

    fn desired(&self) -> Option<Registration> {
        desired(&self.listeners, &self.zone, &self.peer_id)
    }

    /// Resolves the configured server and creates a session for `reg`.
    async fn connect(&self, reg: Registration) -> Result<Session, SrpError> {
        let endpoints: Vec<Endpoint> = tokio::net::lookup_host((self.host.as_str(), self.port))
            .await
            .map_err(|e| SrpError::Discovery(format!("cannot resolve {}: {}", self.host, e)))?
            .map(Endpoint::from)
            .collect();
        let client = SrpClient::with_servers(endpoints)
            .ok_or_else(|| SrpError::Discovery(format!("no address for {}", self.host)))?;
        log::debug!("SRP: server {} at {:?}", self.host, client.servers());
        let key = SrpKey::from_pkcs8(self.key.clone())?;
        Ok(Session::new(client, key, reg, SessionConfig::default()))
    }

    async fn wait_for_change(&mut self, current: &Option<Registration>) -> bool {
        wait_for_change(&mut self.listeners, &self.zone, &self.peer_id, current).await;
        self.listeners.has_changed().is_ok()
    }

    /// Waits for the backoff delay, or until the registration changes.
    /// Returns `false` when the task should end.
    async fn sleep_or_change(
        &mut self,
        backoff: &mut Duration,
        current: &Option<Registration>,
    ) -> bool {
        let delay = *backoff;
        *backoff = (*backoff * 2).min(BACKOFF_MAX);
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = wait_for_change(&mut self.listeners, &self.zone, &self.peer_id, current) => {}
        }
        self.listeners.has_changed().is_ok()
    }
}

/// The registration for the current listen addresses and interfaces.
fn desired(
    listeners: &watch::Receiver<Vec<Multiaddr>>,
    zone: &Name,
    peer_id: &PeerId,
) -> Option<Registration> {
    let allowed = registration::allowed_ips();
    registration::build(zone, peer_id, &listeners.borrow(), |ip| {
        allowed.as_ref().is_none_or(|ips| ips.contains(&ip))
    })
}

/// Completes when the registration differs from `current`, or when the
/// sender of the listen addresses is dropped.
async fn wait_for_change(
    listeners: &mut watch::Receiver<Vec<Multiaddr>>,
    zone: &Name,
    peer_id: &PeerId,
    current: &Option<Registration>,
) {
    loop {
        tokio::select! {
            changed = listeners.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            () = tokio::time::sleep(INTERFACE_POLL) => {}
        }
        if desired(listeners, zone, peer_id) != *current {
            return;
        }
    }
}

fn log_event(event: Event<'_>) {
    match event {
        Event::Registered {
            registration,
            accepted,
            refresh_in,
        } => log::info!(
            "SRP: registered {} at {} with {:?}, lease {} s, refresh in {} s",
            registration
                .host_name()
                .map(|n| n.to_ascii())
                .unwrap_or_default(),
            accepted.server,
            registration.host.addresses,
            accepted.lease().lease,
            refresh_in.as_secs()
        ),
        Event::Renamed { from, to } => log::warn!("SRP: {} renamed to {}", from, to),
        Event::Retrying { error, retry_in } => log::warn!(
            "SRP: refresh failed: {}, retrying in {} s",
            error,
            retry_in.as_secs()
        ),
    }
}

/// Parses `host[:port]`, `[ipv6]:port` or a bare IPv6 address.
pub fn parse_server(server: &str) -> Option<(String, u16)> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    if let Some(rest) = server.strip_prefix('[') {
        let (host, rest) = rest.split_once(']')?;
        let port = match rest.strip_prefix(':') {
            Some(port) => port.parse().ok()?,
            None if rest.is_empty() => DEFAULT_PORT,
            None => return None,
        };
        return Some((host.to_string(), port));
    }
    if server.parse::<std::net::Ipv6Addr>().is_ok() {
        return Some((server.to_string(), DEFAULT_PORT));
    }
    match server.split_once(':') {
        Some((host, port)) if !host.is_empty() => Some((host.to_string(), port.parse().ok()?)),
        Some(_) => None,
        None => Some((server.to_string(), DEFAULT_PORT)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_forms() {
        let parse = |s| parse_server(s);
        assert_eq!(
            parse("sig0lease.dev.zenr.io:8053"),
            Some(("sig0lease.dev.zenr.io".into(), 8053))
        );
        assert_eq!(
            parse("sig0lease.dev.zenr.io"),
            Some(("sig0lease.dev.zenr.io".into(), 53))
        );
        assert_eq!(parse("192.0.2.1:8053"), Some(("192.0.2.1".into(), 8053)));
        assert_eq!(
            parse("[2001:db8::1]:8053"),
            Some(("2001:db8::1".into(), 8053))
        );
        assert_eq!(parse("[2001:db8::1]"), Some(("2001:db8::1".into(), 53)));
        assert_eq!(parse("2001:db8::1"), Some(("2001:db8::1".into(), 53)));
        assert_eq!(parse(""), None);
        assert_eq!(parse("host:port"), None);
        assert_eq!(parse(":8053"), None);
        assert_eq!(parse("[2001:db8::1]x"), None);
    }
}
