# ADR-0008 — Graduating to real writes (test wiki first)

Status: **Accepted** — owner-authorized 2026-06-02. Real writes to `test.wikipedia.org`
are permitted **for testing**; every production wiki stays off-limits until the owner
authorizes that target explicitly (`02 §9`).
Date: 2026-06-02

## Context

M0–M5 built the read/propose spine with the only writer being an in-memory fake and
`dryRun` defaulted on, so "never write to a real wiki" was structurally true. M5's exit
gate ("a page-accurate `{{cite book}}` proposed, confirmed, and committed to a **test
wiki**, with the quote located in OCR") is the first point in the plan that needs a
**real** write. The owner relaxed the standing constraint to allow `test.wikipedia.org`
writes for testing.

A real write is only legitimate because wikiharness is **semi-automated**, not a bot, per
[en:WP:AI agents and the bot policy](https://en.wikipedia.org/wiki/Wikipedia:AI_agents_and_the_bot_policy):
a full draft is shown, the human exercises editorial judgment, the edit is marked
AI-assisted, and one approval is never reused for another action. That posture needs **no
BRFA to test on `test.wikipedia.org`** (which is itself the Wikimedia dev sandbox — volatile
by design, not a content store). Autonomous-agent bot approval is "almost certainly denied"
today; semi-automated is the only viable path, and the architecture already enforces it.

## Decisions

1. **A real `WikiClient` adapter (`MwnWikiClient`) is added**, implementing the existing
   `WikiClient` interface on top of **[`mwn`](https://github.com/siddharthvp/mwn)** — the
   maintained TS "pywikibot for Node" (login/OAuth2, CSRF token fetch+refresh, maxlag, retry,
   query continuation, edit-conflict). `fetchArticle` uses `bot.read`, `applyEdit` uses
   `bot.save` with `baserevid`/`tags`/`nocreate`. The `Mwn` instance is **injected** (the
   structural `MwnBot` interface), so the adapter is unit-tested with a fake bot (hermetic, no
   network) and only the composition root constructs the real `Mwn` (needs owner-supplied
   OAuth2 credentials). It is the project's only code that writes — distinct from the read-only
   `FetchHttpClient`, which still refuses non-GET/HEAD.
   - *Amendment (post-audit):* the original hand-rolled `MediaWikiWriteClient` + its pure
     CSRF/edit `build*/parse*` were reinventing pywikibot; replaced by `mwn`. `mwn` is
     **LGPL-3.0-or-later**, which the owner **explicitly endorses** — copyleft is welcome and
     aligns with the open-distribution posture (`adr/0006`); not a concern. (`nodemw`/BSD
     remains a purely *technical* fallback on maintenance grounds, not a licensing one.) `mwn`
     uses `axios` internally with no fetch-injection seam, so the adapter is tested via the
     injected `bot`, not a network mock. The read path (`get_wikitext`/`get_article` via
     `FetchHttpClient`) is unchanged.

2. **A real write requires ALL of these, enforced structurally (defense in depth):**
   - **Confirmation token** bound to the exact `ProposedEdit` hash (existing, `02 §2`).
   - **`baseRevision` match** — `commit_confirmed_edit` refuses if the article's *current*
     revision ≠ the proposal's `target.baseRevision` (the revision the human reviewed). An
     article that moved under the human is an edit conflict, not a silent rebase. (Fixes
     bug-hunt I8; the fake already rejects a stale `baseRevision` server-side too.)
   - **Write allowlist (the kill switch)** — `ctx.writePolicy.allowedWikis` lists the wiki
     codes a real write may target. It **defaults to empty**, so even with `dryRun=false`
     no write can occur; a test session sets it to exactly `['test']`. The allowlist + an
     empty default IS the global kill switch.
   - **`dryRun=false`** — still the explicit opt-out of the default.
   - **AI-assistance disclosure** in the edit summary + `ai-assisted` tag (policy enforces
     the disclosure at proposal time; the writer carries it through).

3. **`test.wikipedia.org` only, for now.** The adapter maps wiki codes to hosts and the
   allowlist gates the target; production codes are not on any default allowlist. Moving a
   production write path past this requires a *separate* owner authorization per target and
   re-verification of then-current policy (`02` banner) — and, for production, the bot
   process (BRFA/RfC) with evidence (`11`).

## Consequences / non-goals

- The first live test-wiki edit is **not** performed by building this — it additionally
  needs the owner to register an owner-only OAuth2 consumer
  (`meta.wikimedia.org/wiki/Special:OAuthConsumerRegistration`) and supply the credentials
  out-of-band, plus an explicit per-run "go". The build makes the capability exist and be
  tested hermetically; it does not enable it by default.
- No production write path, no autonomous edits, no relaxation of the anti-hallucination,
  identity-invariance, or human-confirmation invariants. This ADR only adds a guarded,
  test-only writer behind the existing gates.
- The write path is the most security-sensitive code in the project; it gets a dedicated
  ADR-0007 stage-end adversarial bug-hunt before it is considered done.
