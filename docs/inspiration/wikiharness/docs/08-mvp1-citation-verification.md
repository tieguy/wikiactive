# 08 — MVP-1: Interactive Citation Verification & Enhancement

> **Staging — HOW (tentative).** Architecture, contracts, and sequencing here are **exploratory, not locked** — a current best guess, revisable freely until M0 (base framework now settled — `adr/0002`, accepted 2026-06-02). The settled *what* lives in `00`–`02`/`11`. See [Staging](../README.md#staging). *(That MVP-1 is the chosen testbed is settled; its spec/mechanics here are not.)*

The architectural testbed. It proves the entire spine — fetch → analyze →
propose → human-confirm → assisted edit — on the most crisply testable, highest-
value, lowest-controversy improvement: **making citations actually support their
claims, and improving them where they don't.**

It is a TypeScript evolution of `citation-checker-script`, but transformed from a
**static report** into an **interactive, human-in-the-loop enhancement workflow**,
delivered **API-first** with both a CLI and a web client.

## User story

> As an editor, I point WikiHarness at an article. It walks the citations, and for
> each one tells me whether the cited source actually supports the claim. Where it
> finds a problem — unsupported claim, dead link, bare URL, malformed cite,
> better source available — it *proposes a specific fix*, shows me the diff, the
> evidence, and its reasoning, and lets me **approve, edit, or reject** it. On
> approval it makes the edit on my behalf (assisted; one at a time). I can do all
> of this from the terminal *or* a web page.

## Scope (in / out)

**In:**
- Verify each citation's claim↔source support (4-verdict scale).
- For problems, propose one or more enhancements:
  - bare URL → full citation (Citoid), 
  - malformed cite → repaired cite,
  - live/dying URL → add an **existing** Wayback `archive-url` if one is available
    (read-only; MVP-1 does **not** trigger Save-Page-Now — see Out),
  - unsupported/weak claim → *flag* and, where possible, propose a better
    existing source (MVP-2 extends this to book scans).
- Human confirms each `ProposedEdit`; assisted commit via OAuth2.
- Full provenance + disclosed edit summaries + audit ledger.

**Out (MVP-1):**
- Writing new prose, rewording claims, or *removing* content (we flag, we don't
  delete).
- Finding brand-new sources from scratch beyond Citoid/CrossRef (book-scan
  sourcing is MVP-2).
- *Creating* new web archives — Save-Page-Now / `archive_url` writes to the
  Internet Archive. MVP-1 reads existing Wayback snapshots only; archiving live
  URLs is deferred post-MVP-1. (So MVP-1's only external **write** is the
  human-confirmed wiki edit.)
- Anything autonomous.

## The workflow as a state machine (durable, resumable)

```
            ┌────────────┐
 invoke ───▶│  LOADING   │  get_article → list citations (extract_claim x N)
            └─────┬──────┘
                  ▼
            ┌────────────┐   per citation, possibly in parallel (≤3 conc.)
            │ ANALYZING  │   resolve_citation_url → fetch_source → verify_citation
            └─────┬──────┘   (anti-hallucination: quote must locate in source)
                  ▼
            ┌────────────┐   for problems: enrich/repair/archive → ProposedEdit(s)
            │ PROPOSING  │   evaluate_policy(proposedEdit) must pass
            └─────┬──────┘
                  ▼
   ┌─────────────────────────────┐   ◀── PAUSE / durable checkpoint
   │ AWAITING_HUMAN_CONFIRMATION │       surfaces diff+rationale+provenance
   └───┬────────┬───────────┬────┘       resumes only on a confirmation token
   approve   edit-approve  reject
       ▼        ▼            ▼
            ┌────────────┐
            │ COMMITTING │   commit_confirmed_edit (assisted, OAuth2)
            └─────┬──────┘
                  ▼
            ┌────────────┐
            │   DONE     │   audit-ledger append; next citation
            └────────────┘
```

`AWAITING_HUMAN_CONFIRMATION` must be a **durable checkpoint**: a web user can
close the tab and resume; a CLI user can `^C` and resume. This is the central
test of the human-in-the-loop machinery (`04` Tier-4) and a primary input to the
base-framework choice (`docs/adr/0002`, criterion D).

## API-first design (one core, two clients)

The workflow is exposed as an HTTP API with streaming; the CLI and web UI are
**thin clients of the same endpoints**. Indicative surface (to be finalized with
the base framework):

```
POST   /sessions                      { article, options } → sessionId
GET    /sessions/:id/events           SSE stream of state + proposals (resumable)
GET    /sessions/:id/proposals        list pending ProposedEdits (+ provenance)
POST   /sessions/:id/proposals/:pid   { decision: approve|edit|reject, editedText?, confirmationToken }
POST   /sessions/:id/resume           resume after reload
GET    /sessions/:id/audit            provenance/audit ledger for the session
```

- **Streaming**: SSE (or WebSocket) so the web UI shows progress live; the CLI
  consumes the same stream.
- **Durability**: session state + checkpoints persisted (`Storage` edge) so
  resume works across reloads/restarts.
- **Auth**: OAuth2 tokens held **server-side only**; the browser never sees them.
- **Contract tests** (`04` Tier-4) run against this API and therefore validate
  *both* clients at once.

## CLI UX (sketch)

```
$ wikiharness verify "Albert Einstein" --wiki en --interactive
✓ loaded (rev 1234567), 142 citations
[12/142] Claim: "...received the 1921 Nobel Prize in Physics..."
  Source: nobelprize.org/...            Verdict: SUPPORTED (94)  "…awarded the 1921…"
[13/142] Claim: "...patented a refrigerator in 1926..."   ⚠ NOT_SUPPORTED (22)
  Proposed fix ▸ replace bare URL with {{cite web}} + add archive-url
  ──────── diff ────────
  - <ref>https://example.org/einstein-fridge</ref>
  + <ref>{{cite web |url=… |title=… |access-date=2026-06-01 |archive-url=… }}</ref>
  Evidence: fetched example.org (hash a1b2…), quote located at offset 4120
  [a]pprove  [e]dit  [r]eject  [s]kip  [?]why ▸
```

## Web UX (sketch)

A single-page client of the same API: a citation list with verdict chips, a
detail pane showing the rendered diff, the located evidence quote highlighted in
the fetched source, the provenance record, and approve/edit/reject controls.
"Edit" opens the proposed wikitext for human modification before approval.

## Reused components (TypeScript ports)

| MVP-1 piece | Ported from | Notes |
|---|---|---|
| claim extraction | citation-checker `core/claim.js` | "between adjacent citations" algorithm |
| URL resolution | citation-checker `core/urls.js` | live-over-archive, sfn, page numbers |
| verify prompt | citation-checker `core/prompts.js` | 9 few-shots, source-only gate |
| verdict parsing | citation-checker `core/parsing.js` + `verdicts.js` | JSON/fenced/markdown recovery |
| multi-provider model calls | citation-checker `core/providers.js` | becomes the `ModelClient` edge |
| source fetch/extract | SIFT `tool_executor.py` web_fetch | trafilatura-equivalent + PDF page |
| citation enrich | Citoid REST | new edge |
| archive lookup (read-only) | Wayback CDX / Availability | new edge — MVP-1 reads existing snapshots; Save-Page-Now deferred |
| edit commit + OAuth2 | SP42 `action_executor.rs`/`oauth.rs` | ported plumbing |

## Definition of done for MVP-1

- All MVP-1 tools unit-tested (Tier 1–2) with fixtures; coverage ≥ 90% on
  core/tools.
- Headless agent-loop test (Tier 3) drives a full article with a scripted model.
- API contract tests (Tier 4): approve / edit / reject / resume-after-reload /
  no-write-without-token.
- Anti-hallucination gate (`05`) = **0** non-locatable supporting quotes on the
  labeled eval set; verdict accuracy ≥ baseline of current citation-checker.
- A real editor completes an end-to-end assisted edit on `test.wikipedia.org`
  from both the CLI and the web UI, with provenance recorded in the audit ledger.
- No write path to production enabled (kill-switch on) until explicitly graduated.
