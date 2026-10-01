# Phase 4 — Claim-sequencing gate category

Plan Phase 4. State re-verified 2026-09-30 (`f060999`): the claim pass
(`src/checks/gate.rs` `paraphrase_reasons`, L380+) whitespace-normalizes
(`split_whitespace().join(" ")`), skips inherited prose (`base_norm.contains`),
and locates staged prose via `crate::render::locate_block_anchor`.

**ACs:** `loopmech.AC4.1`–`AC4.4`.

## Decisions

- `GateReason::ClaimNotStaged { claim_id, prose, span: None }`,
  disposition `NeedsAnchor`, fires iff normalized prose is in neither
  base nor proposed wikitext; the claim loop `continue`s past the other
  claim checks (sequencing is the primary failure). Inherited claims
  keep today's full skip (`inherited_claim_prose_skips_paraphrase_gate`
  stays green); staged claims keep today's checks.
- Display: `claim {id}: prose staged nowhere — neither in base nor
  proposed wikitext: "{prose}" (claims ride the edit or are pre-existing
  article text)`; `format_reasons` groups it under NEEDS ANCHOR
  automatically (disposition-driven).

## Tasks

1. Failing tests first: `tests/gate.rs` — a claim whose prose is in
   neither text produces the reason (id + prose named, NeedsAnchor);
   inherited claim does NOT fire it (existing pin stays); staged claim
   does NOT fire it. `tests/audit_cli.rs` — the reason groups under
   NEEDS ANCHOR in the CLI report. `Verifies: AC4.1, AC4.2, AC4.3, AC4.4`
2. Implement in `gate.rs` (enum arms: disposition/span/Display; the
   claim-loop head computes `proposed_norm` once).
3. Phase gate: fmt/clippy/test fresh, sonar, commit.
