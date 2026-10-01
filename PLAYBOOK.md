# PLAYBOOK — the wikiactive loop, as stages and guarantees

This is the higher-level spec of the loop: its stages, the guarantee each
stage makes, and who acts in it (ADR-0002). Every specific rule the loop
depends on is a mechanism in `wa` — a gate reason, an entry check, a
command refusal, a linter scope — or a rule in the versioned,
checksum-pinned prompts the drafting model receives. A drafting session
that works only from the tool and its prompts follows every rule; this
document does not carry drafting rules. Which rule is enforced by what:
`docs/playbook-enforcement.md`.

## The loop (states, transitions, who acts)

```
             ┌──────────────────────── the operator owns the loop ────────────────────────┐
             │                                                                            │
 SESSION ─► FETCH ─► ANALYZE ─► ASSESS ─► PROPOSE ─► AUDIT ── green ──► artifact ─► REVIEW
 (init       (tool +      (tool)     (model)     (model)    │                (rendered   (human)
  pins base)  operator                │          │         blocked              by the     │
              resolves)               │          │         all reasons          audit)     │ leave
                                       │          │         nothing written               │ comments
                                       │          │                │                       ▼
                                       │          │                └─► fix ledger/draft  RESOLVE (model)
                                       │          │                                       │
                                       └─► wa analyze re-runs when state moved ◄──────────┘
                                                                                  (then AUDIT again)
 REVIEW satisfied ─► PUBLISH (gate re-run + the one human confirmation) ─► re-pin base
                                                                                        │
                                                              POST-PUBLISH: disclosure log;
                                                              Earwig deferred; TALK per ADR-0001;
                                                              screenshots manual
```

- **Tool acts** (`wa`): init, fetch, analyze, the audit gate, render-on-pass,
  publish, the post-publish read-back. Deterministic Rust; every refusal names
  its reason.
- **Model acts** at exactly three judgment points with pinned prompts:
  assess (`prompts/assess.md`), propose (`prompts/propose.md`), resolve
  (`prompts/resolve.md`) — plus the never-blocking diagnosis pass
  (`prompts/review.md`, `wa audit --llm`).
- **Human acts**: review (comments on the artifact) and the publish
  confirmation. The word "review" belongs to the operator only (ADR-0003).

## Stages, guarantees, ownership

### Session init — `wa session init --article <t> --entry-loop <n>`
Guarantees: the base revid is pinned (a moved base aborts publish, never
silently overwrites); an existing session is never erased. Owner: operator.
`--review-since-user` records the operator's last edit; analyze embeds the
drift diff since.

