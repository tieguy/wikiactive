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
                    │      wa render ──► GATE ──► review.html (in-app)
                    │                    │                            │
                    │                 blocked                   leave comments
                    │                    │                     (session page,
                    │              fix + re-render               one per block)
                    │                                                 │
                    │      driver: apply review comments ──► revise ──┘
                    │          (queue → model → splice → resolution note)
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

`--review-since-user [<name>]` (default: house-rules `[operator]` username)
records your last edit to the article; `wa analyze` then embeds the drift
diff since that revision — the re-review-of-own-past-work entry path.

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

0. **Source sweep** (plan-003, before any analysis): `wa sweep inventory
   <slug>` → `wa sweep fetch <slug>` → resolve every `needs_operator`
   source — paste your browser's saved page into `wa serve`'s attach box
   or `wa ledger attach <slug> --source S3 --file capture.html`
   (MHTML/HTML/WARC/text; the browser's "save page" is the capture), or
   sign a disposition (`wa sweep dispose … --disposition
   "dropped: paywall"`). The gate blocks render/publish while any swept
   source is unresolved. This ordering is the lesson of the
   1874→1870→1874 Kidder episode: source-access decisions are made once,
   up front, against the full accessibility picture — not mid-analysis.
1. **Analyze** (step 0, always — the bundle embeds the sweep manifest).
2. **Findings** — author findings against the tier-1 checklist and cards:
   `wa findings add <slug> -` with JSON `{id, wikitext_anchor, rules[],
   evidence: [Q ids], factual_note, proposed_fix, loop}`. Evidence quotes
   MUST already exist in the ledger (register → fetch → quote).
3. **Ledger** — every source: `wa ledger register --url …`, `wa ledger
   fetch --source S1`, `wa ledger archive --source S1` (save-page-now),
   `wa ledger quote --source S1 --text "…"`, `wa ledger claim --prose "…"
   --quotes Q1`. Quotes that don't locate verbatim are rejected at entry.
   **Claim sequencing (live-session lesson):** register a claim only when
   staging the edit whose wikitext carries its prose — the gate assesses
   *every* claim not already in the base wikitext at *each* run
   (`wa check`, render, publish), so a claim registered ahead of its edit
   blocks unrelated publishes on prose that isn't staged yet.
4. **Propose** — edit `sessions/<slug>/proposed.wikitext` with ONE logical
   edit (scoped like a code-review commit).
5. **Check (fail-fast)** — `wa check <slug>` runs the full gate standalone
   against `proposed.wikitext` (no artifact attempt). Iterate here until it
   passes: the report groups reasons as NEEDS ANCHOR (ledger wiring:
   register/fetch/quote) vs HARD BLOCK (revise prose, quote, or lint), each
   with the offending wikitext span.
6. **Render** — `wa render <slug> --round <n> --summary "<one line>"`. The
   gate runs as mandatory pre-flight; blocked = no artifact, all reasons
   listed. On success: review.html (read it in-app on the session page).
7. **Review** — the operator reads the single-column diff with the
   evidence rail and leaves comments on the **session page**, one per
   changed block (plus one per evidence card). Anchoring is exact: the
   form's hidden target IS the block's `wikitext_anchor` from the
   artifact's embedded anchor table — a plain `L..:C..-L..:C..` range
   (new side), `base:`-prefixed (removed wording), or `ledger:Q<n>`
   (evidence). An optional "words you mean" field carries highlighted
   wording (the text-selection substitute).
8. **Resolve** — **driver: apply review comments** (session page) maps
   the OPEN queue entries through the model, grouped by enclosing changed
   block (both sides of a pair go in ONE call), splices the revised
   blocks into `proposed.wikitext` once per group, and writes each
   group's applied/rejected/reply note back to the queue. Or resolve by
   hand: `wa comments resolve <slug> --id K1 --note "…"` (evidence
   comments are about sources, not wikitext — always manual).
9. **Re-render → re-comment** until the operator is satisfied (each
   resolution note says what was done: applied/rejected lines plus the
   model's reply).
10. **Publish** — `wa publish <slug> --summary "<scoped summary>"`. The
    gate re-runs; then the ONE human gate: a `/dev/tty` confirmation. The
    model never self-publishes. On success the base re-pins.
11. **Post-publish** — Earwig compare per new web source; TALK provenance
    note; disclosure-page session-log append; screenshots for the
    disclosure page (operator, manual Commons upload).

### Comment conventions

- **Edit summaries name a rule only when verified.** "per MOS:PROSE" style
  attributions require the rule to actually say what the edit does — check
  against `rules/canonical/` before naming it. wikiactive house rules
  (e.g. the semicolon guard) are NOT Wikipedia rules and must never be
  attributed to the MOS; describe such edits plainly ("split a
  semicolon-joined sentence for readability") or as house style.

- Comments on **changed blocks** target wikitext ranges — fix in
  proposed.wikitext. A **plain** range (`L..:C..-L..:C..`) points into
  the *proposed* wikitext (new side); a **`base:`-prefixed** range
  (`base:L..:C..-L..:C..`) points into the *base* wikitext (old side —
  the removed wording, including pure deletions). When acting on a
  `base:` anchor, the resolution quotes the base span.
- Comments on **evidence cards** (`ledger:Q<n>`) are about the
  source/quote, not the prose: swap sources, adjust quotes (re-verify!),
  or note the dispute. They resolve manually — the driver never edits
  wikitext on their say-so.
- The queue (`sessions/<slug>/comments.jsonl`) is the record: every
  resolution persists its note there (applied/rejected lines + the
  model's reply); the session page renders that, not in-memory state.

### Publish gate invariant

`wa publish` re-runs the same gate as render (AC.11) and then requires an
interactive tty confirmation. If either fails: nothing is written. The
edit summary must be non-empty; the disclosure suffix is appended
mechanically (`LLM-Disclosure: U:LuisVilla/wikiactive`).

## Driver mode — `wa serve` (plan-003 Phase B, plan-004)

The loop, self-served in the browser, with the model called at exactly
the three judgment points (findings authoring, proposal drafting,
comment resolution — `prompts/` is the versioned, checksum-pinned
prompt set; the disclosure page's "exact code including model prompts"
promise is mechanical). Everything else — loop control, the ledger, the
gate, the review comments, the publish confirmation — is deterministic
Rust. One process, zero external server lifecycles (plan-004 cut the
lavish interlink out of the default loop; see the addendum's decision
record).

```
./target/debug/wa serve            # loopback only (default)
./target/debug/wa serve --tsnet    # thin-client: bind the tailnet only
```

Protocol: init the session CLI-side (`wa session init …`), then in the
console — sweep fetch + resolve (dispositions, pasted captures), **driver:
author findings**, review what the model proposed against the manifest,
**driver: draft proposal**, **render review artifact** (in-app: the
session page links it, no external review server), leave comments in the
session page's **Review comments** section (one per changed block + one
per evidence card; the queue is `sessions/<slug>/comments.jsonl`), then
**driver: apply review comments** (judgment point 3: open comments →
model, grouped per changed block → spliced revisions + per-group notes),
re-render, and finally **start publish** and approve/decline the pending
confirmation, which shows the exact prompt. The whole loop runs in this
one process — no Node, no review-server lifecycle.

Invariants preserved: no auto-publish (the edit posts only on the
explicit approve click — pinned by tests/serve.rs), the same gate runs
at render and publish, and `BundledConsent` backs only the
disclosure-log upsert bundled into the one yes, never the article edit.
Credentials: `ZAI_API_KEY` in the env; endpoint and model in
`rules/house-rules.toml [zai]` (fork config, not shell exports); wiki
OAuth as before.

## End-of-session artifacts

- **TALK provenance note** — offer source scans on request; summarize what
  was checked. **Operator decision 2026-09-29 (Kidder close-out):** a
  dedicated TALK note is usually redundant to what the diffs and the
  disclosure log already show — default to skipping it; write one only
  when a talk-page audience genuinely needs the reasoning (e.g. a
  contested fact).
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

- The revisions registry embedded in the artifact keeps every round; the
  on-page legend was a lavish feature (legacy CLI path) and listed at
  most 6 rounds — the registry itself is unbounded.
- Wikitext anchors are line-based (`L..:C..`); col are char columns.
- The linter is regex-level; no `<nowiki>` handling, refs spanning lines
  attribute to the opening line.
- Comment fidelity is block-level, not text-selection: mitigated by the
  "words you mean" field and the resolution's applied/rejected + reply
  notes (acceptable for a single-reviewer tool — operator's call,
  plan-004).
- Block anchors are single-line ranges: the driver's revised block
  splices by the anchor's line, so a multi-line block (multi-paragraph
  pair, table) is resolved and spliced at its anchor line, not over its
  full extent.
- Legacy CLI: `wa render` (lavish open) and `wa poll` still work for the
  tty path, but the default loop and the app UI no longer use them.

## Per-loop rule packs (what `wa analyze` loads)

| Loop | Cards |
|---|---|
| L1 | LEADCITE |
| L2 | RS-TIERS, CLOP, PRIMARY-CARVEOUTS |
| L3 | UNDUE, NOTLITREVIEW, PROSELINE, RECENTISM, LEADCITE, SUMMARY-SPINOFF, EFN-CONFLICTS |
| L4 | RS-TIERS, PRIMARY-CARVEOUTS, CLOP, EFN-CONFLICTS, NOTLITREVIEW |

Tier 1 (attribution / V-SYNTH at sentence level / LEAD-SUMMARYSTYLE) is in
every bundle regardless of loop.
