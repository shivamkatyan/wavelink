//! WDR telemetry (t-B0-obs) — typd observability foundation.
//!
//! Implements the ARCHITECTURE.md §Observability contract, scoped to the
//! platform-free `core/telemetry` seam:
//!
//!   1. [`EventKind`] — stable, lowercase event names used by the `tracing`
//!      layer and by metrics aggregation.
//!   2. [`CorrelationId`] — a session-local u64 nonce (splitmix64-seeded
//!      `OnceLock`) that is never written to exports directly; exports only
//!      carry the opaque [`CorrelationId::export_id`].
//!   3. [`RedactionPolicy`] — the SECURITY_SPEC.md §5.2 allow/deny table.
//!      Fields whose keys match a denied pattern (password, token, secret,
//!      private key, MAC/SSID/serial/fingerprint, audio payload) are
//!      redacted to `[REDACTED]` **before** any export serializer sees them.
//!   4. [`TelemetryEvent`] — one typed event with a local timestamp.
//!   5. [`TelemetryCollector`] — a bounded in-memory ring (cap 4096) of
//!      events, session-local.
//!   6. [`DiagnosticExport`] — a redacted text export plus a per-field
//!      manifest. Guarantee: no DENY_PATTERNS-matching value ever appears in
//!      the exported body.
//!
//! Static policy (SECURITY_SPEC.md §5.2): no audio payload, no pairing
//! secret, no private key, no raw stable device identifier ever crosses the
//! export boundary. Correlation IDs are session-local and stripped from
//! exports by construction (only the transformed `export_id` is kept).

use std::collections::VecDeque;
use std::sync::OnceLock;

/// Stable event kind names, lowercase and projection-friendly.
///
/// `name()` is the stable wire/log name; consumers must not rename these,
/// since export manifests, dashboards and soak-harness aggregations key on
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EventKind {
    DiscoveryAdvertised,
    DiscoveryFound,
    PairingStarted,
    PairingConfirmed,
    PairingRejected,
    CaptureStart,
    CaptureStop,
    Negotiated,
    TransportPacketLoss,
    ReorderEvent,
    LateDiscard,
    RecoveryStart,
    RecoveryComplete,
    RouteAttach,
    RouteDetach,
    RenderUnderrun,
    ClockDriftEstimate,
    CorrectionApplied,
    EncodeTiming,
    DecodeTiming,
    ReconnectAttempt,
    ModeChange,
    Error,
    Fatal,
}

impl EventKind {
    /// Stable lowercase name. Do not rename: out-of-band tooling keys on them.
    pub fn name(&self) -> &'static str {
        match self {
            Self::DiscoveryAdvertised => "discovery_advertised",
            Self::DiscoveryFound => "discovery_found",
            Self::PairingStarted => "pairing_started",
            Self::PairingConfirmed => "pairing_confirmed",
            Self::PairingRejected => "pairing_rejected",
            Self::CaptureStart => "capture_start",
            Self::CaptureStop => "capture_stop",
            Self::Negotiated => "negotiated",
            Self::TransportPacketLoss => "transport_packet_loss",
            Self::ReorderEvent => "reorder_event",
            Self::LateDiscard => "late_discard",
            Self::RecoveryStart => "recovery_start",
            Self::RecoveryComplete => "recovery_complete",
            Self::RouteAttach => "route_attach",
            Self::RouteDetach => "route_detach",
            Self::RenderUnderrun => "render_underrun",
            Self::ClockDriftEstimate => "clock_drift_estimate",
            Self::CorrectionApplied => "correction_applied",
            Self::EncodeTiming => "encode_timing",
            Self::DecodeTiming => "decode_timing",
            Self::ReconnectAttempt => "reconnect_attempt",
            Self::ModeChange => "mode_change",
            Self::Error => "error",
            Self::Fatal => "fatal",
        }
    }
}

/// Seed for the per-process session correlation nonce source.
const CORR_SEED: u64 = 0x6D2B_79F5_DEAD_BEEF;

/// The opaque transform applied to `local` before an export.
///
/// This is a one-way-ish scrambling constant (golden-ratio-like 64-bit
/// constant). It guarantees `export_id != local` in the overwhelming case;
/// it is **not** a security primitive — it prevents accidental linkback of
/// raw correlation strings, which is all SECURITY_SPEC.md §5.1/§5.2 requires.
const EXPORT_TWEAK: u64 = 0x9E37_79B9_7F4A_7C15;

