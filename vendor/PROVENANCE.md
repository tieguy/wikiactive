# Vendored third-party artifacts — provenance

## lavish-axi-0.1.78.tgz

- Source: npm registry (`lavish-axi@0.1.78`)
- sha256: `0c10d9352a63a8c8b6be908d1c29cab2f1b68575910cf32e5cb6d8178ba62b82`
  (also recorded in `docs/spike-notes.md`)
- Purpose: normative reference for lavish payload/TOON shapes.
- License: per upstream package (see tarball).

## enwiki-link-affordances.css

- Snapshot date: 2026-09-25 (MVP-2 Phase A, A.1.2)
- Deployed source observed live:
  `https://en.wikipedia.org/w/load.php?lang=en&modules=skins.vector.styles&only=styles&skin=vector-2022`
- Upstream sources: mediawiki/core `resources/src/mediawiki.skinning/content.links.less`,
  the Vector 2022 skin, and Codex design tokens.
- License: GPL-2.0-or-later (one-way compatible with this repo's
  GPL-3.0-only). Adaptation notes are in the file header; the verbatim
  upstream rules are quoted there for auditability.
- Decision record: see `docs/design-plans/2026-09-25-mvp2-addendum.md`
  (option (b) chosen; remote ResourceLoader linking and VE JS component
  reuse evaluated and rejected).
