//! Screen Recording TCC permission state machine (macOS 13+).
//!
//! ScreenCaptureKit audio capture requires Screen Recording permission
//! (`NSScreenCaptureUsageDescription`; the user grants it in System Settings →
//! Privacy & Security → Screen Recording). There is no public API that answers
//! "is permission granted?" directly — the well-known proxy is whether
//! `SCShareableContent.current` returns real content (macOS 13+).
//!
//! That proxy is **lossy**, and this module stays honest about it:
//!
//! * macOS ≤ 14.3 reports an **empty set without an error** before consent —
//!   indistinguishable from "granted on a headless box with nothing shareable".
//!   We map that to [`PermissionState::NotDetermined`], not `Authorized` (fail
//!   closed: capture never starts on an ambiguous probe).
//! * macOS 14.4+ reports an **error** when consent is missing/revoked; error
//!   text is mapped with [`classify`], which is heuristic (documented below).
//! * Managed/headless hosts produce `NoShareableContent`-style errors that are
//!   mapped to [`PermissionState::Restricted`] (capture impossible regardless
//!   of consent) — sticky by design.
//!
//! The state machine itself is pure and unit-tested with an injected
//! [`PermissionProbe`] fake (denied → never authorizes on that outcome; denied
//! → authorized once the user grants; restricted is sticky; an inconclusive
//! outcome never silently flips a decided state). The real probe
//! ([`ScShareableContentProbe`]) calls the `screencapturekit` wrapper around
//! `SCShareableContent.current`. Runtime consent changes still require a
//! logged-in session (hardware gate, build-check.md).

/// Screen Recording permission state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    /// No conclusive probe yet (fresh install, or ambiguous 13/14.3 empty-set).
    NotDetermined,
    /// Consent explicitly denied (or revoked).
    Denied,
    /// Capture is impossible regardless of consent (managed policy / headless /
    /// no shareable session).
    Restricted,
    /// Consent granted; capture may begin.
    Authorized,
    // NOTE: there is deliberately no `Limited` state. iOS models "limited"
    // library access; Screen Recording TCC on macOS is a single binary
    // grant/deny (no partial-audio mode), so four states fully cover it.
}

/// A single permission probe result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOutcome {
    /// Consent confirmed.
    Authorized,
    /// Consent denied (or revoked). `reason` is the OS message.
    Denied { reason: String },
    /// Capture impossible regardless of consent.
    Restricted { reason: String },
    /// Probe was inconclusive (empty set on ≤14.3, unknown error, …).
    NotDetermined { reason: String },
}

/// Injected permission probe (testable).
pub trait PermissionProbe {
    /// Query the OS-level permission state.
    fn current_permission(&self) -> PermissionOutcome;
}

/// State-machine transition: one probe outcome applied to one state.
///
/// Rules (fail closed, no silent downgrades or upgrades):
/// * `Restricted` is sticky — a managed/headless host never "becomes" allowed
///   because a probe happened to succeed later.
/// * `Authorized` ⇒ proceed; `Denied` ⇒ never authorize (even if the previous
///   state was `Authorized`/`NotDetermined`).
/// * `NotDetermined` is inconclusive: it never flips a decided state.
pub fn transition(from: PermissionState, outcome: PermissionOutcome) -> PermissionState {
    match (from, outcome) {
        (PermissionState::Restricted, _) => PermissionState::Restricted,
        (_, PermissionOutcome::Authorized) => PermissionState::Authorized,
        (_, PermissionOutcome::Denied { .. }) => PermissionState::Denied,
        (_, PermissionOutcome::Restricted { .. }) => PermissionState::Restricted,
        (state, PermissionOutcome::NotDetermined { .. }) => state,
    }
}

/// Gate that holds a probe and the resolved state.
#[derive(Debug)]
pub struct PermissionGate<P> {
    state: PermissionState,
    probe: P,
}

