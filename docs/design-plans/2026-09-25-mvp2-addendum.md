# MVP-2 addendum — decisions and records (2026-09-25 →)

Running record for the MVP-2 (0.9) work: Phase A proves the L2/L4 content
loops live with the current Polytoken+GLM session driver; Phase B ports the
loop to `wa serve` (axum, loopback-only) with a direct z.ai (GLM) driver.
The MVP-1 design plan (`2026-09-24-mvp1-structured-loop.md`) remains the
base reference; this file records post-MVP-1 decisions.

## A.1.2 — Review-surface link affordances (decided 2026-09-25)

Operator backlog note (2026-09-25): the dotted-underline wikilink marker
was an MVP expedient and read like many systems' misspelling marks; use the
wiki's own link affordances as the reference styling.

**Decision: option (b) — vendor a pinned snapshot of the deployed enwiki
(Vector 2022) link-affordance rules** into the renderer
(`vendor/enwiki-link-affordances.css`, inlined into the artifact's style
block, checksum-pinned by test, provenance in `vendor/PROVENANCE.md`).

Values snapshot from the live deployed CSS
(`en.wikipedia.org/w/load.php?…modules=skins.vector.styles&skin=vector-2022`,
2026-09-25): progressive `#36c`, hover `#3056a9` + underline, active
`#233566` + underline, visited `#6a60b0`, no underline at rest; redlink
family `#bf3c2c`/`#9f5555`/`#9f3526`/`#612419` (included for the future
page-existence case). The Vector external-link icon is omitted (externals
keep the progressive color, as on enwiki).

Alternatives evaluated and rejected:

- **(a) Link enwiki ResourceLoader CSS into the artifact** — rejected: the
  artifact is static and must render offline (fixtures/golden tests,
  re-review of saved sessions); load.php output is an unpinned moving
  target; and a remote <link> makes the review record network-dependent.
- **(c) Reuse VisualEditor components (diff machinery / link rendering)** —
  rejected: VE's machinery is a ResourceLoader/OO.ui runtime for the
  editing surface, with no static-artifact packaging path; the artifact is
  built from Parsoid HTML + our own diff, and only the *affordance values*
  needed to match. Re-evaluate only if the artifact ever becomes an
  interactive editing surface.

License note: the snapshot derives from GPL-2.0-or-later sources
(mediawiki/core `content.links.less`, Vector, Codex tokens); one-way
compatible with this repo's GPL-3.0-only.

## Housekeeping

- The MVP-1 design plan's repo-layout block originally annotated LICENSE as
  "GPL-3.0-or-later"; corrected to **GPL-3.0-only** to match `Cargo.toml`
  and `CLAUDE.md` (pre-existing inconsistency, fixed 2026-09-25).
- Temple Fielding round 3 (MOS:TITLES de-italicization) was rejected in
  review ("the original form is correct — they bought the *title*") and is
  **abandoned**; the session is closed with edits 1–2 published (operator
  call, 2026-09-25). Nothing owed on-wiki (TALK note and disclosure log
  cover the published edits).
