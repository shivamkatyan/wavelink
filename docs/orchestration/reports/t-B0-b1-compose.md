# t-B0-b1-compose — Wire the compose harness for the B1 reference sim

- **Task:** R-B1-COMPOSE — finalize `compose.yml` into a runnable impaired-network
  harness (emitter-sim → netem → receiver-sim) with a metrics collector and
  assertion/bootstrapping scripts + a disabled CI job. Owner: DevEx.
- **Attempt:** 1 — **complete** (empirically validated against the running
  Docker engine on this host, not just statically).
- **Note:** `crates/wdr_refsim` had NOT landed when this report was written; the
  harness is designed to be runnable with the CURRENT dev image (services start,
  wait, and report "sim not ready" gracefully while still validating topology /
  NET_ADMIN). This is intentional and satisfies the task's multi-worker
  compatibility requirement.

## What was created / changed (allowed path set only)

| Path | Type | Purpose |
|------|------|---------|
| `compose.yml` | edited (integration-owned) | 4 services, resolved topology, NET_ADMIN confined to `netem` |
| `docker/netem.sh` | new | apply `tc netem` impairment profile; readiness file; keep-alive |
| `docker/assert.sh` | new | per-profile acceptance floor + "no crash" checks, bounded polling |
| `docker/bootstrap-netem.sh` | new | fresh-clone bootstrap: docker+NET_ADMIN preflight, build, suite, teardown (idempotent) |
| `docker/metrics.sh` | new | lightweight collector tailing the shared metrics volume |
| `docker/sim-start.sh` | new | run ref sim via `cargo run -p wdr_refsim` or degrade to "sim not ready" placeholder |
| `.github/workflows/reference-sim.yml` | new | DISABLED (`if: false`) CI job for the reference sim suite |
| `docs/orchestration/reports/t-B0-b1-compose.md` | new | this report |

## Topology (empirically validated)

The B0 skeleton left three `sleep infinity` placeholder services on the default
bridge, which **cannot** be shaped: traffic between two peers on one bridge is
switched at L2 by the Docker bridge and never transits any container, and a
"router" container only works with sims it can add routes for (which needs
NET_ADMIN on the sims too, or a shared netns). The design therefore uses the
**network-namespace shared "sidecar" model**.

```
 ┌─────────────── netem netns (network_mode: service:netem shared by emitter) ───────────────┐
 │  netem  (container, cap_add: [NET_ADMIN] only)          emitter-sim (shares netem netns) │
 │   entrypoint: docker/netem.sh ➜ tc qdisc add dev eth0 root netem <profile>               │
 └───────┬───────────────────────────────────────────────────────────┐                        │
         │ eth0 172.28.0.3/24 (bridge egress path                       └────────────────────┘
         │         = emitter's egress; shaped by netem qdisc)
         ▼
 ┌─── wdr-net (internal bridge 172.28.0.0/24)  ───┐
 └───────┬─────────────────────────────────────────┘
         │ eth0 172.28.0.4/24
 ┌───────▼──────────┐   ┌──────────────────────────────┐
 │ receiver-sim     │   │ metrics (mounts shared volume)│
 │ (plain peer)     │   └──────────────────────────────┘
 └──────────────────┘
```

- **Why NET_ADMIN is only on `netem`:** because `emitter-sim` uses
  `network_mode: service:netem`, the emitter process runs **inside the netem
  netns**. `tc qdisc` on netem's `eth0` therefore shapes the emitter's *local
  egress* — the source→receiver direction that matters — with zero capabilities
  on the sims. Receiver-sim is a plain peer on the same bridge. Verified live:
  `ping` from the shared netns to the receiver showed 0/20 loss clean, 5/5
  (100%) lost under `loss 100%`, restored to 5/5 on delete.
- **Readiness / metrics:** all services mount a shared named volume
  `wdr-metrics:/tmp/metrics`. `netem.sh` writes `netem-ready.json` (profile +
  run id) only after `tc` applies; `sim-start.sh` poll-waits on it with a bounded
  deadline (never sleep-and-assume). Sims write `emitter-sim.json` /
  `receiver-sim.json`; `metrics.sh` aggregates these into `collector.json` +
  `runs.log` (assert-across-runs).