impl<P: PermissionProbe> PermissionGate<P> {
    /// New gate, starting `NotDetermined` until the first probe.
    pub fn new(probe: P) -> Self {
        PermissionGate {
            state: PermissionState::NotDetermined,
            probe,
        }
    }

    /// Current resolved state (may be stale since the last probe).
    pub fn state(&self) -> PermissionState {
        self.state
    }

    /// Only `Authorized` allows capture to start.
    pub fn capture_allowed(&self) -> bool {
        self.state == PermissionState::Authorized
    }

    /// Re-run the probe and advance the state machine.
    pub fn refresh(&mut self) -> PermissionState {
        self.state = transition(self.state, self.probe.current_permission());
        self.state
    }
}

/// Real probe: `SCShareableContent.current` availability via `screencapturekit`
/// (macOS 13+). Requires the Screen Recording TCC prompt / approval to return
/// content (hardware-gated to a logged-in session).
#[derive(Debug, Default)]
pub struct ScShareableContentProbe;

impl PermissionProbe for ScShareableContentProbe {
    fn current_permission(&self) -> PermissionOutcome {
        match screencapturekit::shareable_content::SCShareableContent::get() {
            Ok(content) => {
                let empty = content.displays().is_empty()
                    && content.windows().is_empty()
                    && content.applications().is_empty();
                if empty {
                    PermissionOutcome::NotDetermined {
                        reason: "SCShareableContent returned an empty set — macOS ≤ 14.3 "
                            .to_owned()
                            + "reports empty (not an error) before Screen Recording consent; "
                            + "cannot distinguish 'not yet' from 'denied'.",
                    }
                } else {
                    PermissionOutcome::Authorized
                }
            }
            Err(e) => classify(&e.to_string()),
        }
    }
}

