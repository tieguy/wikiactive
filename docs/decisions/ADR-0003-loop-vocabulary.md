# ADR-0003: Loop vocabulary — stages are verbs; records are named after their stages
Status: Accepted (2026-10-01)
Context: The MVP-1 command surface (findings / sweep / check / review / render / poll) predated a settled model of the loop; by 2026-09-30 the words had drifted from the stages they named ("review" meant both a model advice pass and the human act; "sweep" named a stage that is really fetch-or-dispose; "findings" named both the record and the judgment). One word carrying two meanings is how instructions get followed wrongly.
Decision: One word, one meaning, and the words are the loop's stages:
- **assess** — article-side judgment (record: `Assessment`, `AS<n>`, `assessments.json`);
- **fetch** — the source-accessibility stage (inventory + fetch in one invocation);
- **audit** — draft-side examination: the deterministic **gate**, plus the model **diagnosis pass** (`wa audit --llm`); a green audit renders the artifact (render-on-pass);
- **review** — the human act only: reading the artifact and leaving comments;
- **propose**, **resolve**, **publish** — unchanged meanings.
The record/ledger split is two layers: the **ledger** (sources → fetched text → quotes → claims) is the evidence basis, mechanically checkable; **assessments** are editorial judgments about the article, anchored to ledger evidence. The organizing 2×2:
| | mechanical | judgment |
|---|---|---|
| article-side | analyze (bundle, defect candidates) | assess |
| draft-side | audit (gate) | audit `--llm` (diagnosis) |
Alternatives considered:
- Keep the MVP-1 names (familiarity) — rejected: familiarity is sunk cost; the names misdescribed the stages, and the rename is a one-time documented break (single user, early stage, operator-approved).
- Noun-based vocabulary ("assessment-authoring", "source-sweep") — rejected: the loop is a sequence of actions; verbs name the steps the operator and the driver actually take.
- A thesaurus pass keeping "review" for the model pass — rejected: the human act is the one thing the tool must never confuse with automation; "review" belongs to the operator.
Consequences: Retired invocations fail with pointers (`wa findings` → `wa assess`, `wa sweep` → `wa fetch`, `wa check`/`wa review` → `wa audit`, `wa render`/`wa poll` → `wa audit`); the divergence from the archival MVP-1 plan is recorded here, not by editing history. The disposition ladder's prose is deleted under this vocabulary (its ladder was a judgment heuristic without an owner — operator decision 2026-09-30 — not a stage or a mechanism).
Open questions: whether the lavish tty pair ever returns as a review surface (retired in Phase 2; the pin and fixtures remain as the recorded reference).
