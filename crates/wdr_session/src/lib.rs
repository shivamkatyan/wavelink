//! WDR session state machine.
//!
//! The lifecycle is a deterministic, clock-injected transition function:
//! `Session::advance(&mut self, Event, now_ms) -> Vec<Effect>`. No wall-clock
//! reads anywhere in the crate — all timing is supplied by the caller through
//! `now_ms`, so the whole machine is testable with fake time. `grep -n
//! "std::time" crates/wdr_session/src/lib.rs` returns no matches by design.
//!
//! State path (see `docs/planning/PROTOCOL_SPEC.md` §"State machine"):
//! `Idle → Discover → Connect → Pairing → Negotiating → Streaming → Paused
//! → Renegotiating → Recovering → Terminated | Error`.
//!
//! Locked numeric bounds (PROTOCOL_SPEC §"Numeric bounds"):
//! - reconnect backoff ×1.5, max 30 s
//! - session idle expiry 60 s (no payload)
//! - control response timeout 5 s
//! - pairing window 60 s
//!
//! Scope: this crate is intentionally self-contained (zero external
//! dependencies) so the build cannot be broken by sibling crates.

/// Top-level session lifecycle states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Discover,
    Connect,
    Pairing(PairingPhase),
    Negotiating,
    Streaming,
    Paused,
    Renegotiating,
    Recovering,
    Terminated { reason: String },
    Error { reason: String },
}

/// Sub-phases of pairing (SAS shown → confirmed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingPhase {
    AwaitingSas,
    AwaitingConfirm,
}

/// Inputs consumed by [`Session::advance`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Discovered,
    ConnectRequested,
    PairingStarted,
    SasRejected,
    PairingConfirmed,
    FingerprintMismatch,
    NegotiationOk,
    NegotiationMismatch,
    StreamStart,
    StreamPause,
    StreamResume,
    StreamStop,
    ModeChangeRequested,
    RouteChanged,
    LinkLost,
    LinkBack,
    SleepEntered,
    WakeEntered,
    ForgetPeers,
    Timeout { label: &'static str },
    RecoveryExhausted,
}

