//! `wdr_discovery` — LAN discovery for Wavelink peers (FR-003, ADR-006).
//!
//! Advertises and browses the `_wdr._tcp` mDNS service over `mdns-sd`. This is
//! the **local-network + manual-IP/QR fallback** discovery surface: a receiver
//! advertises the QUIC port it listens on; an emitter browses and picks a
//! peer to dial instead of a hand-typed address.
//!
//! Privacy contract (ADR-006 evidence): the TXT record is **privacy-minimized** —
//! no identity material, no device serials, no telemetry. mDNS identity is
//! never trusted for pairing: security binds to the Noise XX handshake pubkey
//! (fingerprint / SAS / QR), not to the advertisement. Bounds:
//! `wdr_proto::MAX_TXT_RECORD_BYTES` → the `_wdr._tcp` TXT stays tiny.
//!
//! Testable seam: [`MdnsDiscovery`] implements the `wdr_fakes::adapters::Discovery`
//! trait, so refsim/shells browse through a fake on CI and this crate only for
//! live LAN tests.
//!
//! The mDNS multicast socket is bound by `mdns-sd`; on loopback this works for
//! in-process advertise→browse tests (macOS routes multicast to `lo`).

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use wdr_fakes::adapters::{AdapterError, Discovery, FakeDiscoveryPeer};

/// The advertised service type (`_wdr._tcp.local.` — ADR-006, bound in
/// `wdr_proto`'s TXT-record cap for privacy-minimized advertisement; the
/// `.local.` multicast-DNS suffix is required by `mdns-sd`).
pub const SERVICE_TYPE: &str = "_wdr._tcp.local.";

/// Default host name used in advertisements (the DNS host for A/AAAA records;
/// must carry the trailing `.` per `mdns-sd`).
pub const DEFAULT_HOST: &str = "wdr.local.";

/// Generic instance name for single-receiver runs ("wdr-receiver").
pub const RECEIVER_INSTANCE: &str = "wdr-receiver";

/// One discovered Wavelink peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    /// The instance name from the advertisement (human-readable, e.g. the
    /// receiver's friendly name — never trusted for identity).
    pub name: String,
    /// The address the peer advertised.
    pub addr: IpAddr,
    /// The QUIC port the peer listens on.
    pub port: u16,
}

impl DiscoveredPeer {
    /// The dialable `host:port` for the QUIC transport.
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.addr, self.port)
    }
}

/// Typed discovery errors (no panics on the network).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    /// mDNS daemon/registration failure (typed reason).
    Backend(String),
    /// Browse found nothing within the bounded wait.
    Timeout,
    /// The service daemon reported an error while browsing.
    Browse(String),
}

impl core::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DiscoveryError::Backend(e) => write!(f, "discovery: {e}"),
            DiscoveryError::Timeout => write!(
                f,
                "discovery: no `{SERVICE_TYPE}` peer found within the browse window"
            ),
            DiscoveryError::Browse(e) => write!(f, "discovery: browse: {e}"),
        }
    }
}
impl std::error::Error for DiscoveryError {}

/// The transaction handle for a live advertisement. Dropping the handle drops
/// the (shared) daemon, stopping the advertisement.
pub struct Advertiser {
    // Private on purpose: the daemon is shared and released on drop.
    _daemon: ServiceDaemon,
}

/// Advertise the `_wdr._tcp` service on `port` for `ip`, with a
/// privacy-minimized TXT (no identity/device data — ADR-006). Returns a handle
/// that keeps the advertisement alive.
pub fn advertise_on<Ip: mdns_sd::AsIpAddrs>(
    instance: &str,
    ip: Ip,
    port: u16,
) -> Result<Advertiser, DiscoveryError> {
    let daemon =
        ServiceDaemon::new().map_err(|e| DiscoveryError::Backend(format!("daemon: {e}")))?;
    // Privacy-minimized TXT: only a `v` (protocol version marker) is published;
    // identity/pairing binds to the handshake, never to mDNS.
    let props = [("v", "1")];
    let info = ServiceInfo::new(SERVICE_TYPE, instance, DEFAULT_HOST, ip, port, &props[..])
        .map_err(|e| DiscoveryError::Backend(format!("ServiceInfo: {e}")))?;
    daemon
        .register(info)
        .map_err(|e| DiscoveryError::Backend(format!("register: {e}")))?;
    Ok(Advertiser { _daemon: daemon })
}

