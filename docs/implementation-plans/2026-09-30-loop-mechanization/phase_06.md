# Phase 6 — Analyze defect scan

Plan Phase 6. State re-verified 2026-10-01 (`2a3f616`):
`build_context_bundle` (`src/rules.rs`, signature `(corpus, article,
assessments, ledger)`) has `corpus.linter` in scope and `article.wikitext`;
the corpus's `scan_whole_page` is whole-page detection (all severities).

**ACs:** `loopmech.AC9.1`–`AC9.3`.

## Decisions

- The bundle gains a section after the session summary:
  `## Base-article defect candidates (detection only — nothing auto-applies)`
  listing every `scan_whole_page(base)` hit (`[severity] rule (line N):
  detail — description`) plus a scope note: candidates for the assess
  step to judge; the gate's drafted-lines rules apply only to lines this
  tool drafts. Empty scan ⇒ no section.
- Detection is bundle text only: no state is written, nothing feeds the
  gate (structural), pinned observably: a base with defects and an
  identical proposed text still passes `run_gate`, while the bundle
  lists the defects; and the existing drafted-lines pins stay green.

## Tasks

1. Failing test in `tests/rules_corpus.rs`: a base with known defects
   (tense-drift marker, unspaced heading) surfaces them under the
   labeled section with severity and line; a clean base emits no
   section; `run_gate` over that base (identical proposed, no claims)
   does not block — scan output alone never blocks any stage.
   `Verifies: AC9.1, AC9.2`
2. Implement the section in `build_context_bundle`; the drafted-lines
   scope pins (linter suite) stay green. `Verifies: AC9.3`
3. Phase gate: fmt/clippy/test, sonar, commit.
