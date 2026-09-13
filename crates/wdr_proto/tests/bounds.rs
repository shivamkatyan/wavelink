//! Bounds + error-taxonomy contract tests (SECURITY_SPEC §4 / §5.1).

use wdr_proto::*;

#[test]
fn numeric_bounds_match_security_spec() {
    assert_eq!(MAX_CONTROL_MSG, 16 * 1024);
    assert_eq!(MAX_FRAME_PAYLOAD, 4096);
    assert_eq!(MAX_REQUEST_RATE, 20);
    assert_eq!(MAX_RATE, 20);
    assert_eq!(MAX_PENDING_HANDSHAKES, 8);
    assert_eq!(PAIRING_WINDOW_SECS, 60);
}

#[test]
fn user_actions_cover_required_set() {
    // The task requires these five actions to be reachable.
    assert_eq!(
        Error::protocol(ErrorCode::LocalNetworkPermission).user_action(),
        UserAction::GrantPermission
    );
    assert_eq!(
        Error::protocol(ErrorCode::PairAttemptRejected).user_action(),
        UserAction::ReconnectDac
    );
    assert_eq!(
        Error::protocol(ErrorCode::PolicyViolation).user_action(),
        UserAction::ChangeBuffer
    );
    assert_eq!(
        Error::protocol(ErrorCode::MissingResource).user_action(),
        UserAction::ReturnToWifi
    );
    assert_eq!(
        Error::protocol(ErrorCode::TooLarge).user_action(),
        UserAction::None
    );
}

#[test]
fn every_error_code_is_classified() {
    use ErrorCode::*;
    for c in [
        Malformed,
        TooLarge,
        VersionTooOld,
        PatternMismatch,
        RateLimited,
        PendingHandshakeFull,
        PairingTimeout,
        PairAttemptRejected,
        SasMismatch,
        AuthenticationFailure,
        PolicyViolation,
        Revoked,
        MissingResource,
        LocalNetworkPermission,
        Internal,
        Reserved,
    ] {
        // is_retryable is total; no panic; action is one of the five.
        let _ = c.is_retryable();
        let _ = Error::protocol(c).user_action();
    }
}

#[test]
fn control_decode_rejects_oversized_input() {
    // A message longer than MAX_CONTROL_MSG must be rejected with
    // LimitExceeded before any parser allocation (SEC-05).
    let big = vec![0u8; MAX_CONTROL_MSG + 1];
    let r: Result<ControlMessage, DecodeError> = unpack(&big);
    assert_eq!(r.unwrap_err(), DecodeError::LimitExceeded);
}

#[test]
fn control_encode_rejects_oversized_output() {
    // Encode is bounded symmetric to decode: too-large control payloads are
    // rejected (SizeExceeded) rather than emitted.
    let big_name = "x".repeat(MAX_CONTROL_MSG);
    let msg = ControlMessage::Hello {
        header: MsgHeader {
            msg_ver: PROTO_MAJOR,
            stream_id: 0,
            seq: 0,
            media_ts: 0,
        },
        device_name: big_name,
    };
    assert_eq!(pack(&msg), Err(EncodeError::SizeExceeded));
}

#[test]
fn frame_decode_rejects_oversized_total() {
    let big = vec![0u8; MAX_FRAME_PAYLOAD + Frame::header_max_len() + 1];
    assert_eq!(
        Frame::unpack(&big).unwrap_err(),
        DecodeError::PayloadOverflow
    );
}
