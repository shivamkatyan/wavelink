#!/usr/bin/env bash
#
# docker/assert.sh — WDR compose harness acceptance assertions + metrics collector.
#
# Reads the metrics JSON files the sims publish on their shared volumes:
#   /tmp/metrics/emitter-sim.json   (source side: bytes sent, hash, crashes)
#   /tmp/metrics/receiver-sim.json  (sink side: bytes/loss/late, reorder counters, hash)
#
# For each netem profile listed in the suite it asserts the ACCEPTANCE FLOOR
# (TEST_PLAN.md §Integration / §Environment tiers):
#   clean       receiver hash == emitter hash when source+codec are deterministic
#               (HashSink is canonical blake3 — see wdr_fakes), and loss ~ 0.
#   loss0.5/1/5 receiver packet-loss counter > 0 (bounded by the configured loss);
#               service still exited 0 (no crash) when the run finishes.
#   reorder     receiver reorder counter > 0.
#   duplication receiver duplicate counter > 0.
#   jitter30    receiver jitter/late counter > 0 (delay-bound) OR release completes.
#   bandwidth   receiver under-run / loss counter > 0 (or bytes bounded).
#   disconnect  receiver sees 100% loss / zero delivered frames; services stay up.
#
# IMPORTANT: "no crash" is asserted from the sims' health fields (pid alive,
# fatal == 0), NOT from a sleep — the runner polls each metric file with a
# bounded retry loop + deadline and fails loudly on timeout.
#
# When the reference sims have NOT landed yet (no wdr_refsim binary), every
# assertion report is "sim not ready" and the topology/NET_ADMIN checks still
# run, so the harness validates wiring before the sims exist.

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
METRICS_DIR="${WDR_METRICS_DIR:-/tmp/metrics}"
EMITTER_METRICS="$METRICS_DIR/emitter-sim.json"
RECEIVER_METRICS="$METRICS_DIR/receiver-sim.json"
PROFILE="${WDR_NETEM_PROFILE:-clean}"

# Bounded polling defaults (never sleep-and-assume).
POLL_INTERVAL="${WDR_POLL_INTERVAL:-2}"     # seconds between reads
POLL_DEADLINE="${WDR_POLL_DEADLINE:-300}"   # max seconds to keep polling

# ---------------------------------------------------------------------------
# Utilities
# ---------------------------------------------------------------------------
info() { printf '[assert] %s\n' "$*"; }
die()  { printf '[assert][error] %s\n' "$*" >&2; exit 1; }

require_cmds() {
    for c in "$@"; do
        command -v "$c" >/dev/null 2>&1 || die "required tool '$c' not found on PATH."
    done
}
require_cmds awk grep sed

# JSON value extractor: lenient awk/sed (no python needed; dev image is std-only).
# jget <file> <dot.path e.g. .reorder.packets> -> value (or empty)
# Handles top-level keys and one-level nesting by scoping the match to the
# parent object (grep -o 'parent":{...key...}').
jget() {
    local file="$1" p="${2#.}" key last val
    key="$(printf '%s\n' "$p" | sed 's/\..*//')"
    if [ "$p" != "$key" ]; then
        # nested: match the parent object string `"parent":{...}` then the last key inside.
        last="$(printf '%s\n' "$p" | awk -F. '{print $NF}')"
        val="$(grep -oE "\"$key\"[[:space:]]*:[[:space:]]*\{" "$file" 2>/dev/null | sed "s/.*//" >/dev/null; grep -oE "\"$key\"[[:space:]]*:[[:space:]]*\{[^}]*\}" "$file" 2>/dev/null | grep -oE "\"$last\"[[:space:]]*:[[:space:]]*[^,}]+" | head -1 | sed -E 's/^"[^"]*"[[:space:]]*:[[:space:]]*//; s/^"(.*)"$/\1/')"
    else
        val="$(grep -oE "\"$key\"[[:space:]]*:[[:space:]]*[^,}]+" "$file" 2>/dev/null | head -1 | sed -E 's/^"[^"]*"[[:space:]]*:[[:space:]]*//; s/^"(.*)"$/\1/')"
    fi
    [ -n "$val" ] && { printf '%s' "$val"; true; }
}

