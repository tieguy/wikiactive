# ADR-0004: Assessments are implicitly typed via `rules[]` — explicitly tentative
Status: Accepted (2026-10-01)
Context: An `Assessment` record carries `rules[]` (WP shortcuts, card ids, linter rule ids) naming what it implicates. A typed-record design (one record kind per rule family) was considered when the record was renamed from `Finding` (loop-mechanization Phase 1).
Decision: Keep one record type with implicit typing through `rules[]`, and treat that typing as tentative by construction: the `rules[]` entries are what the assessment *implicates*, not a schema commitment. `wa` never branches on an assessment's "type"; the gate consumes evidence ids and anchors regardless.
Revisit triggers (recorded so the decision is falsifiable rather than inert):
- if a consumer needs to enumerate assessments of one family (e.g. "all MOS-variety assessments") and hand-filtering `rules[]` stops being adequate — a query surface, not a schema change, comes first;
- if a rule family needs fields no other family uses (e.g. per-source dispositions) — that is the signal the implicit typing is load-bearing past its strength.
Alternatives considered:
- Explicit `type` field (e.g. `kind: "citation" | "variety" | "paraphrase"`) — rejected: a second axis that must be kept consistent with `rules[]` while gating on neither; the type would be derived data with drift risk.
- Separate record kinds per family (`CitationAssessment`, …) — rejected: admission, validation, the file format, the gate, and the prompts would each multiply for zero added enforcement.
Consequences: prompt schema and admission validation stay family-free; reviewers flag code that switches on `rules[]` content as if it were a type system. The revisit triggers above are the sanctioned way to reopen this.
Open questions: none.
