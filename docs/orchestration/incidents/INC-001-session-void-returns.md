# INC-001 — Repeated void returns on t-B0-session

- **Date:** 2026-09-06
- **Root task:** R-B0-SESSION (crates/wdr_session)
- **Owner:** Protocol/Networking (subagent)
- **Classification:** Contract mismatch / worker-return-contract violation (task produced no output twice; diagnostician attempt produced nothing a third time). Not a product-code defect (no code authored), not a test defect, not a toolchain failure.
- **Symptom:** Three dispatched workers each returned an EMPTY final message (no YAML, no report file, no crate). 0 evidence produced per attempt.
- **Attempts (same hypothesis_id H-SESSION-1):**
  1. t-B0-session worker (full contract) → void.
  2. t-B0-session worker (full contract redispatch) → void.
  3. Independent diagnostician (narrowed, hardened contract + segmentation, alternates required) → void.
- **Evidence boundary:** §15.2 permits at most 3 evidence-producing attempts per unchanged hypothesis. No attempt produced evidence. Repair ladder exhausted for H-SESSION-1.
- **Action per §15.2/15.3:** Incident created; dependent B1 integration marked blocked-on-R-B0-SESSION; independent work continues. The session FSM stays on the plan's critical path and is queued for a short, tightly-scoped re-attempt using a NEW hypothesis (H-SESSION-2) confined to a single self-contained state-machine module with explicit incrementally-verifiable milestones and no dependency on sibling-crate survey.
- **Status:** open → re-attempt with narrowed approach (attempt 1 of new hypothesis).