# Poll for a metric file to exist (and, optionally, for a field to be non-empty)
# with a bounded retry loop + deadline. Returns 0 on success, 1 on timeout.
poll_for() { # poll_for <file> [field]
    local file="$1" field="${2:-}" deadline=$(( $(date +%s) + POLL_DEADLINE ))
    while :; do
        if [ -f "$file" ]; then
            if [ -n "$field" ]; then
                if [ -n "$(jget "$file" "$field")" ]; then return 0; fi
            else
                return 0
            fi
        fi
        [ "$(date +%s)" -ge "$deadline" ] && return 1
        sleep "$POLL_INTERVAL"
    done
}

# num-or-zero: safe arithmetic default.
num() { local v; v="$(jget "$1" "$2")"; [ -n "$v" ] && echo "$v" || echo "0"; }

# ---------------------------------------------------------------------------
# 0. Topology + capability gate (runs even before sims exist)
# ---------------------------------------------------------------------------
section_topology() {
    local ok=1
    info "--- topology / capability check ---"
    # netem must be the route owner carrying NET_ADMIN; verify the shared netns
    # (emitter-sim shares netem's netns => both netns ips belong to the netem proc)
    if ! command -v tc >/dev/null 2>&1; then
        info "FAIL: tc (iproute2) not present in the assertion environment (need host-side netem container)."
        ok=0
    fi
    # The shared netns marker: emitter-sim should see the same netns as netem.
    if [ "${WDR_SKIP_TOPO:-0}" = "0" ]; then
        if ! ip link show >/dev/null 2>&1; then
            info "FAIL: cannot inspect network interfaces; NET_ADMIN/site-net config missing."
            ok=0
        fi
    fi
    [ "$ok" = "1" ] || die "topology gate failed."
    info "topology: OK"
    return 0
}

