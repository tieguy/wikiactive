# Phase-0 spike notes — manual round-trip log

## Environment

- Node v24.20.0, npx-pinned `lavish-axi@0.1.78` (tarball vendored:
  `vendor/lavish-axi-0.1.78.tgz`, sha256
  `0c10d9352a63a8c8b6be908d1c29cab2f1b68575910cf32e5cb6d8178ba62b82`).
- Rust 1.96.0 via rustup (installed during the spike; repo pins
  rust-version 1.96).

## TOON verification (Phase 0.2) — done

- `lavish-axi --help` output IS TOON (`bin:`, `description:`,
  `playbooks[8]{id,use_when}:` table rows, `help[12]:` quoted arrays).
  Captured: `fixtures/lavish/help-output.toon`, `poll-help.txt`.
- Source-mined payload schema (vendored `dist/cli.mjs`):
  - element comments: `{uid, selector, tag, text(≤240), prompt}` where
    `selector(el)` walks up ≤5 parts, stopping at the first `id`
    (`tag#id`, `:nth-of-type(n)` between) — id-rooted selectors by design;
  - text-range comments add `target = {type:"text-range", text, selector,
    start:{selector, path, offset}, end:{…}}` where `path` is childNodes
    indices and `offset` a text-node char offset;
  - revisions registry: `<script type="application/json"
    data-lavish-revisions>[{id,label,timestamp,summary}]</script>`,
    `data-lavish-revision="<id>"` on changed blocks; legend caps at 6
    entries, malformed registries silently ignored (lavish
    `parseRevisionRegistry`).
- The TOON parser (`src/lavish.rs`) is written against these shapes plus
  the captured fixtures; fixture test
  `parses_real_captured_help_fixture` pins it.

## Live fetch + render round-trip (Phase 0.4, read side) — done

- Fetched `User:LuisVilla` wikitext @ revid 709414140 (Action API,
  identifying UA) and its Parsoid HTML (REST `page/html`, profile
  2.8.0). Stored under `fixtures/lavish/spike/`.
- Built the spike artifact `fixtures/lavish/spike/spike-roundtrip.html` in
  the renderer's shape (`wa-N` + `data-wiki-anchor` ranges, `ev-N` +
  `ledger:Q1`, revisions registry), opened the lavish session
  (`http://…:4387/session/c9306658e8a64100`), poll running in a tracked
  background job.

## Manual annotation round-trip (Phase 0.3) — PENDING OPERATOR

Waiting on the operator's three annotations (element comment, text
selection, evidence comment) + Send. On delivery: capture the real poll
TOON + annotation payload into `fixtures/lavish/`, resolve the selectors
through `wa poll`, and record results here.

## Write round-trip

Rides the Phase 3 OAuth smoke publish (README runbook) — no credentials
were available during Phase 0.

## Gate

Anchor round-trip feasibility is confirmed to the extent testable without
a human hand: id-rooted selectors are produced by lavish for every element
with an `id` (verified in the vendored `selector(el)`), the renderer puts
ids on every annotatable block, and `tests/anchors.rs` pins resolution for
both payload shapes. The one-time human annotation remains to close Phase
0; the fallback (minimal home-built viewer) is not needed.