- **Harness even before refsim lands:** `sim-start.sh` checks for
  `crates/wdr_refsim` / built `ref_<role>` binary; if absent it writes a
  `"status":"not_ready"` metrics file and sleeps. `assert.sh` treats
  `not_ready` as structure-only (topology + NET_ADMIN validated, assertions
  deferred), so `bootstrap-netem.sh` is green on a fresh clone with only the
  B0 dev image.
- **Refsim compatibility (no hardcoded binary path):** when the crate exists,
  `sim-start.sh` runs `cargo run --quiet -p wdr_refsim --bin ref_emitter` /
  `ref_receiver` with config passed **via env** (`WDR_METRICS_DIR`,
  `WDR_SIM_ROLE`, `WDR_NETEM_PROFILE`) so the refsim worker can evolve the CLI
  surface freely.

## Profiles (`WDR_NETEM_PROFILE`, default `clean`)

Mapped to `tc` in `docker/netem.sh`, all empirically applied:

| Profile | `tc` command (on netem eth0) | TEST_PLAN anchor |
|---|---|---|
| `clean` | delete root qdisc | loss 0 |
| `loss0.5` | `netem loss 0.5%` | loss {…0.5…} |
| `loss1` | `netem loss 1%` | loss {…1…} |
| `loss5` | `netem loss 5%` | loss {…5…} |
| `jitter30` | `netem delay 30ms 10ms distribution normal` | jitter |
| `reorder` | `netem delay 10ms reorder 25% gap 3` | reorder |
| `duplication` | `netem duplicate 10%` | duplication |
| `bandwidth` | `tbf rate 512kbit burst 32kbit latency 400ms` | bandwidth cap |
| `disconnect` | `netem loss 100%` | disconnect |

`tc missing` → clear `die` with exit != 0; invalid profile → same.

## Acceptance architecture (`docker/assert.sh`)

- **Topology/NET_ADMIN gate first** (runs even when sims are absent): checks
  `tc` present + `ip link` works in the assert environment; fails the suite
  loudly on a non-capable host.
- **"No crash" floor:** reads `status`, `fatal_count` from both sim metrics;
  crashed/fatal → FAIL.
- **Per-profile floor:** clean → receiver hash == emitter hash when both are
  non-null (canonical blake3 per `wdr_fakes::HashSink`) + loss ≤ 100; loss* →
  receiver loss counter > 0; reorder → reorder counter > 0; duplication →
  duplicate > 0; jitter30 → late/jitter signal; bandwidth → source bytes > 0;
  disconnect → receiver loss > 0.
- **Bounded polling:** `poll_for` retries with `POLL_INTERVAL`/`POLL_DEADLINE`
  (defaults 2s/300s) reading `status` from the metrics files — never a
  sleep-and-assume.
- **Where it runs:** the sims' metrics live on the named volume which the host
  cannot read, so `bootstrap-netem.sh` runs `assert.sh` inside a throwaway
  container from the dev image mounting `wdr-metrics` read-only (+ the scripts
  dir). Verified against the live volume.

## Scripts

- `docker/netem.sh` — profile apply → write `netem-ready.json`+`/tmp/netem-ready`
  → `exec sleep infinity`. Shellcheck clean.
- `docker/metrics.sh` — pure std tools (awk/grep/sed — deliberately no python,
  the dev image lacks it): lenient `jget`, writes `collector.json` + `runs.log`.
  Shellcheck clean.
- `docker/sim-start.sh` — bounded wait on netem readiness → run ref sim or write
  `not_ready` placeholder. Shellcheck clean.
- `docker/assert.sh` — see above. Shellcheck clean.
- `docker/bootstrap-netem.sh` — `require_docker` (engine + throwaway internal
  bridge), build image, per-profile `compose up --force-recreate` + containerized
  assert + `down -v`. Idempotent. Shellcheck clean.

## CI job (`.github/workflows/reference-sim.yml`)