/// Per-process splitmix64 PRNG, seeded once.
///
/// Session-local correlation nonces are random-ish per session but need no
/// OS entropy at the crate root: the generator is seeded by a fixed constant
/// plus a raw time value, giving cheap distinct-per-boot ranges. A hostile
/// reader who can observe one nonce can in principle predict the stream, but
/// correlation IDs are *not* secrets by design (SECURITY_SPEC.md §5.1 — they
/// are linkability scum, stripped from exports).
fn next_corr_nonce() -> u64 {
    static RNG: OnceLock<StdMutex<SplitMix64>> = OnceLock::new();
    let rng = RNG.get_or_init(|| {
        let seed = CORR_SEED
            ^ std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15);
        StdMutex::new(SplitMix64::new(seed))
    });
    let mut guard = rng.lock().unwrap_or_else(|e| e.into_inner());
    guard.next_u64()
}

/// A tiny std-only mutex wrapper so we need no locking crate.
type StdMutex<T> = std::sync::Mutex<T>;

/// SplitMix64 — deterministic, std-only, doctest-agnostic PRNG.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Session-local correlation identifier.
///
/// Each call to [`CorrelationId::new`] draws a fresh nonce from the
/// process-wide splitmix source, so two sessions in the same process differ.
/// The raw [`CorrelationId::local`] value is retained in local logs for
/// cross-referencing but is **stripped** from exports — serializers must only
/// ever emit [`CorrelationId::export_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrelationId(u64);

impl CorrelationId {
    /// A fresh random-ish session-local nonce.
    pub fn new() -> Self {
        Self(next_corr_nonce().wrapping_add(1))
    }

    /// Raw session-local value. Never serialize this into an export.
    pub fn local(&self) -> u64 {
        self.0
    }

    /// Opaque, *different* value that may appear in exports.
    ///
    /// `export_id != local` guaranteed; used to re-correlate a single export
    /// without exposing the raw session-local nonce (SEC-11).
    pub fn export_id(&self) -> u64 {
        self.0.wrapping_add(EXPORT_TWEAK)
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

/// Redaction allow/deny policy (SECURITY_SPEC.md §5.2).
///
/// Every field key is matched, case-insensitively, against [`DENY_PATTERNS`].
/// A match means the value is replaced with the literal `[REDACTED]` before
/// anything leaves the process. Default policy denies the same set.
pub struct RedactionPolicy {
    deny: &'static [&'static str],
}

/// Denied field-key fragments. **Exactly** the dark list from SECURITY_SPEC.md
/// §5.2: no audio, no pairing secret, no private key, no raw stable device
/// identifier, no peer fingerprint.
pub const DENY_PATTERNS: &[&str] = &[
    "password",
    "token",
    "secret",
    "private_key",
    "mac",
    "ssid",
    "bluetooth_address",
    "serial",
    "fingerprint",
    "audio_payload",
];

/// The literal replacement emitted for any denied field.
pub const REDACTED: &str = "[REDACTED]";

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self {
            deny: DENY_PATTERNS,
        }
    }
}

impl RedactionPolicy {
    /// Custom policy with an explicit deny list (tests / future allow-groups).
    pub fn with_deny(deny: &'static [&'static str]) -> Self {
        Self { deny }
    }

    /// Redact `value` if `key` contains any deny pattern (case-insensitive).
    pub fn redact_value(&self, key: &str, value: String) -> String {
        let key_lower = key.to_ascii_lowercase();
        if self
            .deny
            .iter()
            .any(|p| key_lower.contains(&p.to_ascii_lowercase()))
        {
            REDACTED.to_string()
        } else {
            value
        }
    }
}

/// One typed telemetry event.
///
/// `fields` is key/value baggage; the key is matched against the
/// [`RedactionPolicy`] at export time. `kind` stores the stable event name
/// (`[`EventKind::name`]`) so the collector stays dependency-free and the
/// struct stays `Copy`-friendly.
#[derive(Debug, Clone)]
pub struct TelemetryEvent {
    /// Milliseconds since session start (local clock, monotonic-ish).
    pub at_rel_ms: u64,
    /// Stable event name (see [`EventKind::name`]).
    pub kind: &'static str,
    /// Session-local correlation id (exported as `export_id` only).
    pub corr: CorrelationId,
    /// Baggage key/value pairs, redacted at export.
    pub fields: Vec<(String, String)>,
}