### Fetch — `wa fetch <slug>` (inventory + fetch in one invocation)
Guarantee: the accessibility of every cited source is decided up front, against
the full picture — every source ends the stage with text, an operator capture,
or a signed disposition (`wa fetch dispose`, `wa ledger attach`, or the serve
console). Unresolved sources refuse assessment and block audit/publish.
Owner: tool + operator (captures and dispositions are the operator's to sign).
The ordering is the lesson of the 1874→1870→1874 Kidder episode.

### Analyze — `wa analyze <slug>`
Guarantee: the iteration's judgment context is mechanically assembled — article
state, tier-1 core verbatim, this loop's cards, fetch manifest, session
summary, and base-article defect candidates (detection only, ADR-0005). A
missing corpus fails loudly. Analyze must be fresh for the iteration being
assessed: the assess entry checks refuse a stale bundle. Owner: tool.

### Assess — `wa assess add <slug> -` / the serve Assess action (model)
Guarantee: every admitted assessment is schema-valid, cites only ledger quotes
that exist, rides a fresh analyze, and rides a resolved fetch stage; bypasses
are explicit flags recorded in the round log. When the fetch summary
shows sources without fetched text, unverifiable claims are flagged
("cannot verify from available sources"), never proposed for removal —
absence in reachable evidence is not unsupportedness (ADR-0002's
prompt-carried rule layer). Assessments look backward at the
article (ADR-0003's two-layer split: the ledger is the evidence basis, the
assessment the editorial judgment — implicitly typed via `rules[]`, tentative
by construction with revisit triggers in ADR-0004). Owner: model drafts, tool
admits.

### Propose (model + operator)
Guarantee: one logical edit per publish unit, scoped like a code-review
commit. Owner: model drafts (`prompts/propose.md`), operator checks.

### Audit — `wa audit <slug> [--llm|--no-llm] [--summary "…"]`
Guarantee: the deterministic gate (ledger wiring, linter on drafted lines,
paraphrase, anchors, claim staging, fetch completeness, summary rule
resolution) is the whole decision — green renders the round artifact
(render-on-pass), blocked lists every reason and writes nothing. The LLM
diagnosis pass is a rider that never decides and never blocks; its default is
fork config (`[audit] llm_pass`). Owner: tool (gate), model (diagnosis
only), operator (reads the artifact).

### Review — the human act
Guarantee: both decisions are explicit — **Reject this edit** (beside
Publish) restores the draft to the article text, records an `aborted`
round entry, declines any parked publish confirmation, and stales the
artifact under the rejected banner: the operator's "no" carries the same
safety guarantees as the publish confirmation. Comments anchor exactly (the form's hidden target is the block's
wikitext anchor from the artifact's embedded anchor table); the queue
(`comments.jsonl`) is the durable record. Owner: operator. The word is the
operator's.

### Resolve — the serve **Process comments** action (model) or `wa comments resolve`
Guarantee: the open queue entries map through the model grouped by enclosing
changed block, splice once per group, and write each group's
applied/rejected/reply note back to the queue; the next audit starts the next
round. Evidence-card comments stay manual (they are about sources, not
wikitext). Owner: model drafts revisions, tool splices and records.

### Publish — `wa publish <slug> --summary "…"`
Guarantee: the gate re-runs, the summary resolves its rule attributions, then
the ONE human gate (tty or in-app approval) — the model never self-publishes.
On success the base re-pins and the saved revision is read back (mismatches
print under VERIFY and land in the round log). Owner: operator confirms.

### Post-publish
Disclosure-page log append (idempotent per session); Earwig compare deferred
(backlog); TALK note default-skip per ADR-0001; screenshots manual.

## Triage — selecting the entry loop (at init)

- **L1 Rescue** — no structure, no lead, no citations: minimum scaffold, resist
  doing more.
- **L2 Mine existing sources** — verify claims against existing sources
  (claim ↔ quote), extract more, fix formats, remove uncited material.
- **L3 GA-style pass / re-review of own prior work** (the bundle carries the
  drift diff) — substantive criteria first, stylistic later.
- **L4 New sources, one at a time** — each publish unit is one source →
  integrated content; conflicts become `{{efn}}` notes, never silent
  adjudication.
- **L5 Wikidata writeback** — deferred (schema hooks only).

## Driver mode — `wa serve`

The loop, self-served in the browser, one process. The session page top to
bottom is the stage order: **Sources** (run fetch, sign dispositions, attach
captures), **Draft** (**Assess**, then **Draft the edit** — both call the
drafting model through the pinned prompts), **Audit** (the form: what changed
this round + the LLM diagnosis toggle; the round is computed, rendering is a
consequence), the in-app review artifact with block-anchored comments,
**Process comments**, and **Publish this edit** behind the explicit approval
block. No auto-publish, the same gate at audit and publish, `BundledConsent`
backs only the disclosure-log upsert. Credentials: `ZAI_API_KEY` in the env;
endpoint/model in `rules/house-rules.toml [zai]`; audit default in `[audit]`.

```
./target/debug/wa serve            # loopback only (default)
./target/debug/wa serve --tsnet    # thin-client: bind the tailnet only
```

## End-of-session artifacts

- **Disclosure-page log append** — one entry per article session (lands when
  that session's publishing completes): article, date, drafting model +
  version, tool code revision, per-edit diff links; idempotent per session id.
- **Screenshots** — capture the live review artifact; Commons upload per the
  README's licensing note. The evidence-rail round is the canonical
  demonstration.
- **Earwig post-checks** — deferred (backlog: a post-publish command).

## Guards vs defects (scoping)

Linter rules like the semicolon ban are **model-quirk guards**: they gate
every line *this tool drafts* (`applies = "added-lines"` in `rules/linter.toml`, hard block) and are never
reported as pre-existing article defects. The analyze bundle's defect
candidates invert the telescope honestly: detection of base-article defects is
labeled detection, feeds assessment judgment, and can never originate a gate
reason (ADR-0005).

## Known limitations (by design)

- The revisions registry embedded in the artifact keeps every round,
  unbounded.
- Wikitext anchors are line-based (`L..:C..`); columns are char columns.
- The linter is regex-level; no `<nowiki>` handling; refs spanning lines
  attribute to the opening line.
- Comment fidelity is block-level, not text-selection; the resolution notes
  carry what was done.
- Block anchors are single-line ranges: multi-line blocks splice at their
  anchor line.
- The lavish tty pair (`wa render`/`wa poll`) is retired; a green audit
  renders (ADR-0003). The lavish pin and fixtures remain as the recorded
  reference.

## Per-loop rule packs (what `wa analyze` loads)

| Loop | Cards |
|---|---|
| L1 | LEADCITE |
| L2 | RS-TIERS, CLOP, PRIMARY-CARVEOUTS |
| L3 | UNDUE, NOTLITREVIEW, PROSELINE, RECENTISM, LEADCITE, SUMMARY-SPINOFF, EFN-CONFLICTS |
| L4 | RS-TIERS, PRIMARY-CARVEOUTS, CLOP, EFN-CONFLICTS, NOTLITREVIEW |

Tier 1 (attribution / V-SYNTH at sentence level / LEAD-SUMMARYSTYLE) is in
every bundle regardless of loop.