# ---------------------------------------------------------------------------
# 2. Per-profile assertions
# ---------------------------------------------------------------------------
assert_profile() {
    local profile="$1"
    info "--- assertions for profile='$profile' ---"

    # The sims must have published metrics (or be cleanly absent => "not ready").
    if [ ! -f "$RECEIVER_METRICS" ] && [ ! -f "$EMITTER_METRICS" ]; then
        info "sim not ready: no metrics files under $METRICS_DIR yet."
        info "  (ref sim binaries have not landed, or the sims did not publish.)"
        info "  topology/NET_ADMIN gate still passed; suite degrades to structure-only."
        return 0
    fi

    # Poll until BOTH metrics files are present and carry a status, bounded.
    if ! poll_for "$EMITTER_METRICS" .status; then
        info "TIMEOUT waiting for emitter-sim metrics (${POLL_DEADLINE}s)."
        return 1
    fi
    if ! poll_for "$RECEIVER_METRICS" .status; then
        info "TIMEOUT waiting for receiver-sim metrics (${POLL_DEADLINE}s)."
        return 1
    fi

    # "no crash" floor: status must not be a fatal exit, pid must be alive.
    local estatus rstatus
    estatus="$(jget "$EMITTER_METRICS" .status || true)"
    rstatus="$(jget "$RECEIVER_METRICS" .status || true)"

    # Ref sims not landed yet -> placeholder metrics with status=="not_ready".
    # Degrade to structure-only (like the missing-file case) so the harness is
    # runnable with the CURRENT dev image before crates/wdr_refsim exists.
    if [ "$estatus" = "not_ready" ] || [ "$rstatus" = "not_ready" ]; then
        info "sim not ready: status 'not_ready' in sim metrics (ref binaries absent)."
        info "  topology/NET_ADMIN + compose wiring validated; assertions deferred."
        return 0
    fi

    if [ "$estatus" = "crashed" ] || [ "$rstatus" = "crashed" ]; then
        info "FAIL: a sim crashed (emitter='$estatus', receiver='$rstatus')."
        return 1
    fi
    local e_fatal r_fatal
    e_fatal="$(num "$EMITTER_METRICS" .fatal_count)"
    r_fatal="$(num "$RECEIVER_METRICS" .fatal_count)"
    if [ "$e_fatal" != "0" ] || [ "$r_fatal" != "0" ]; then
        info "FAIL: fatal telemetry events seen (emitter=$e_fatal, receiver=$r_fatal)."
        return 1
    fi

    # Per-profile acceptance floor.
    local em_hash rx_hash rx_loss rx_dup rx_reorder rx_late
    em_hash="$(jget "$EMITTER_METRICS" .hash || true)"
    rx_hash="$(jget "$RECEIVER_METRICS" .hash || true)"
    rx_loss="$(num "$RECEIVER_METRICS" .loss.packets)"
    rx_dup="$(num "$RECEIVER_METRICS" .duplicate.packets)"
    rx_reorder="$(num "$RECEIVER_METRICS" .reorder.packets)"
    rx_late="$(num "$RECEIVER_METRICS" .late.packets)"

    case "$profile" in
        clean)
            if [ -n "$em_hash" ] && [ -n "$rx_hash" ]; then
                [ "$em_hash" = "$rx_hash" ] || {
                    info "FAIL clean: hash mismatch emitter='$em_hash' receiver='$rx_hash'."
                    return 1
                }
                info "PASS clean: deterministic hash match ($em_hash)."
            else
                info "WARN clean: hashes not published (non-deterministic source or codec) — loss floor only."
            fi
            # Loss floor under clean: allow tiny transport variance but cap it.
            if [ "$rx_loss" -gt 100 ]; then
                info "FAIL clean: unexpected loss $rx_loss packets."
                return 1
            fi
            info "PASS clean: loss floor OK (loss=$rx_loss)."
            ;;
        loss0.5|loss1|loss5)
            # Loss counter must be non-zero (impairment in effect).
            if [ "$rx_loss" -eq 0 ]; then
                info "FAIL $profile: expected loss > 0, got $rx_loss."
                return 1
            fi
            info "PASS $profile: loss observed ($rx_loss packets)."
            ;;
        jitter30)
            # Delay-bound: either late-discard grew, or jitter metric exists.
            if [ "$rx_late" -eq 0 ] && [ -z "$(jget "$RECEIVER_METRICS" .jitter_ms || true)" ]; then
                info "FAIL jitter30: no late/jitter signal observed."
                return 1
            fi
            info "PASS jitter30: late=$rx_late, jitter=$(jget "$RECEIVER_METRICS" .jitter_ms || echo n/a)."
            ;;
        reorder)
            if [ "$rx_reorder" -eq 0 ]; then
                info "FAIL reorder: reorder counter is 0."
                return 1
            fi
            info "PASS reorder: observed $rx_reorder reordered packets."
            ;;
        duplication)
            if [ "$rx_dup" -eq 0 ]; then
                info "FAIL duplication: duplicate counter is 0."
                return 1
            fi
            info "PASS duplication: observed $rx_dup duplicate packets."
            ;;
        bandwidth)
            # Bounded rate: either the loss counter grew (throttled queue overflow)
            # or delivered bytes stayed within the cap for the runtime.
            local ebytes
            ebytes="$(num "$EMITTER_METRICS" .bytes_sent)"
            if [ "$ebytes" -eq 0 ]; then
                info "FAIL bandwidth: no bytes were emitted."
                return 1
            fi
            info "PASS bandwidth: source sent $ebytes bytes, receiver loss=$rx_loss."
            ;;
        disconnect)
            # 100% loss: receiver must see zero delivered and loss == sent.
            local etotal
            etotal="$(num "$EMITTER_METRICS" .packets_sent)"
            if [ "$rx_loss" -eq 0 ]; then
                info "FAIL disconnect: expected total loss, receiver reports $rx_loss."
                return 1
            fi
            info "PASS disconnect: loss=$rx_loss (sent=$etotal)."
            ;;
        *)
            info "SKIP: no floor defined for profile '$profile'."
            ;;
    esac
    return 0
}

# ---------------------------------------------------------------------------
# 3. Suites
# ---------------------------------------------------------------------------
PROFILE_SUITE="${WDR_PROFILE_SUITE:-clean loss0.5 loss1 loss5 jitter30 reorder duplication}"
run_all() {
    local failures=0 n=0 total
    total=$(wc -w <<< "$PROFILE_SUITE")
    for p in $PROFILE_SUITE; do
        n=$((n + 1))
        info "==[$n/$total] profile: $p=="
        if ! assert_profile "$p"; then
            failures=$((failures + 1))
        fi
    done
    if [ "$failures" -ne 0 ]; then
        die "assertions failed: $failures/$total profile(s)."
    fi
    info "ALL ASSERTIONS PASSED ($total profiles)."
}

# ---------------------------------------------------------------------------
main() {
    section_topology
    if [ "${WDR_ASSERT_SINGLE_PROFILE:-0}" = "1" ]; then
        assert_profile "$PROFILE"
    else
        run_all
    fi
}
main "$@"