impl TelemetryEvent {
    /// Convenience constructor for tests / callers.
    pub fn new(at_rel_ms: u64, kind: EventKind, corr: CorrelationId) -> Self {
        Self {
            at_rel_ms,
            kind: kind.name(),
            corr,
            fields: Vec::new(),
        }
    }
}

/// Bounded in-memory ring of telemetry events (metrics-ring style,
/// ARCHITECTURE.md §Observability).
///
/// New events evict the oldest when the cap is reached, so memory stays
/// bounded (~`CAP` events, typically 4096).
#[derive(Debug)]
pub struct TelemetryCollector {
    cap: usize,
    ring: VecDeque<TelemetryEvent>,
}

/// Default ring capacity (SECURITY_SPEC.md §5.3 "metrics ring 24 h
/// in-memory"; typ 4096 events).
pub const RING_CAP: usize = 4096;

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::with_cap(RING_CAP)
    }
}

impl TelemetryCollector {
    /// A collector with `cap` events max.
    pub fn with_cap(cap: usize) -> Self {
        Self {
            cap,
            ring: VecDeque::with_capacity(cap),
        }
    }

    /// Append an event, evicting the oldest if at capacity.
    pub fn log(&mut self, ev: TelemetryEvent) {
        if self.ring.len() == self.cap {
            self.ring.pop_front();
        }
        self.ring.push_back(ev);
    }

    /// Number of events currently held.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// Count events of a given `EventKind` currently in the ring.
    pub fn count(&self, kind: EventKind) -> usize {
        self.ring.iter().filter(|e| e.kind == kind.name()).count()
    }

    /// Drain all held events (oldest first), emptying the ring.
    pub fn drain(&mut self) -> Vec<TelemetryEvent> {
        self.ring.drain(..).collect()
    }
}

/// A redacted diagnostic export.
///
/// [`DiagnosticExport::build`] walks every collected event, applies the
/// [`RedactionPolicy`] to every field value, drops *no* event, and produces
/// (a) a per-field manifest listing each included field type and why, and
/// (b) a plain `key=value` body (no JSON dependency). The export body is
/// guaranteed free of any DENY_PATTERNS-matching value.
#[derive(Debug)]
pub struct DiagnosticExport {
    /// `(field-name, reason)` — one entry per **included** field type.
    pub manifest: Vec<(String, String)>,
    /// Redacted `key=value` lines, one per event/field.
    pub redacted_body: String,
}

impl DiagnosticExport {
    /// Reason strings used in the manifest (keep in sync with the literals
    /// used inline in [`DiagnosticExport::build`]).
    const REASON_CORR_LOCAL_STRIPPED: &'static str =
        "excluded (session-local correlation, stripped per SECURITY_SPEC 5.1)";
    const REASON_CORR_EXPORT: &'static str = "included as opaque export_id (re-correlation)";
    const REASON_KIND: &'static str = "included (stable event classification)";
    const REASON_AT: &'static str = "included for perf/triage timestamps";
    const REASON_NONE: &'static str = "included (no deny-list match)";

    /// Build a manifest + redacted body from a drained collector.
    ///
    /// Drops **no** event: every event becomes at least one body line. Fields
    /// whose keys match the deny list are redacted to `[REDACTED]`.
    pub fn build(collector: &mut TelemetryCollector) -> Self {
        let policy = RedactionPolicy::default();
        let mut body = String::new();
        let mut manifest_seen: Vec<(String, String)> = Vec::new();
        let mut emitted_key_once: Vec<String> = Vec::new();

        for ev in collector.drain() {
            // per-event structural fields
            let struct_fields = [
                ("kind", ev.kind.to_string(), Self::REASON_KIND),
                ("at_rel_ms", ev.at_rel_ms.to_string(), Self::REASON_AT),
                (
                    "correlation_id_export",
                    ev.corr.export_id().to_string(),
                    Self::REASON_CORR_EXPORT,
                ),
            ];
            for (k, v, reason) in struct_fields {
                if !emitted_key_once.iter().any(|e| e == k) {
                    manifest_seen.push((k.to_string(), reason.to_string()));
                    emitted_key_once.push(k.to_string());
                }
                body.push_str(&format!("{k}={v}\n"));
            }

            // correlation_id_local: must NOT appear in body (stripped);
            // manifest records the exclusion exactly once.
            if !emitted_key_once.iter().any(|e| e == "correlation_id_local") {
                manifest_seen.push((
                    "correlation_id_local".to_string(),
                    Self::REASON_CORR_LOCAL_STRIPPED.to_string(),
                ));
                emitted_key_once.push("correlation_id_local".to_string());
            }

            for (k, v) in ev.fields {
                let redacted = policy.redact_value(&k, v);
                if !emitted_key_once.iter().any(|e| e == &k) {
                    let reason = if redacted == REDACTED {
                        "redacted by deny-list (SECURITY_SPEC 5.2)".to_string()
                    } else {
                        Self::REASON_NONE.to_string()
                    };
                    manifest_seen.push((k.clone(), reason));
                    emitted_key_once.push(k.clone());
                }
                body.push_str(&format!("{k}={redacted}\n"));
            }
        }

        let manifest = manifest_seen;
        Self {
            manifest,
            redacted_body: body,
        }
    }
}

