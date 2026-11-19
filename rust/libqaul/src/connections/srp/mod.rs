// Copyright (c) 2026 Open Community Project Association https://ocpa.ch
// This software is published under the AGPLv3 license.

//! # SRP Connection Module
//!
//! DNS-SD Service Registration Protocol (RFC 9665) client, built on the
//! `srp-client` crate from the sig0lease client library.
//!
//! On startup the module loads the node's SRP host key from the storage
//! folder, creating one if none exists yet. The key is the identity behind
//! the first-come-first-served name claim at the SRP server, so it is
//! persisted and reused across restarts.
//!
//! The SRP server and the registration zone are configured in the `srp`
//! section of `config.yaml`. Only the configured server is used; there is no
//! DNS discovery of the registrar.
//!
//! Once the event loop runs, the LAN swarm is registered via SRP, see
//! [`registration`] for the records and [`registrar`] for the task that
//! keeps them registered.

pub mod registrar;
pub mod registration;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use libp2p::Multiaddr;
use srp_client::SrpKey;
use tokio::sync::watch;

use crate::storage::configuration::{Configuration, SrpOptions};
use crate::storage::Storage;

/// File name of the SRP host key (PKCS#8 DER) inside the storage folder.
pub const KEY_FILE_NAME: &str = "srp.pk8";

/// Instance-based SRP state.
pub struct SrpState {
    /// SRP host key, loaded or created on startup.
    key: RwLock<Option<Arc<SrpKey>>>,
    /// Listen addresses of the LAN swarm, watched by the registrar task.
    lan_listeners: watch::Sender<Vec<Multiaddr>>,
}

impl SrpState {
    /// Create an empty SRP state; the key is set by [`Srp::init`].
    pub fn new() -> Self {
        Self {
            key: RwLock::new(None),
            lan_listeners: watch::channel(Vec::new()).0,
        }
    }
}

impl Default for SrpState {
    fn default() -> Self {
        Self::new()
    }
}

/// SRP connection module.
pub struct Srp {}

impl Srp {
    /// Initialize the SRP module.
    ///
    /// Adds the default `srp` section to the configuration file when it is
    /// missing, and loads the SRP host key from the storage folder, creating
    /// and storing a new key if none is present.
    pub fn init(state: &crate::QaulState) {
        // make sure the SRP options are written to the configuration file,
        // replacing a section from before `zone` existed
        let outdated = Configuration::get(state)
            .srp
            .as_ref()
            .is_none_or(|srp| srp.zone.is_none());
        if outdated {
            Configuration::get_mut(state).srp = Some(SrpOptions::default());
            Configuration::save(state);
        }
        let options = Self::options(state);
        log::info!(
            "SRP server: {:?}, zone: {:?}",
            options.server,
            options.zone.unwrap_or_default()
        );

        // load or create the SRP host key
        let path = Self::key_path(state);
        match Self::load_or_create_key(&path) {
            Ok(key) => {
                *state.connections.srp.key.write().unwrap() = Some(Arc::new(key));
            }
            Err(e) => log::error!("SRP key {:?} unavailable: {}", path, e),
        }
    }

    /// Start the task registering the LAN swarm via SRP.
    ///
    /// Needs a running tokio runtime. Does nothing if no server is
    /// configured, or if the key, the server or the zone are not usable.
    pub fn spawn_registrar(state: &crate::QaulState) {
        let options = Self::options(state);
        if options.server.trim().is_empty() {
            log::info!("SRP: no server configured, LAN swarm is not registered");
            return;
        }
        let Some(key) = Self::key(state) else {
            log::error!("SRP: no key, LAN swarm is not registered");
            return;
        };
        let Some((host, port)) = registrar::parse_server(&options.server) else {
            log::error!("SRP: invalid server {:?}", options.server);
            return;
        };
        let zone_str = options.zone.unwrap_or_default();
        let zone = match srp_client::model::parse_zone(&zone_str) {
            Ok(zone) => zone,
            Err(e) => {
                log::error!("SRP: invalid zone {:?}: {}", zone_str, e);
                return;
            }
        };
        let registrar = registrar::Registrar {
            host,
            port,
            zone,
            peer_id: crate::node::Node::get_id(state),
            key: key.pkcs8().to_vec(),
            listeners: state.connections.srp.lan_listeners.subscribe(),
        };
        tokio::spawn(registrar.run());
    }

    /// Report the current listen addresses of the LAN swarm.
    pub fn set_lan_listeners(state: &crate::QaulState, mut listeners: Vec<Multiaddr>) {
        listeners.sort();
        listeners.dedup();
        state
            .connections
            .srp
            .lan_listeners
            .send_if_modified(|current| {
                if *current == listeners {
                    return false;
                }
                log::debug!("SRP: LAN listen addresses {:?}", listeners);
                *current = listeners;
                true
            });
    }

    /// Configured SRP options.
    pub fn options(state: &crate::QaulState) -> SrpOptions {
        Configuration::get(state).srp.clone().unwrap_or_default()
    }

    /// The SRP host key, if it has been loaded.
    pub fn key(state: &crate::QaulState) -> Option<Arc<SrpKey>> {
        state.connections.srp.key.read().unwrap().clone()
    }

    /// Path of the SRP host key file in the storage folder.
    pub fn key_path(state: &crate::QaulState) -> PathBuf {
        Path::new(&Storage::get_path(state)).join(KEY_FILE_NAME)
    }

    /// Load the key at `path`, or generate and store a new one if the file
    /// does not exist. An existing but unreadable key is never overwritten,
    /// since replacing it gives up the name claim.
    fn load_or_create_key(path: &Path) -> Result<SrpKey, String> {
        match fs::read(path) {
            Ok(pkcs8) => {
                let key = SrpKey::from_pkcs8(pkcs8).map_err(|e| e.to_string())?;
                log::trace!("SRP key loaded from {:?}", path);
                Ok(key)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let key = SrpKey::generate().map_err(|e| e.to_string())?;
                write_private(path, key.pkcs8()).map_err(|e| e.to_string())?;
                log::info!("SRP key created at {:?}", path);
                Ok(key)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Writes via a temporary file so a crash never leaves a truncated key
/// behind; the file is only readable by the owner.
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("pk8.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_created_once_and_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE_NAME);

        let created = Srp::load_or_create_key(&path).unwrap();
        assert!(path.exists());
        let loaded = Srp::load_or_create_key(&path).unwrap();
        assert_eq!(created.pkcs8(), loaded.pkcs8());
    }

    #[test]
    fn corrupt_key_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE_NAME);
        fs::write(&path, b"garbage").unwrap();

        assert!(Srp::load_or_create_key(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"garbage");
    }
}