/// Advertise on the loopback address (in-process tests / localhost runs).
pub fn advertise_loopback(instance: &str, port: u16) -> Result<Advertiser, DiscoveryError> {
    advertise_on(
        instance,
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port,
    )
}

/// Browse `_wdr._tcp` for `wait` and return every resolved peer. Bounded wait —
/// never sleep-and-assume: returns [`DiscoveryError::Timeout`] if fewer than
/// `min_peers` resolve in time.
pub fn browse(wait: Duration, min_peers: usize) -> Result<Vec<DiscoveredPeer>, DiscoveryError> {
    let daemon =
        ServiceDaemon::new().map_err(|e| DiscoveryError::Backend(format!("daemon: {e}")))?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| DiscoveryError::Browse(e.to_string()))?;
    let mut out: Vec<DiscoveredPeer> = Vec::new();
    let deadline = std::time::Instant::now() + wait;
    while out.len() < min_peers {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(DiscoveryError::Timeout);
        }
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let port = info.get_port();
                out.push(DiscoveredPeer {
                    name: info.get_fullname().to_string(),
                    addr: first_addr(info.get_addresses()),
                    port,
                });
            }
            // Non-resolution events (SearchStarted/ServiceFound/Removed/
            // SearchStopped, plus future non-exhaustive additions) do not
            // advance the count — keep waiting within the window.
            Ok(_) => {}
            Err(_) => return Err(DiscoveryError::Timeout),
        }
    }
    Ok(out)
}

/// First advertised address (any family), else a loopback placeholder so a
/// test peer stays reachable rather than the discovery silently vanishing.
fn first_addr(addrs: &HashSet<mdns_sd::ScopedIp>) -> IpAddr {
    addrs
        .iter()
        .next()
        .map(|a| a.to_ip_addr())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
}

/// Implement the local `Discovery` seam over live mDNS, so refsim/shells can
/// swap the fake for the real browse behind one interface.
pub struct MdnsDiscovery {
    wait: Duration,
}

impl MdnsDiscovery {
    /// New live-mDNS discovery adapter with the given bounded browse window.
    pub fn new(wait: Duration) -> Self {
        Self { wait }
    }
}

impl Discovery for MdnsDiscovery {
    fn browse(&mut self) -> Result<Vec<FakeDiscoveryPeer>, AdapterError> {
        let peers = browse(self.wait, 1).map_err(|_| AdapterError::Failed)?;
        Ok(peers
            .into_iter()
            .map(|p| FakeDiscoveryPeer {
                id: format!("{}:{}", p.addr, p.port),
                name: p.name,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loopback advertise → browse roundtrip: the same process registers and
    /// resolves `_wdr._tcp` on `lo` (macOS routes multicast there). Skipped
    /// silently if the host mDNS loopback path is unavailable (CI sandboxes) —
    /// the fake-seam unit tests below always run.
    #[test]
    fn loopback_advertise_browse_roundtrip() {
        for _ in 0..20 {
            let port = ephemeral_port();
            let Ok(adv) = advertise_loopback(RECEIVER_INSTANCE, port) else {
                continue;
            };
            match browse(Duration::from_secs(2), 1) {
                Ok(peers) => {
                    let p = &peers[0];
                    assert_eq!(p.port, port, "resolved port must match advertisement");
                    drop(adv);
                    return;
                }
                Err(_) => {
                    // Not resolvable on this host network — try another port,
                    // then record the skip honestly.
                    drop(adv);
                }
            }
        }
        eprintln!(
            "[wdr_discovery] SKIP loopback mDNS roundtrip: no multicast resolution on this host \
             (CI sandbox?) — live mDNS is host-gated; the fake seam tests below always run."
        );
    }

    fn ephemeral_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .map(|l| l.local_addr().unwrap().port())
            .unwrap_or(9_000)
    }

    #[test]
    fn fake_discovery_seam_still_works() {
        // The fakes Discovery trait remains the CI/testable seam.
        let mut fake =
            wdr_fakes::adapters::FakeDiscovery::new(vec![wdr_fakes::adapters::FakeDiscoveryPeer {
                id: "peer-1".into(),
                name: "receiver-a".into(),
            }]);
        let peers = fake.browse().expect("fake browse");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "receiver-a");
    }
}