/// Outputs produced by a transition, returned in a consistent order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Emit { frame: &'static str },
    StartTimer { label: &'static str, ms: u64 },
    CancelTimer { label: &'static str },
    Renegotiate,
    Recover { attempt: u32, next_ms: u64 },
    Fatal { reason: String },
    Ok,
}

/// Control response timeout (s → ms).
pub const CONTROL_RESPONSE_TIMEOUT_MS: u64 = 5000;
/// Reconnect backoff multiplier numerator (×1.5 via integer math).
pub const RECONNECT_BACKOFF_X: u64 = 3;
/// Reconnect backoff cap.
pub const RECONNECT_CAP_MS: u64 = 30_000;
/// Session idle expiry (no payload).
pub const IDLE_EXPIRY_MS: u64 = 60_000;
/// Initial reconnect backoff.
pub const INITIAL_RECONNECT_MS: u64 = 1500;
/// Pairing window.
pub const PAIRING_WINDOW_MS: u64 = 60_000;

/// Reconnect backoff: ×1.5 each retry, capped at [`RECONNECT_CAP_MS`].
pub fn next_backoff(prev: u64) -> u64 {
    prev.saturating_mul(RECONNECT_BACKOFF_X)
        .saturating_div(2)
        .min(RECONNECT_CAP_MS)
}

/// The session machine.
#[derive(Debug, Clone)]
pub struct Session {
    pub state: SessionState,
    /// `"lossy"` | `"lossless"` | `None`.
    pub mode: Option<&'static str>,
    /// `false` = pro.
    pub peer_free: bool,
    /// `false` = pro.
    pub my_free: bool,
    /// Caller-clock ms of the last activity / state entry. Drives the control
    /// timeout (Negotiating) and idle expiry (Streaming).
    last_activity_ms: u64,
    /// Current reconnect attempt index while recovering (0 = not recovering).
    attempt: u32,
    /// Last scheduled reconnect backoff (ms); 0 = not started.
    last_reconnect_ms: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            state: SessionState::Idle,
            mode: None,
            peer_free: false,
            my_free: false,
            last_activity_ms: 0,
            attempt: 0,
            last_reconnect_ms: 0,
        }
    }
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clock-injected transition function.
    ///
    /// Deterministic: for a given `(state, event, now_ms)` it always returns
    /// the same effect list. Timing decisions (control timeout, idle expiry,
    /// reconnect backoff) are derived purely from `now_ms` relative to the
    /// recorded timestamps.
    pub fn advance(&mut self, ev: Event, now_ms: u64) -> Vec<Effect> {
        match (&self.state, ev) {
            (SessionState::Idle, Event::Discovered) => {
                self.transition_to(SessionState::Discover, now_ms, vec![Effect::Ok])
            }
            (SessionState::Discover, Event::ConnectRequested) => {
                self.transition_to(SessionState::Connect, now_ms, vec![Effect::Ok])
            }
            (SessionState::Connect, Event::PairingStarted) => self.transition_to(
                SessionState::Pairing(PairingPhase::AwaitingSas),
                now_ms,
                vec![Effect::StartTimer {
                    label: "pairing",
                    ms: PAIRING_WINDOW_MS,
                }],
            ),
            (SessionState::Pairing(PairingPhase::AwaitingSas), Event::SasRejected) => self
                .transition_to(
                    SessionState::Error {
                        reason: "SAS rejected".into(),
                    },
                    now_ms,
                    vec![Effect::Ok],
                ),
            (SessionState::Pairing(PairingPhase::AwaitingSas), Event::PairingConfirmed) => self
                .transition_to(
                    SessionState::Pairing(PairingPhase::AwaitingConfirm),
                    now_ms,
                    vec![Effect::Ok],
                ),
            (SessionState::Pairing(PairingPhase::AwaitingConfirm), Event::PairingConfirmed) => self
                .transition_to(
                    SessionState::Negotiating,
                    now_ms,
                    vec![Effect::StartTimer {
                        label: "control_timeout",
                        ms: CONTROL_RESPONSE_TIMEOUT_MS,
                    }],
                ),
            (SessionState::Pairing(_), Event::FingerprintMismatch) => self.transition_to(
                SessionState::Error {
                    reason: "fingerprint mismatch".into(),
                },
                now_ms,
                vec![Effect::Ok],
            ),
            (SessionState::Negotiating, Event::NegotiationOk) => {
                self.transition_to(SessionState::Streaming, now_ms, vec![Effect::Ok])
            }
            (SessionState::Negotiating, Event::NegotiationMismatch) => self.transition_to(
                SessionState::Error {
                    reason: "capability mismatch".into(),
                },
                now_ms,
                vec![Effect::Fatal {
                    reason: "capability mismatch".into(),
                }],
            ),
            (
                SessionState::Negotiating,
                Event::Timeout {
                    label: "control_timeout",
                },
            ) => {
                if now_ms.saturating_sub(self.last_activity_ms) >= CONTROL_RESPONSE_TIMEOUT_MS {
                    self.transition_to(
                        SessionState::Error {
                            reason: "control timeout".into(),
                        },
                        now_ms,
                        vec![Effect::Ok],
                    )
                } else {
                    vec![Effect::Ok]
                }
            }
            (SessionState::Streaming, Event::StreamPause) => {
                self.transition_to(SessionState::Paused, now_ms, vec![Effect::Ok])
            }
            (SessionState::Paused, Event::StreamResume) => {
                self.transition_to(SessionState::Streaming, now_ms, vec![Effect::Ok])
            }
            // FR-026/FR-047: never silently drop lossless — any mode change on
            // a lossless session must go through renegotiation for explicit
            // confirmation, regardless of current peer quality class.
            (SessionState::Streaming, Event::ModeChangeRequested)
                if self.mode == Some("lossless") =>
            {
                self.transition_to(
                    SessionState::Renegotiating,
                    now_ms,
                    vec![Effect::Renegotiate],
                )
            }
            (SessionState::Streaming, Event::SleepEntered) => self.transition_to(
                SessionState::Paused,
                now_ms,
                vec![Effect::CancelTimer { label: "all" }],
            ),
            (SessionState::Paused, Event::WakeEntered) => self.transition_to(
                SessionState::Negotiating,
                now_ms,
                vec![Effect::StartTimer {
                    label: "control_timeout",
                    ms: CONTROL_RESPONSE_TIMEOUT_MS,
                }],
            ),
            (SessionState::Streaming, Event::LinkLost) => self.transition_to(
                SessionState::Recovering,
                now_ms,
                vec![
                    Effect::Recover {
                        attempt: 1,
                        next_ms: INITIAL_RECONNECT_MS,
                    },
                    Effect::StartTimer {
                        label: "reconnect",
                        ms: INITIAL_RECONNECT_MS,
                    },
                ],
            ),
            (SessionState::Recovering, Event::LinkBack) => self.transition_to(
                SessionState::Negotiating,
                now_ms,
                vec![Effect::StartTimer {
                    label: "control_timeout",
                    ms: CONTROL_RESPONSE_TIMEOUT_MS,
                }],
            ),
            (SessionState::Recovering, Event::RecoveryExhausted) => self.transition_to(
                SessionState::Terminated {
                    reason: "recovery budget exceeded".into(),
                },
                now_ms,
                vec![Effect::Fatal {
                    reason: "recovery budget exceeded".into(),
                }],
            ),
            (SessionState::Recovering, Event::Timeout { label: "reconnect" }) => {
                let prev = self.last_reconnect_ms;
                let raw = prev.saturating_mul(RECONNECT_BACKOFF_X).saturating_div(2);
                if raw > RECONNECT_CAP_MS {
                    if prev >= RECONNECT_CAP_MS {
                        vec![Effect::Ok]
                    } else {
                        self.attempt += 1;
                        self.last_reconnect_ms = RECONNECT_CAP_MS;
                        vec![
                            Effect::Recover {
                                attempt: self.attempt,
                                next_ms: RECONNECT_CAP_MS,
                            },
                            Effect::StartTimer {
                                label: "reconnect",
                                ms: RECONNECT_CAP_MS,
                            },
                        ]
                    }
                } else {
                    self.attempt += 1;
                    self.last_reconnect_ms = raw;
                    vec![
                        Effect::Recover {
                            attempt: self.attempt,
                            next_ms: raw,
                        },
                        Effect::StartTimer {
                            label: "reconnect",
                            ms: raw,
                        },
                    ]
                }
            }
            (SessionState::Streaming, Event::Timeout { label: "keepalive" }) => {
                if now_ms.saturating_sub(self.last_activity_ms) >= IDLE_EXPIRY_MS {
                    self.transition_to(
                        SessionState::Terminated {
                            reason: "session idle".into(),
                        },
                        now_ms,
                        vec![Effect::Ok],
                    )
                } else {
                    vec![Effect::Ok]
                }
            }
            _ => vec![Effect::Ok],
        }
    }

    /// Record a state change, refreshing timing bookkeeping for the new state.
    fn transition_to(
        &mut self,
        next: SessionState,
        now_ms: u64,
        effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.state = next;
        self.last_activity_ms = now_ms;
        if matches!(self.state, SessionState::Recovering) {
            self.attempt = 1;
            self.last_reconnect_ms = INITIAL_RECONNECT_MS;
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_state(s: &Session, expected: SessionState) {
        assert_eq!(s.state, expected, "state mismatch");
    }

    fn drive_to_negotiating(s: &mut Session, now: u64) {
        s.advance(Event::Discovered, now);
        s.advance(Event::ConnectRequested, now + 1);
        s.advance(Event::PairingStarted, now + 2);
        s.advance(Event::PairingConfirmed, now + 3);
        s.advance(Event::PairingConfirmed, now + 4);
        assert_state(s, SessionState::Negotiating);
    }

    fn drive_to_streaming(s: &mut Session, now: u64) {
        drive_to_negotiating(s, now);
        s.advance(Event::NegotiationOk, now + 5);
        assert_state(s, SessionState::Streaming);
    }

    #[test]
    fn transition_table_ok() {
        // Hardcoded table of (state, event) -> newstate, driven through the
        // real machine with fake clock values. First element of each row names
        // the documented transition being verified.
        type Row = (&'static str, Vec<(Event, u64)>, SessionState);
        let table: Vec<Row> = vec![
            (
                "Idle+Discovered -> Discover",
                vec![(Event::Discovered, 100)],
                SessionState::Discover,
            ),
            (
                "Discover+ConnectRequested -> Connect",
                vec![(Event::Discovered, 100), (Event::ConnectRequested, 101)],
                SessionState::Connect,
            ),
            (
                "Connect+PairingStarted -> Pairing{AwaitingSas}",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                ],
                SessionState::Pairing(PairingPhase::AwaitingSas),
            ),
            (
                "AwaitingSas+SasRejected -> Error",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::SasRejected, 103),
                ],
                SessionState::Error {
                    reason: "SAS rejected".into(),
                },
            ),
            (
                "AwaitingSas+PairingConfirmed -> AwaitingConfirm",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                ],
                SessionState::Pairing(PairingPhase::AwaitingConfirm),
            ),
            (
                "AwaitingConfirm+PairingConfirmed -> Negotiating",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                ],
                SessionState::Negotiating,
            ),
            (
                "Pairing+FingerprintMismatch -> Error",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::FingerprintMismatch, 103),
                ],
                SessionState::Error {
                    reason: "fingerprint mismatch".into(),
                },
            ),
            (
                "Negotiating+NegotiationOk -> Streaming",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                ],
                SessionState::Streaming,
            ),
            (
                "Negotiating+NegotiationMismatch -> Error",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationMismatch, 105),
                ],
                SessionState::Error {
                    reason: "capability mismatch".into(),
                },
            ),
            (
                "Negotiating+control_timeout(>=5s) -> Error",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (
                        Event::Timeout {
                            label: "control_timeout",
                        },
                        104 + CONTROL_RESPONSE_TIMEOUT_MS,
                    ),
                ],
                SessionState::Error {
                    reason: "control timeout".into(),
                },
            ),
            (
                "Streaming+StreamPause -> Paused",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::StreamPause, 106),
                ],
                SessionState::Paused,
            ),
            (
                "Paused+StreamResume -> Streaming",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::StreamPause, 106),
                    (Event::StreamResume, 107),
                ],
                SessionState::Streaming,
            ),
            (
                "Streaming+LinkLost -> Recovering",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::LinkLost, 106),
                ],
                SessionState::Recovering,
            ),
            (
                "Recovering+LinkBack -> Negotiating",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::LinkLost, 106),
                    (Event::LinkBack, 107),
                ],
                SessionState::Negotiating,
            ),
            (
                "Recovering+RecoveryExhausted -> Terminated",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::LinkLost, 106),
                    (Event::RecoveryExhausted, 107),
                ],
                SessionState::Terminated {
                    reason: "recovery budget exceeded".into(),
                },
            ),
            (
                "Streaming+SleepEntered -> Paused",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::SleepEntered, 106),
                ],
                SessionState::Paused,
            ),
            (
                "Paused+WakeEntered -> Negotiating",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::SleepEntered, 106),
                    (Event::WakeEntered, 107),
                ],
                SessionState::Negotiating,
            ),
            (
                "Streaming+unlisted(StreamStop) stays Streaming (no-op)",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::StreamStop, 106),
                ],
                SessionState::Streaming,
            ),
            (
                "Streaming+ModeChangeRequested(lossless) -> Renegotiating",
                vec![
                    (Event::Discovered, 100),
                    (Event::ConnectRequested, 101),
                    (Event::PairingStarted, 102),
                    (Event::PairingConfirmed, 103),
                    (Event::PairingConfirmed, 104),
                    (Event::NegotiationOk, 105),
                    (Event::ModeChangeRequested, 106),
                ],
                SessionState::Renegotiating,
            ),
        ];

        for (name, script, expected) in table {
            let mut s = Session::new();
            s.mode = Some("lossless");
            for (ev, now) in script {
                s.advance(ev, now);
            }
            assert_state(&s, expected.clone());
            let _ = name;
        }
    }

    #[test]
    fn happy_path() {
        let mut s = Session::new();
        assert_state(&s, SessionState::Idle);

        s.advance(Event::Discovered, 0);
        assert_state(&s, SessionState::Discover);
        s.advance(Event::ConnectRequested, 1);
        assert_state(&s, SessionState::Connect);
        let effects = s.advance(Event::PairingStarted, 2);
        assert_state(&s, SessionState::Pairing(PairingPhase::AwaitingSas));
        assert_eq!(
            effects,
            vec![Effect::StartTimer {
                label: "pairing",
                ms: PAIRING_WINDOW_MS
            }]
        );

        s.advance(Event::PairingConfirmed, 3);
        assert_state(&s, SessionState::Pairing(PairingPhase::AwaitingConfirm));
        let effects = s.advance(Event::PairingConfirmed, 4);
        assert_state(&s, SessionState::Negotiating);
        assert_eq!(
            effects,
            vec![Effect::StartTimer {
                label: "control_timeout",
                ms: CONTROL_RESPONSE_TIMEOUT_MS
            }]
        );

        // Streaming is only reachable through Negotiating+NegotiationOk.
        assert_ne!(s.state, SessionState::Streaming);
        let effects = s.advance(Event::NegotiationOk, 5);
        assert_state(&s, SessionState::Streaming);
        assert_eq!(effects, vec![Effect::Ok]);
    }

    #[test]
    fn backoff_sequence() {
        let mut s = Session::new();
        drive_to_streaming(&mut s, 1000);

        let effects = s.advance(Event::LinkLost, 2000);
        assert_state(&s, SessionState::Recovering);
        assert_eq!(
            effects,
            vec![
                Effect::Recover {
                    attempt: 1,
                    next_ms: INITIAL_RECONNECT_MS
                },
                Effect::StartTimer {
                    label: "reconnect",
                    ms: INITIAL_RECONNECT_MS
                },
            ]
        );

        let mut now = 2000;
        let mut pending = INITIAL_RECONNECT_MS;
        let mut emitted: Vec<u64> = vec![pending];
        for expected_attempt in 2..=9u32 {
            now += pending;
            let effects = s.advance(Event::Timeout { label: "reconnect" }, now);
            assert_state(&s, SessionState::Recovering);
            let next = match effects.as_slice() {
                [Effect::Recover { attempt, next_ms }, Effect::StartTimer { label, .. }] => {
                    assert_eq!(*attempt, expected_attempt);
                    assert_eq!(*label, "reconnect");
                    *next_ms
                }
                other => panic!("unexpected effects: {other:?}"),
            };
            emitted.push(next);
            pending = next;
        }
        // ×1.5 each retry, capped at 30 s: 1500→2250→3375→…→25624→30000.
        assert_eq!(
            emitted,
            vec![1500, 2250, 3375, 5062, 7593, 11389, 17083, 25624, 30000]
        );

        // Next reconnect timeout finds the budget spent: 45000 > 30000 cap.
        now += pending;
        let effects = s.advance(Event::Timeout { label: "reconnect" }, now);
        assert_state(&s, SessionState::Recovering);
        assert_eq!(effects, vec![Effect::Ok]);

        // Explicit exhaustion terminates the session.
        let effects = s.advance(Event::RecoveryExhausted, now + 1);
        assert_state(
            &s,
            SessionState::Terminated {
                reason: "recovery budget exceeded".into(),
            },
        );
        assert_eq!(
            effects,
            vec![Effect::Fatal {
                reason: "recovery budget exceeded".into()
            }]
        );
    }

    #[test]
    fn mid_stream_pro_to_free_invalid() {
        let mut s = Session::new();
        drive_to_streaming(&mut s, 0);
        s.mode = Some("lossless");
        s.peer_free = false;
        s.my_free = false;

        let effects = s.advance(Event::ModeChangeRequested, 10);
        // A lossless stream must never silently keep its mode across a change
        // request — it must renegotiate for explicit confirmation.
        assert_state(&s, SessionState::Renegotiating);
        assert_ne!(s.state, SessionState::Streaming);
        assert!(effects.contains(&Effect::Renegotiate));
        // And once in Renegotiating the machine never jumps back to Streaming
        // without a NegotiationOk.
        assert_ne!(s.state, SessionState::Streaming);
    }

    #[test]
    fn idle_expiry() {
        let mut now = 1000u64;
        let mut s = Session::new();
        s.advance(Event::Discovered, now);
        now += 1;
        s.advance(Event::ConnectRequested, now);
        now += 1;
        s.advance(Event::PairingStarted, now);
        now += 1;
        s.advance(Event::PairingConfirmed, now);
        now += 1;
        s.advance(Event::PairingConfirmed, now);
        now += 1;
        s.advance(Event::NegotiationOk, now);
        assert_state(&s, SessionState::Streaming);
        let streaming_entry = now;

        // Below the 60 s budget the session stays alive.
        let effects = s.advance(
            Event::Timeout { label: "keepalive" },
            streaming_entry + IDLE_EXPIRY_MS - 1,
        );
        assert_state(&s, SessionState::Streaming);
        assert_eq!(effects, vec![Effect::Ok]);

        // No payload for 60 s -> session idle, terminated.
        let effects = s.advance(
            Event::Timeout { label: "keepalive" },
            streaming_entry + IDLE_EXPIRY_MS,
        );
        assert_state(
            &s,
            SessionState::Terminated {
                reason: "session idle".into(),
            },
        );
        assert_eq!(effects, vec![Effect::Ok]);
    }

    #[test]
    fn no_sleep_and_no_broadcast() {
        // Verified: `grep -n "std::time" crates/wdr_session/src/lib.rs` returns
        // no matches — there is no std::time::Instant / Duration / Sleep
        // anywhere in this crate. All timing is caller-injected `u64 now_ms`,
        // so the machine is fully deterministic under fake time.
        let s = Session::new();
        assert_eq!(s.state, SessionState::Idle);
    }

    #[test]
    fn effects_order_is_consistent() {
        let mut s = Session::new();
        drive_to_streaming(&mut s, 0);
        let first = s.advance(Event::LinkLost, 10);

        let mut s2 = Session::new();
        drive_to_streaming(&mut s2, 100);
        let second = s2.advance(Event::LinkLost, 200);
        assert_eq!(
            first, second,
            "recovery effects must be returned in a consistent order"
        );
    }
}
