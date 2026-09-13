# t-B0-obs — wdr_telemetry observability foundation

- **Task:** implement `core/telemetry` (ARCHITECTURE.md §Observability,
  SECURITY_SPEC.md §5.1–§5.4 / T13, SEC-11) as crate `crates/wdr_telemetry`.
- **Attempt:** 1 (round-2 dispatch returned VOID; absence verified on disk
  before work: no `crates/wdr_telemetry`, no `t-B0-obs.md` report).
- **Scope delivered:** single `src/lib.rs` (zero external deps, std only).

## Delivered surface

- `EventKind` — 24 stable lowercase names (`.name()`), exhaustive-probe test.
- `CorrelationId` — session-local u64 nonce from a splitmix64 `OnceLock`
  source (seed = constant ^ boot-time nanos); `local()` never exported;
  `export_id()` = `local` + `0x9E37_79B9_7F4A_7C15` (opaque, differs).
- `RedactionPolicy` / `DENY_PATTERNS` / `REDACTED` — case-insensitive
  key-match deny list (password, token, secret, private_key, mac, ssid,
  bluetooth_address, serial, fingerprint, audio_payload); default policy
  denies those keys. `redact_value(key, value) -> "[REDACTED]" | value`.
- `TelemetryEvent` — `{at_rel_ms, kind, corr, fields}`.
- `TelemetryCollector` — bounded `VecDeque` ring, cap 4096 (`RING_CAP`),
  `log` / `count(EventKind)` / `drain`.
- `DiagnosticExport::build` — drains collector, applies policy to **every**
  field value, drops **no** event, emits plain `key=value` body (no JSON) and
  a per-field manifest with include/redact/strip reasons. Guarantee: no
  DENY_PATTERNS-matching value survives into the body; `correlation_id` local
  is excluded (only `correlation_id_export` appears).

## Tests (in-file `#[cfg(test)]`, 7)

| Test | Asserts |
|---|---|
| `eventkind_names_stable` | 24 distinct lowercase `_`-only names |
| `redaction_policy_default_matches_deny_list` | mac/ssid/fingerprint/password → `[REDACTED]`; latency_ms passes; key match case-insensitive |
| `correlation_export_differs` | `export_id != local`; two session draws differ |
| `bounded_ring` | 5000 pushes cap at 4096 |
| `redaction_poison` | MAC/SSID/fingerprint/password marker values absent from body; denied keys present with `[REDACTED]`; perf field survives |
| `audio_payload_never_stored` | export body + manifest contain no `audio_payload` value |
| `manifest_lists_every_field` | kind/at_rel_ms/corr_export/corr_local + every field key present with non-empty reason; redacted marked redacted, stripped marked stripped |

## Validation (via `source dev/env.sh`)

- `cargo build -p wdr_telemetry` — OK
- `cargo test -p wdr_telemetry` — 7 passed, 0 failed
- `cargo clippy -p wdr_telemetry --all-targets --all-features -- -D warnings` — clean
- `cargo fmt -p wdr_telemetry -- --check` — clean
- `cargo build --workspace` — OK (no sibling crates touched)

No commit made.