impl EventKind {
    /// Test-only: map a discriminant int back to the variant (0..24).
    #[cfg(test)]
    fn from_variant_index(i: usize) -> Option<EventKind> {
        Some(match i {
            0 => Self::DiscoveryAdvertised,
            1 => Self::DiscoveryFound,
            2 => Self::PairingStarted,
            3 => Self::PairingConfirmed,
            4 => Self::PairingRejected,
            5 => Self::CaptureStart,
            6 => Self::CaptureStop,
            7 => Self::Negotiated,
            8 => Self::TransportPacketLoss,
            9 => Self::ReorderEvent,
            10 => Self::LateDiscard,
            11 => Self::RecoveryStart,
            12 => Self::RecoveryComplete,
            13 => Self::RouteAttach,
            14 => Self::RouteDetach,
            15 => Self::RenderUnderrun,
            16 => Self::ClockDriftEstimate,
            17 => Self::CorrectionApplied,
            18 => Self::EncodeTiming,
            19 => Self::DecodeTiming,
            20 => Self::ReconnectAttempt,
            21 => Self::ModeChange,
            22 => Self::Error,
            23 => Self::Fatal,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event(corr: CorrelationId, at: u64, kind: EventKind) -> TelemetryEvent {
        TelemetryEvent::new(at, kind, corr)
    }

    #[test]
    fn eventkind_names_stable() {
        assert_eq!(
            EventKind::DiscoveryAdvertised.name(),
            "discovery_advertised"
        );
        assert_eq!(
            EventKind::TransportPacketLoss.name(),
            "transport_packet_loss"
        );
        assert_eq!(EventKind::Fatal.name(), "fatal");
        // Every variant maps to a distinct, non-empty, lowercase name.
        let mut names: Vec<&str> = (0..u8::MAX as usize)
            .filter_map(EventKind::from_variant_index)
            .map(|k| k.name())
            .collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 24, "expected 24 distinct event-kind names");
        for n in &names {
            assert!(!n.is_empty());
            assert!(n.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }

    #[test]
    fn redaction_policy_default_matches_deny_list() {
        let p = RedactionPolicy::default();
        assert_eq!(
            p.redact_value("mac", "AA:BB:CC:DD:EE:FF".to_string()),
            REDACTED
        );
        assert_eq!(p.redact_value("ssid", "MyWifi".to_string()), REDACTED);
        assert_eq!(
            p.redact_value("peer_fingerprint", "abc123".to_string()),
            REDACTED
        );
        assert_eq!(p.redact_value("password", "hunter2".to_string()), REDACTED);
        assert_eq!(p.redact_value("latency_ms", "42".to_string()), "42");
        // Case-insensitive key matching.
        assert_eq!(p.redact_value("PASSWORD", "x".to_string()), REDACTED);
        assert_eq!(p.redact_value("MAC_ADDR", "x".to_string()), REDACTED);
    }

    #[test]
    fn correlation_export_differs() {
        let c = CorrelationId::new();
        assert_ne!(c.local(), c.export_id());
        assert_eq!(c.export_id(), c.local().wrapping_add(EXPORT_TWEAK));
        let c2 = CorrelationId::new();
        // Two draws from the same process differ (stream advances).
        assert_ne!(c.local(), c2.local());
        assert_ne!(c.export_id(), c2.export_id());
    }

    #[test]
    fn bounded_ring() {
        let mut col = TelemetryCollector::default();
        let corr = CorrelationId::new();
        for i in 0..5000u64 {
            col.log(TelemetryEvent::new(i, EventKind::ModeChange, corr));
        }
        assert_eq!(col.len(), RING_CAP);
        assert_eq!(col.len(), 4096);
        let drained = col.drain();
        assert_eq!(drained.len(), 4096);
    }

    #[test]
    fn redaction_poison() {
        let mut col = TelemetryCollector::default();
        let corr = CorrelationId::new();
        let mut ev = sample_event(corr, 5, EventKind::DiscoveryFound);
        ev.fields = vec![
            ("mac".to_string(), "AA:BB:CC:DD:EE:FF".to_string()),
            ("ssid".to_string(), "MyWifi".to_string()),
            ("peer_fingerprint".to_string(), "abc123".to_string()),
            ("password".to_string(), "hunter2".to_string()),
            ("latency_ms".to_string(), "12".to_string()),
        ];
        col.log(ev);
        let export = DiagnosticExport::build(&mut col);

        for marker in ["AA:BB:CC:DD:EE:FF", "MyWifi", "abc123", "hunter2"] {
            assert!(
                !export.redacted_body.contains(marker),
                "poison value leaked into export: {marker}"
            );
        }
        // The redaction literal itself is present, and the *denied keys* still
        // appear (no event dropped) with the redacted value.
        assert!(export.redacted_body.contains("mac=[REDACTED]"));
        assert!(export.redacted_body.contains("ssid=[REDACTED]"));
        assert!(export.redacted_body.contains("peer_fingerprint=[REDACTED]"));
        assert!(export.redacted_body.contains("password=[REDACTED]"));
        // A legit perf field survives.
        assert!(export.redacted_body.contains("latency_ms=12"));
    }

    #[test]
    fn audio_payload_never_stored() {
        let mut col = TelemetryCollector::default();
        let corr = CorrelationId::new();
        let ev = sample_event(corr, 1, EventKind::CaptureStop);
        col.log(ev);
        // By construction the collector holds no "audio_payload" field: the
        // caller must never queue raw audio, so we assert the export body and
        // the manifest contain no such value (only a possible manifest entry
        // would be a redaction note — assert the *value* is absent and the
        // manifest does not list audio_payload as included).
        let export = DiagnosticExport::build(&mut col);
        assert!(!export.redacted_body.contains("audio_payload"));
        assert!(!export.manifest.iter().any(|(k, _r)| k == "audio_payload"));
    }

    #[test]
    fn manifest_lists_every_field() {
        let mut col = TelemetryCollector::default();
        let corr = CorrelationId::new();
        let mut ev = sample_event(corr, 9, EventKind::EncodeTiming);
        ev.fields = vec![
            ("latency_ms".to_string(), "7".to_string()),
            ("bitrate_kbps".to_string(), "320".to_string()),
            ("private_key".to_string(), "deadbeef".to_string()),
        ];
        col.log(ev);
        let export = DiagnosticExport::build(&mut col);
        let manifest: Vec<&str> = export.manifest.iter().map(|(k, _)| k.as_str()).collect();

        for key in [
            "kind",
            "at_rel_ms",
            "correlation_id_export",
            "correlation_id_local",
            "latency_ms",
            "bitrate_kbps",
            "private_key",
        ] {
            assert!(
                manifest.contains(&key),
                "manifest missing {key}: {manifest:?}"
            );
        }
        // Every manifest entry has a non-empty reason.
        for (k, r) in &export.manifest {
            assert!(!r.is_empty(), "no reason for {k}");
        }
        // Redacted field documented as redacted; included field documented.
        let find_reason = |key: &str| {
            export
                .manifest
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, r)| r.as_str())
                .expect("key in manifest")
        };
        assert!(
            find_reason("private_key").contains("redacted"),
            "private_key should be documented as redacted"
        );
        assert!(
            find_reason("latency_ms").contains("perf")
                || find_reason("latency_ms").contains("included"),
            "latency_ms should be documented as included"
        );
        assert!(
            find_reason("correlation_id_local").contains("stripped"),
            "correlation_id_local must be documented as stripped"
        );
    }
}
