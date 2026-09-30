# Rule enforcement plan — verify outcomes, not only inputs (2026-09-30)

This plan is written for a session that was not present when it was made.
It says what to build, where, and how to know each part is done.

## Why

Two findings from the code review of 2026-09-30 have one cause.

1. **Every edit to an existing page was saved as a minor edit.** The edit
   call sent `minor=0`. MediaWiki treats `minor` as a presence flag, so any
   value marks the edit minor. Read on 2026-09-30: revid 1377121505
   (Sarah Kidder) returns `minor: true`. The code, the tests and
   `docs/api-etiquette.md` agreed with each other and were all wrong about
   the server. No step reads a saved revision back.
2. **Wikipedia's content and style rules are given to the model as reading
   material, and almost nothing checks the output against them.** The
   operator is the only reviewer of neutrality, weight, tone and most of
   the Manual of Style.

The common cause: rules are stated on the way in and not verified on the
way out. Each work item below adds a check on an outcome.

## Starting state

Verify each of these before you start. If one is false, stop and report.

- The review fixes of 2026-09-30 are committed. Check:
  `grep -n '"notminor"' src/wikipedia.rs` prints one line, and
  `git status --short` prints nothing.
- `cargo test` passes and `cargo clippy --all-targets` is clean.

How the rules reach the model today:

| Layer | Where | Effect |
|---|---|---|
| Policy snapshots | `rules/canonical/*.wikitext` | No code reads them. |
| Distilled rules | `rules/tier1-core.md` (clusters A, B, C and a standing checklist), `rules/cards/*.md` | `rules::build_context_bundle` puts them in `wa analyze` output. Nothing checks compliance. |
| Driver prompts | `prompts/author-findings.md`, `propose.md`, `resolve.md` | Contain craft rules only. No tier-1 text, no cards. These are the prompts behind the `wa serve` buttons. |
| `rules[]` on a finding | `src/session.rs` | Validated as non-empty. Shown as links on the evidence card. |
| Linter | `rules/linter.toml`, `src/checks/linter.rs` | 11 rules. Errors block in the gate. Warnings (`tense-drift`, `see-also-duplication`) are not shown on the web review. |
| Quote and paraphrase gate | `src/checks/gate.rs` | Blocks. Covers verifiability and close paraphrase, not style or neutrality. |
| Operator | Review page, publish approval | The only check on everything else. |

## Rules for the executing session

- Read `CLAUDE.md` and `PLAYBOOK.md` first.
- Toolchain: `cargo fmt` before each commit; `cargo clippy --all-targets`
  must be clean (pedantic lints are denied; functions over 100 lines fail).
- Tests are offline. Use `httpmock`, as `tests/publish.rs` and
  `tests/driver_steps.rs` do. Do not call Wikipedia or the model endpoint
  from a test.
- A change to a request sent to a Wikimedia host must update
  `docs/api-etiquette.md` in the same commit.
- Prompt files are checksum-pinned in `src/driver/prompts.rs`
  (`prompt_checksums_are_pinned`). When you change or add a prompt, update
  the pin and say why in the commit message.
- Other sessions may have uncommitted work in this tree. Stage files by
  explicit path. After each commit, check `git show HEAD --stat`.
- Do not publish to Wikipedia. Work item 1 needs one live check; ask the
  operator to run it.
- One commit per work item.

## Work items

Do them in this order. Items 1 and 2 are independent of items 3 to 6.

### 0. Correct PLAYBOOK steps 7 and 8

`PLAYBOOK.md`, "One iteration", steps 7 and 8 say that comments are left on
the session page and mention a "words you mean" field. The current UI puts
a **Comment** control under each changed block and each evidence card on
the review page (`/sessions/<slug>/review`), and the apply action is the
**Apply comments** button on that page. The session-protocol diagram at the
top says "(session page, one per block)"; correct that too.

Done when: `grep -n "words you mean\|session page" PLAYBOOK.md` shows no
line that describes commenting.

### 1. Read every published revision back

**Goal:** after an edit is saved, compare what Wikipedia recorded with what
the tool intended. A mismatch is shown to the operator and recorded.

**Where:** `src/wikipedia.rs` (`Wikipedia::edit`, `EditOutcome`),
`src/cli.rs` (`publish_core`, `PublishOutcome`), `src/serve.rs`
(`run_publish`).

**Steps:**

1. Add `Wikipedia::verify_revision(revid, expected) -> Vec<String>`. It
   sends one query: `action=query`, `prop=revisions`, `revids=<revid>`,
   `rvprop=ids|flags|comment|user`. It returns one line per mismatch:
   - `minor` is true;
   - `parentid` is not the base revid the edit was pinned to (skip for a
     page creation);
   - the comment does not end with the disclosure suffix
     (`crate::DISCLOSURE_SUFFIX`);
   - the user is not the expected account (house-rules `[operator]`
     `username`; skip when it is not set).
2. Compare the text: fetch it with the existing `wikitext_at_revid` and
   compare to the proposed text with trailing whitespace trimmed on both
   sides. MediaWiki applies pre-save transforms, so report a difference as
   "saved text differs from the proposed text", not as a failure of the
   edit.
