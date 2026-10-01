# Decision records (ADRs)

Format follows SP42's ADR conventions (Constitution Article 4) — the same
records wikiactive's ported code already cites (`quote_anchor.rs` references
SP42 ADR-0007 §5). Single-maintainer adaptation: no maintainer-model layer.

## Rules

1. **Link to the ADR or it didn't happen.** A decision made in conversation or
   planning that the work depends on gets a record in the same effort. Design
   plans cite records; they never re-argue them.
2. **Cross-reference by textual ID** (`ADR-0007`), never by file path — records
   can move without breaking links. Code comments may cite IDs (SP42's do).
3. **Records are immutable once committed.** A reversal is a new record that
   names its predecessor in `Status: Supersedes ADR-NNNN`. Never edit a live
   record's decision.
4. **Alternatives considered are mandatory**, with the reason each lost. This is
   the anti-relitigation payload: a model (or future you) reading the record
   must see what was rejected and why, not just what won.
5. **Open questions are recorded inline**, not dropped.

## Format

```
# ADR-NNNN: Title
Status: Accepted | Superseded by ADR-MMMM | Superseded ADR-MMMM  (date)
Context: the forces in play when decided
Decision: the choice, in one or two sentences
Alternatives considered: each option + why rejected
Consequences: what becomes true, what reviewers should (not) flag
Open questions: if any survive the decision
```

## Homing

One flat series for now. If a decision governs a reusable mechanism another
future capability would consume by design (the SP42 reuse test), say so in the
record — that's the signal to split series later.

## Harvest status

Seeded 2026-09-30: ADR-0001 lifted from PLAYBOOK. Loop-mechanization added
ADR-0002 (loop layering), ADR-0003 (loop vocabulary), ADR-0004
(assessments implicitly typed), ADR-0005 (detection vs enforcement) on
2026-10-01. Decisions embedded in `docs/design-plans/*` are unharvested —
lift on first cite (harvest-on-cite), as part of the citing plan's
deliverables.