/// Map an `SCShareableContent` error string to a permission outcome.
///
/// Heuristic and intentionally lossy (the OS error text is a C-string from the
/// framework, not a stable enum): denial keywords → [`PermissionOutcome::Denied`];
/// "no capturable content / not running as user / headless" → [`PermissionOutcome::Restricted`];
/// anything else → inconclusive [`PermissionOutcome::NotDetermined`] (fail closed —
/// an unrecognised error never authorizes).
fn classify(message: &str) -> PermissionOutcome {
    let lower = message.to_lowercase();
    let denied_hints = [
        "deni",       // denied / denial
        "permission", // permission denied
        "authoriz",   // not authorized / authorization
        "consent",
        "user declined",
        "4600", // SCErrorOperationNotPermitted (-4600) family
        "4601",
    ];
    if denied_hints.iter().any(|hint| lower.contains(hint)) {
        PermissionOutcome::Denied {
            reason: message.to_string(),
        }
    } else if lower.contains("no capturable content")
        || lower.contains("not running in user session")
        || lower.contains("logged in")
        || lower.contains("headless")
    {
        PermissionOutcome::Restricted {
            reason: message.to_string(),
        }
    } else {
        PermissionOutcome::NotDetermined {
            reason: message.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Scripted probe for state-machine tests.
    #[derive(Debug)]
    struct FakeProbe {
        script: Vec<PermissionOutcome>,
        index: Cell<usize>,
    }

    impl FakeProbe {
        fn playing(script: Vec<PermissionOutcome>) -> Self {
            FakeProbe {
                script,
                index: Cell::new(0),
            }
        }
    }

    impl PermissionProbe for FakeProbe {
        fn current_permission(&self) -> PermissionOutcome {
            let i = self.index.get();
            let outcome = self
                .script
                .get(i)
                .cloned()
                .unwrap_or(PermissionOutcome::NotDetermined {
                    reason: "end of script".into(),
                });
            self.index.set(i + 1);
            outcome
        }
    }

    fn denied(reason: &str) -> PermissionOutcome {
        PermissionOutcome::Denied {
            reason: reason.into(),
        }
    }

    fn not_determined(reason: &str) -> PermissionOutcome {
        PermissionOutcome::NotDetermined {
            reason: reason.into(),
        }
    }

    #[test]
    fn not_determined_to_authorized_on_grant() {
        let mut gate = PermissionGate::new(FakeProbe::playing(vec![PermissionOutcome::Authorized]));
        assert_eq!(gate.state(), PermissionState::NotDetermined);
        assert!(!gate.capture_allowed(), "never starts before a probe");
        gate.refresh();
        assert_eq!(gate.state(), PermissionState::Authorized);
        assert!(gate.capture_allowed());
    }

    #[test]
    fn denied_outcome_never_authorizes() {
        let mut gate = PermissionGate::new(FakeProbe::playing(vec![denied("permission denied")]));
        gate.refresh();
        assert_eq!(gate.state(), PermissionState::Denied);
        assert!(!gate.capture_allowed());
        // Defense in depth: an inconclusive probe cannot upgrade a Denied state.
        assert_eq!(
            transition(PermissionState::Denied, not_determined("??")),
            PermissionState::Denied
        );
    }

    #[test]
    fn grant_after_denial_transitions_to_authorized() {
        // User denies, the app parks in Denied, then the user grants in System
        // Settings and we refresh again → Authorized.
        let mut gate = PermissionGate::new(FakeProbe::playing(vec![
            denied("permission denied"),
            PermissionOutcome::Authorized,
        ]));
        gate.refresh();
        assert_eq!(gate.state(), PermissionState::Denied);
        gate.refresh();
        assert_eq!(gate.state(), PermissionState::Authorized);
        assert!(gate.capture_allowed());
    }

    #[test]
    fn revoked_after_authorized_transitions_to_denied() {
        assert_eq!(
            transition(PermissionState::Authorized, denied("revoked")),
            PermissionState::Denied
        );
    }

    #[test]
    fn restricted_is_sticky() {
        assert_eq!(
            transition(PermissionState::Restricted, PermissionOutcome::Authorized),
            PermissionState::Restricted,
            "managed/headless never unlocks on a later successful probe"
        );
        assert_eq!(
            transition(PermissionState::Restricted, not_determined("unknown")),
            PermissionState::Restricted
        );
        let mut gate = PermissionGate::new(FakeProbe::playing(vec![
            PermissionOutcome::Restricted {
                reason: "not running as user".into(),
            },
            PermissionOutcome::Authorized,
        ]));
        gate.refresh();
        assert_eq!(gate.state(), PermissionState::Restricted);
        gate.refresh();
        assert_eq!(gate.state(), PermissionState::Restricted);
    }

    #[test]
    fn inconclusive_outcome_keeps_prior_state() {
        // Fresh gate: inconclusive stays NotDetermined (fail closed).
        let mut gate = PermissionGate::new(FakeProbe::playing(vec![not_determined("empty set")]));
        gate.refresh();
        assert_eq!(gate.state(), PermissionState::NotDetermined);
        // Decided states are not silently downgraded/upgraded by an
        // inconclusive probe.
        assert_eq!(
            transition(PermissionState::Denied, not_determined("??")),
            PermissionState::Denied
        );
        assert_eq!(
            transition(PermissionState::Authorized, not_determined("??")),
            PermissionState::Authorized
        );
    }

    #[test]
    fn classify_recognizes_denial_restricted_and_unknown() {
        assert!(matches!(
            classify("Screen capture permission has been denied"),
            PermissionOutcome::Denied { .. }
        ));
        assert!(matches!(
            classify("The user declined the screen recording permission request"),
            PermissionOutcome::Denied { .. }
        ));
        assert!(matches!(
            classify("No capturable content"),
            PermissionOutcome::Restricted { .. }
        ));
        assert!(matches!(
            classify("no shareable content available: not running in user session"),
            PermissionOutcome::Restricted { .. }
        ));
        // Unrecognised text never authorizes: inconclusive.
        assert!(matches!(
            classify("some framework internal error"),
            PermissionOutcome::NotDetermined { .. }
        ));
    }
}