3. Call it in `publish_core` when `outcome.created_revision()` is true,
   after the re-pin. The edit is already live, so a mismatch must not turn
   the publish into an error. Add `verification: Vec<String>` to
   `PublishOutcome`.
4. Record the result in the `published` round entry (`rounds.jsonl`
   `detail`: keep the diff URL first, then the mismatch lines).
5. Show it: the CLI prints each line under a `VERIFY` heading. `run_publish`
   appends the lines to its outcome message and includes the word `failed`
   when there is a mismatch, so the web notice uses the error style
   (`notice_html` in `src/serve.rs` styles on that word).
6. A failed read-back query (network error) is itself reported as
   "could not verify the saved revision".

**Tests** (`tests/publish.rs`): a mock revision with `minor: true` yields a
mismatch line; a matching revision yields none; a query error yields the
"could not verify" line; the publish still returns `Ok` in all three.

**Docs:** add the read-back to `docs/api-etiquette.md` ("Edit-path safety")
and to `PLAYBOOK.md` step 10.

**Live check (operator):** publish one edit to an existing userspace page
and confirm the outcome shows no mismatch and the page history shows no
**m** flag.

### 2. Build edit parameters from a typed value

**Goal:** a presence flag such as `minor` cannot be written as `minor=0`.

**Where:** `src/wikipedia.rs`, the `params` vector in `Wikipedia::edit`.

**Steps:**

1. Add a private `EditParams` struct with typed fields: title, text,
   summary, and `base: Base` where `enum Base { Existing(u64), Create }`.
   Its `to_pairs()` method is the only place that names API keys.
   `Existing` emits `baserevid` and `nocreate`; `Create` emits
   `createonly`. It always emits `assert=user` and `notminor=1`. There is
   no field for `minor`.
2. Add a unit test that asserts the exact key set for each `Base` variant.
   A new parameter then needs a deliberate test change.
3. Add a table to `docs/api-etiquette.md`: each parameter, why it is sent,
   and a link to its API documentation.

### 3. Give the driver prompts the tier-1 rules and the loop's cards

**Goal:** the model behind "Write findings", "Draft the edit" and "Apply
comments" receives the same rules text that `wa analyze` prints.

**Where:** `src/rules.rs`, `src/driver/steps.rs`, `prompts/*.md`,
`src/serve.rs` (`run_driver_findings`, `run_driver_propose`,
`resolve_groups`).

**Steps:**

1. In `src/rules.rs`, add `pub fn guidance_for_loop(corpus, loop_id) ->
   Result<String, String>`: tier-1 verbatim, then the cards from
   `cards_for_loop`. A missing card is an error, as in
   `build_context_bundle`. Change `build_context_bundle` to call it, so
   there is one source for this text.
2. Add a `{{guidance}}` slot to each of the three prompt files, under a
   heading such as "Rules you must apply". Fill it with `prompts::render`.
3. Pass the guidance into `author_findings`, `draft_proposal` and
   `resolve_comments` (a field on `FindingsContext`; a parameter on the
   other two). The serve handlers load `RulesCorpus` and the session's
   `entry_loop`.
4. Update the three checksum pins.

**Tests:** in `tests/driver_steps.rs`, the mock model endpoint matches on a
distinctive tier-1 string (`body_includes("Cluster A")`) for each of the
three steps. A corpus with a card removed makes the step fail before any
model call.

**Size check:** tier-1 is about 8 kB and each card about 1.2 kB. Record the
resulting prompt size for loop 3 (seven cards) in the commit message.

### 4. Show linter warnings on the review page

**Goal:** warn-level findings reach the operator.

**Where:** `src/serve.rs` (`inject_comment_ui`), `src/checks/linter.rs`,
`src/cli.rs` (`check_cmd`).

**Steps:**

1. First establish the facts: does `linter::gate` return warn-level
   findings, and does `wa check` print them? Read `run_gate` in
   `src/checks/gate.rs` to see where severity is filtered. State what you
   found in the commit message.
2. On a current (not stale) review, run `linter::gate(base, proposed,
   cfg)`, keep the warnings, and show them in a block under the status
   bar: rule id, description from `rules/linter.toml`, line, detail.
   Warnings never block.
3. Where a warning's line falls inside a rendered block's anchor range
   (the anchor table gives `L<start>…L<end>` per block), show it under
   that block instead.
4. If `wa check` does not print warnings, print them under a `WARNINGS`
   heading after the gate report.

**Tests** (`tests/serve.rs`): a proposed text containing "is now" (the
`tense-drift` sample in `rules/linter.toml`) shows the warning on the
served review and does not block the render.

### 5. Add a rule-review judgment point

**Goal:** a separate model pass reads the drafted text and reports, clause
by clause, where it may break the tier-1 rules. The result is advice shown
beside the diff. It never blocks.

**Where:** new `prompts/review.md`; `src/driver/steps.rs`
(`review_draft`); `src/serve.rs` (route, display); `src/cli.rs`
(`wa review <slug>`); `src/session.rs` (paths).