- **`if: false` — DISABLED by design** with an enable-marker comment block.
- **Why it cannot run on standard hosted runners:** the `netem` service needs
  `cap_add: [NET_ADMIN]` honored by the Docker daemon. GitHub-hosted
  `ubuntu-latest` runners are themselves nested containers whose daemon cannot
  grant NET_ADMIN to compose services without `--privileged`/`--cap-add`
  (which hosted runners don't expose). So this job declares
  `runs-on: [self-hosted, linux]` and documents that a self-hosted runner with a
  stock `dockerd` (which honors `--cap-add=NET_ADMIN`) is required.
- Steps: checkout → NET_ADMIN preflight (throwaway internal bridge) → build dev
  image → `docker compose config --quiet` → `docker compose up -d --build` →
  `bash docker/bootstrap-netem.sh` (assertion suite) → teardown → diagnostics
  on failure. `if: always()` teardown.

## Capabilities requirement (mirrors DEVELOPMENT_ENVIRONMENT.md note)

The compose file documents prominently and enforces that **NET_ADMIN is granted
to `netem` only** (verified: compose config shows `netem: ['NET_ADMIN']`, all
other services `None`). Required, per DEVELOPMENT_ENVIRONMENT.md
"Docker / dev container": a Docker engine whose bridge driver honors
`cap_add` + in-container `tc` (Linux hosts / Docker Desktop-WSL2 backend). The
same requirement is called out in `compose.yml` comments and the CI workflow.
`DEVELOPMENT_ENVIRONMENT.md` itself was not edited (out of scope for this task);
the note is delivered via `compose.yml` comments + this report + the workflow.

## Validation performed (all on this host, Docker 29.7.2 engine)

- `docker compose config --quiet` → PASS (twice, incl. final state).
- `bash -n docker/*.sh` → all 5 scripts parse.
- `shellcheck -x docker/*.sh` → clean on all 5 scripts (no findings).
- All 9 profiles applied live and verified via `tc -s qdisc show`:
  `loss1`→`netem loss 1%`, `jitter30`→`delay 30ms 10ms`, `reorder`→`delay 10ms
  reorder 25% gap 3`, `duplication`→`duplicate 10%`, `bandwidth`→`tbf 512Kbit`,
  `disconnect`→`loss 100%`.
- Impairment end-to-end (shared netns): 0/20 loss → apply `loss 100%` → 5/5
  lost → delete qdisc → 5/5 OK.
- `emitter-sim` shares netem netns (identical `/proc/self/net/dev` eth0 stats);
  `receiver-sim` separate peer (`172.28.0.4/24`); `metrics` sees shared volume.
- `bootstrap-netem.sh` full run (clean + loss1) green; idempotent (2nd run green).
- `assert.sh` unit check: clean → hash match PASS; loss1 → PASS; reorder non-zero
  → PASS; reorder zero → FAIL; status `not_ready` → structure-only PASS.
- Placeholder sims in live harness write `emitter-sim.json`/`receiver-sim.json`
  with `status:not_ready`; collector wrote valid `collector.json` +
  `runs.log`; containerized assert reports "sim not ready … deferred".
- Final state: `compose down -v` — 0 stray wdr containers/networks/volumes.

## Decisions

- **Topology:** shared-netns sidecar (`network_mode: service:netem`) rather than
  gateway/router-with-NET_ADMIN-on-sims. Sole gateway model needs NET_ADMIN on
  the sims or host routing; shared netns confines NET_ADMIN to netem (verified).
- **Assertions run inside a throwaway container** mounting the named volume
  (host cannot read it) — keeps the authoritative pass/fail independent of the
  `metrics` service while reusing the dev image.
- **Metrics collector = std tools only** (no python) because `Dockerfile.dev`
  does not install python and that file is out of scope to modify.
- **Env-only refsim interface** so `cargo run -p wdr_refsim --bin …` works
  regardless of the refsim worker's CLI parser; no positional flags passed.
- **No commit** (per instructions; supervisor integrates).

## Out of scope / follow-ups

- `crates/wdr_refsim` (refsim worker); the exact metrics-JSON key names are a
  contract the refsim worker should match (`role/status/hash/packets_sent/
  packets_recv/loss/duplicate/reorder/late/fatal_count/bytes_sent`).
  `assert.sh` is lenient about extra/nested keys.
- Enabling the CI job: requires a registered `self-hosted, linux` runner with a
  NET_ADMIN-capable dockerd (enable marker in `.github/workflows/reference-sim.yml`).
- `loss0.5/loss5` acceptance floors only require loss>0 in this revision; a
  bounded *rate* model (loss ≤ expected×k) is a natural next tightening once
  refsim publishes per-run totals.
