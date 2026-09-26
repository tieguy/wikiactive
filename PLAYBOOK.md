# PLAYBOOK — the wikiactive loop driver

This is the durable process artifact: the session protocol the agent follows
for every article. The tool (`wa`) mechanically enforces the invariants;
this document is the operating manual for the loop itself.

## Session protocol

```
session init ─► [step 0] analyze ─► triage (already done at init) ─► iterate:
                    │                                                │
                    │      propose ONE logical edit (findings → proposed.wikitext)
                    │                                                │
                    │      ledger work: register/fetch/archive/quote/claim
                    │                                                │
                    │      wa render ──► GATE ──► review.html ──► lavish open
                    │                    │                            │
                    │                 blocked                        poll
                    │                    │                            │
                    │              fix + re-render               comments
                    │                                                 │
                    │      resolve comments (anchors.rs) ──► revise ──┘
                    │
                    └──► operator confirms ──► wa publish ──► GATE re-run
                                                  │                │
                                              tty confirm      re-pin base
                                                  │
                                        post-publish: Earwig, TALK note,
                                        disclosure-page log append
```

### 0. `wa session init --article <title> --entry-loop <n>`

Pins the current revid (the session's base) and stores the base wikitext.
Every edit in the session is prepared against this base; a moved base aborts
publish (AC.10), never silently overwrites.

### Step 0 of EVERY iteration: `wa analyze`

Before proposing anything, run `wa analyze <slug>` and read its output. The
context bundle mechanically contains:

- article state (title, pinned base revid, entry loop, wikitext bytes),
- **Tier 1 judgment core, verbatim** (the three clusters + standing
  checklist — this is the "Tier 1 always in context" guarantee),
- the triage-selected trigger cards for the entry loop,
- session summary (ledger sources/quotes/claims, findings).

If the corpus were missing, `analyze` fails loudly — it cannot be silently
dropped.

### Triage — selecting the entry loop (not just "does L1 run")

- **L1 Rescue** — article below basic quality: no structure, no lead, no
  citations. Do the minimum scaffold: MOS-compliant headings, a lead
  sentence with its first citation. Resist doing more.
- **L2 Mine existing sources** — article has citations: verify claims
  against the article's *existing* sources (claim ↔ quote), extract more
  from them, fix citation formats, remove uncited material, Earwig-check
  suspicious passages, kill spamlinks.
- **L3 GA-style pass** — articles already at basic quality, and re-reviews
  of articles the operator improved before (the analyze bundle carries the
  diff since the prior session for drift review). Substantive criteria
  first (V, NOR/SYNTH, NPOV, coverage/UNDUE), stylistic later (MOS:LEAD,
  Words to watch, linter).
- **L4 New sources, one at a time** — each publish unit is one source →
  integrated content. Conflicts between sources become `{{efn}}` notes,
  never silent adjudication.
- **L5 Wikidata writeback** — deferred (schema hooks only).

### One iteration (one logical edit)

1. **Analyze** (step 0, always).
2. **Findings** — author findings against the tier-1 checklist and cards:
   `wa findings add <slug> -` with JSON `{id, wikitext_anchor, rules[],
   evidence: [Q ids], factual_note, proposed_fix, loop}`. Evidence quotes
   MUST already exist in the ledger (register → fetch → quote).
3. **Ledger** — every source: `wa ledger register --url …`, `wa ledger
   fetch --source S1`, `wa ledger archive --source S1` (save-page-now),
   `wa ledger quote --source S1 --text "…"`, `wa ledger claim --prose "…"
   --quotes Q1`. Quotes that don't locate verbatim are rejected at entry.
4. **Propose** — edit `sessions/<slug>/proposed.wikitext` with ONE logical
   edit (scoped like a code-review commit).
5. **Check (fail-fast)** — `wa check <slug>` runs the full gate standalone
   against `proposed.wikitext` (no artifact attempt). Iterate here until it
   passes: the report groups reasons as NEEDS ANCHOR (ledger wiring:
   register/fetch/quote) vs HARD BLOCK (revise prose, quote, or lint), each
   with the offending wikitext span.
6. **Render** — `wa render <slug> --round <n> --summary "<one line>"`. The
   gate runs as mandatory pre-flight; blocked = no artifact, all reasons
   listed. On success: review.html opens in lavish.
7. **Review** — the operator reads the two-pane diff with the evidence
   rail; comments anchor to `wa-N` (wikitext ranges) and `ev-N` (ledger
   quotes).
8. **Poll** — `wa poll <slug>` (or re-poll with `--agent-reply "<msg>"`
   after applying feedback). Comments resolve to anchors; quote the
   resolved span back in your reply so mis-maps are visible.
9. **Revise → re-render → re-poll** until the operator is satisfied.
10. **Publish** — `wa publish <slug> --summary "<scoped summary>"`. The
    gate re-runs; then the ONE human gate: a `/dev/tty` confirmation. The
    model never self-publishes. On success the base re-pins.
11. **Post-publish** — Earwig compare per new web source; TALK provenance
    note; disclosure-page session-log append; screenshots for the
    disclosure page (operator, manual Commons upload).

### Comment conventions (lavish)

- **Edit summaries name a rule only when verified.** "per MOS:PROSE" style
  attributions require the rule to actually say what the edit does — check
  against `rules/canonical/` before naming it. wikiactive house rules
  (e.g. the semicolon guard) are NOT Wikipedia rules and must never be
  attributed to the MOS; describe such edits plainly ("split a
  semicolon-joined sentence for readability") or as house style.

- Comments on **changed blocks** (`wa-N`) resolve to wikitext ranges — fix
  in proposed.wikitext. A **plain** range (`L..:C..-L..:C..`) points into
  the *proposed* wikitext (new side); a **`base:`-prefixed** range
  (`base:L..:C..-L..:C..`) points into the *base* wikitext (old side —
  the removed wording, including pure deletions). When acting on a
  `base:` anchor, quote the base span in your reply.
- Comments on **evidence cards** (`ev-N`) resolve to ledger quotes — the
  comment is about the source/quote, not the prose: swap sources, adjust
  quotes (re-verify!), or note the dispute.
- Always reply with the resolved anchor and the span text you acted on.

### Publish gate invariant

`wa publish` re-runs the same gate as render (AC.11) and then requires an
interactive tty confirmation. If either fails: nothing is written. The
edit summary must be non-empty; the disclosure suffix is appended
mechanically (`LLM-Disclosure: U:LuisVilla/wikiactive`).

### End-of-session artifacts

- **TALK provenance note** — offer source scans on request; summarize what
  was checked (only negative results and the provenance note belong on
  talk, per the disposition ladder).
- **Disclosure-page log append** — one entry per **article session** (each
  `wa session init` is a page-session; the entry lands when that session's
  publishing completes): article, date, drafting model + version, tool code
  revision, per-edit diff links. Entries live on the `/log` subpage;
  idempotent per session id.
- **Screenshots** — capture the live review artifact (itself a browser
  page); upload to Commons as own work per the README's licensing note.
  The evidence-rail round is the canonical citation-review demonstration.
- **Earwig post-checks** — compare each new web source against the
  published revision; record verdicts in the round log.

### Disposition ladder for discovered material

*use in article with attribution → `{{efn}}` → talk page (negative results
and end-of-session provenance only) → drop.*

### Drafting-style guards vs article defects

Rules like the semicolon ban (`semicolon-prose`) are **model-quirk guards**:
they gate every line *this tool drafts* (added-lines enforcement, hard
block) but are not reported as pre-existing article defects — a semicolon
in existing prose may be another editor's (or another model's) style, not
ours to flag. Scope `drafted-lines` in `rules/linter.toml` encodes this
(operator review note, TF live session round 1: the semicolon tic is an
Opus 5.5 drafting quirk).

### Known limitations (by design)

- The lavish revisions legend lists **at most 6 rounds**; the registry in
  the artifact keeps every round, rounds beyond 6 just don't appear in the
  legend.
- Wikitext anchors are line-based (`L..:C..`); col are char columns.
- The linter is regex-level; no `<nowiki>` handling, refs spanning lines
  attribute to the opening line.

## Per-loop rule packs (what `wa analyze` loads)

| Loop | Cards |
|---|---|
| L1 | LEADCITE |
| L2 | RS-TIERS, CLOP, PRIMARY-CARVEOUTS |
| L3 | UNDUE, NOTLITREVIEW, PROSELINE, RECENTISM, LEADCITE, SUMMARY-SPINOFF, EFN-CONFLICTS |
| L4 | RS-TIERS, PRIMARY-CARVEOUTS, CLOP, EFN-CONFLICTS, NOTLITREVIEW |

Tier 1 (attribution / V-SYNTH at sentence level / LEAD-SUMMARYSTYLE) is in
every bundle regardless of loop.