**Output schema** (one JSON array):

```
[{"clause":"A1","verdict":"concern","span":"<verbatim text from the proposed block>","note":"<one or two sentences>"}]
```

**Validation (same discipline as quotes — model output is untrusted):**

- `clause` must be a clause id that exists in the guidance text. Tier-1
  clause ids have the form `**A1.` at the start of a paragraph; card ids
  are the card slugs. Build the allowed list from the corpus, not by hand.
- `span` must be a verbatim substring of the proposed text of a changed
  block. A span that does not locate rejects the output.
- `verdict` is `ok` or `concern`. A `concern` needs a non-empty `note`.
- One corrective retry, then the step fails (`call_json` already does
  this).

**Steps:**

1. Input to the step: the guidance from item 3, and for each changed block
   its base text, its proposed text and the evidence quotes with sources.
   Take the changed blocks from `render::review_targets` and the anchor
   table, as `bucket_groups` in `src/serve.rs` does.
2. Store the result in `sessions/<slug>/rule-review.json` with the round
   number and a timestamp. Append a round-log entry with phase
   `rule-reviewed`. (`artifact_state` in `src/serve.rs` reacts only to
   `published` and `comments-resolved`, so this phase does not make the
   review stale. Add a test that pins this.)
3. Trigger: a **Check against the rules** button on the review page, and
   `wa review <slug>`. It runs on demand, not on every render (it is a
   paid call).
4. Display: under each block, list its concerns (clause id, note, the span
   highlighted in the note). Style them differently from operator
   comments. Show "No concerns raised" when the list is empty, and "Not
   checked for this round" when there is no result for the current round.
   A result from an earlier round is not shown as current.
5. Do not add the result to the gate.

**Tests:** schema and validation cases in `tests/driver_steps.rs` (unknown
clause, span not in the text, empty note); the served review shows a
concern under the right block; a stale result is labelled as such.

**Docs:** add the step to `PLAYBOOK.md` between Render and Review, and to
the README's description of `wa serve`.

### 6. Mechanical style rules: repair, then borrow

**Part A — repair known misfires.** Each was confirmed by a probe on
2026-09-30. Add a failing test first, then fix.

False passes:
- A whole-page rule that reports only its first match hides a second
  violation behind one that already exists in the base
  (`page-pages-consistency`; by reading, also `italic-mismatch`,
  `lead-body-duplication`, `national-variety-mix`). Emit one finding per
  match.
- `semicolon-prose`: the pre-existing-semicolon guard skips the whole
  line, so a new semicolon added to a line that already had one passes.
  Compare counts, or mask the pre-existing fragment.
- `named-ref-with-pinpoint` cannot match a citation that spans several
  lines, because added-lines rules scan one line at a time.

False blocks:
- `national-variety-mix` matches by suffix: "prize", "size", "seized"
  count as American; "advertising", "crises", "expertise" count as
  British. It also iterates a `HashMap`, so the reported stem can differ
  between the base and proposed scans.
- `italic-mismatch` does a case-sensitive substring match against all
  plain text, headings included (`''Life''` against a `== Life ==`
  heading; `''Time''` against "Times").
- `semicolon-prose` flags semicolons inside a multi-line template, for the
  same one-line-at-a-time reason.

**Part B — survey prior art before adding rules.** Deliverable: a dated
note in `docs/` that compares the rules in `rules/linter.toml` with
[Check Wikipedia](https://en.wikipedia.org/wiki/Wikipedia:WikiProject_Check_Wikipedia)'s
error list and
[AutoWikiBrowser's general fixes](https://en.wikipedia.org/wiki/Wikipedia:AutoWikiBrowser/General_fixes).
For each of our rules: is there a community definition, and does ours
match it? For their rules: which ones apply to text this tool drafts? End
with a short list of rules to add, each with its source definition. No
code in Part B. Fetch those pages with `wm-fetch`, not `WebFetch`.

## Decisions for the operator

The plan assumes the first option in each case. Ask before you change one.

1. **Rule review: advice or gate?** Advice. These are judgment calls, and
   a blocking model verdict would need an override path.
2. **Rule review: on demand or at every render?** On demand.
3. **Rule review: same model as the drafter, or a different one?** Same
   endpoint and model, separate prompt, no shared conversation. A
   different model would be a stronger check; it needs a second endpoint
   configuration in `rules/house-rules.toml`.
4. **Read-back mismatch:** warn loudly and record; do not attempt an
   automatic revert.

## Not in this plan

- Making the gate require a ledger claim for every added sentence. Today
  an edit with no findings and no claims passes if it is lint-clean. That
  changes how every session works and needs its own decision.
- Ellipsis quotes that drop fragments under 8 characters
  (`src/checks/quote_anchor.rs`). The code is ported from SP42; fix it
  there first.
- Checking that a finding's `rules[]` entries name real policy shortcuts.
- A correction for the edits already saved as minor. The flag on a saved
  revision cannot be changed; the operator decides whether to note it on
  the disclosure page.
