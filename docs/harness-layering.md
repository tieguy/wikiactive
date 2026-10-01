# Harness layering — who controls what

The rule this document enforces: **each artifact has exactly one authority, and a
pointer to it lives exactly one layer down from where it is used.** Duplication of
control flow = drift debt. When two documents seem to state the same rule, one of
them is wrong about its own job.

## Two model contexts (do not cross-wire)

wikiactive has two distinct model consumers, and rules must live with the one
that executes them:

- **Drafting context** — the model `wa serve` calls at the three judgment points
  (assess, propose, resolve). Its context is assembled by the tool: the
  versioned, checksum-pinned prompts in `prompts/` plus the analyze bundle
  (tier-1 core, cards, fetch manifest, base-defect candidates). Rules governing
  drafted output — summary rule-attribution, one-logical-edit scoping — belong
  here or in the gate. Never in the coding harness (ADR-0002).
- **Coding context** — a Polytoken session maintaining the tool, rules corpus,
  prompts, and docs. Carries repo facts (CLAUDE.md), plan/execute governance,
  and no drafting procedure. `PLAYBOOK.md` is NOT loaded at runtime by this
  context; it is read on demand as the behavior spec when changing the loop.

A rule that only exists in prose the coding harness reads is a camouflaged
failure: the harness performs compliance in text while `wa` enforces nothing.
The cure is always the same — move the rule into the gate, into `prompts/`, or
onto the enforcement inventory's backlog. ADR-0002 makes this total: every
rule the drafting loop depends on is a mechanism in `wa` or a pinned prompt;
PLAYBOOK carries stages, guarantees, and ownership only.

## Control flow

```
CODING SIDE                                    DRAFTING SIDE (inside wa serve)
CLAUDE.md (facts + triggers)                   prompts/ (versioned, checksum-pinned,
  ├─ wikimedia-api skill ─► wmfetch hook               disclosed on the wiki)
  └─ repo facts                                 analyze bundle (tier-1 core verbatim,
        │                                             cards, fetch manifest, session state)
        ▼                                             │
PLAN FACET ◄── project_vars.yaml plan spec            ▼
  └─ plan-reviewer → handoff (operator)        the three judgment points ─► GATE
        │                                            (ledger, linter, checks)
        ▼                                             │
EXECUTE FACET ◄── finishing-a-plan +                   ▼
  verification-before-completion               review page → operator comments
  └─ final gate: cargo gates →                 → publish on human confirm only
     code-reviewer (+Sonar) → test-analyst
  └─ stage 4: CI/SonarCloud · playbook-audit · effectiveness-review

PLAYBOOK.md — behavior spec of the loop (maintenance artifact):
  paired with code changes (plan spec), audited (playbook-audit),
  classified (docs/playbook-enforcement.md). NOT runtime context for either side.
docs/decisions/ (ADRs) — cited by `ADR-NNNN` ID; superseded, never edited
docs/design-plans/ — committed handoff plans (archival direction)
docs/implementation-plans/ — per-phase task files (just-in-time)
```

## Authority table

| Artifact | Sole authority over | Pointed at by |
|---|---|---|
| CLAUDE.md | repo facts, skill-load triggers | harness (auto-loaded) |
| prompts/ | what the drafting model is told | `wa` only; versioned + checksum-pinned |
| analyze bundle | per-iteration judgment context | `wa analyze` (mechanized) |
| `wa` code | mechanical enforcement of the loop (ADR-0002) | everything defers to it |
| PLAYBOOK.md | higher-level loop spec: stages, guarantees, ownership (ADR-0002, ADR-0003) | plan-facet pairing, playbook-audit |
| docs/playbook-enforcement.md | which playbook rules are code vs advisory | playbook-audit |
| project_vars.yaml (plan spec) | plan shape + process rules | plan facet, plan-reviewer |
| finishing-a-plan skill | execute-side loop + final-gate order | execute facet |
| rules/ corpus | content judgment (cards, tier-1 core) | analyze bundle (embedded) |
| docs/decisions/ | why decisions were made (immutable) | cited by `ADR-NNNN` ID everywhere |
| docs/harness-layering.md | this layering itself | changed only via plan-facet work |

## Product invariants (canonical statement → enforcement → check)

| Invariant | Canonical statement | Enforced by | Checked at |
|---|---|---|---|
| Content changes flow ledger→gate, never model memory | design plan (MVP-1) | `src/checks/gate.rs`, ledger | spec (plans), code-reviewer |
| Quote-anchor locator keeps SP42 provenance headers | source headers | code convention | code-reviewer grep |
| `USER_AGENT` defined once | `src/lib.rs::USER_AGENT` | tests assert | code-reviewer, spec |
| API etiquette internalized (UA, maxlag, assert, backoff) | `docs/api-etiquette.md` | `src/wikipedia.rs`, `src/ledger/net.rs` per house-rules | `tests/integrations.rs` |
| `fixtures/` re-recorded, never hand-edited | CLAUDE.md | convention (hook candidate) | code-reviewer |
| `sessions/` never committed | .gitignore | git | code-reviewer |
| One logical edit per publish unit | prompts/ + gate scope | scope discipline + gate | operator review, test-analyst |
| Model never self-publishes | PLAYBOOK (spec) | tty/app confirmation, `tests/serve.rs` | mechanical |
| Disclosure suffix on every edit | house-rules `[disclosure]` | publish path | gate |
| WMF fetches via wm-fetch only | CLAUDE.md, wikimedia-api skill | `wmfetch-only-for-wikimedia` hook | mechanical |
| Playbook/code pairing | plan spec + this doc | code-reviewer finding | final gate + playbook-audit |
| Drafting rules live in prompts/ or gate, not the harness | ADR-0002 | review discipline | playbook-audit + code-reviewer |
| Detection ≠ enforcement (base scan detects; added-lines gate) | ADR-0005 | `rules/linter.toml` `applies` scope + the bundle's labeled section | linter suite + rules-corpus suite |

New invariants get a row here when they are born; a row with no enforcement
mechanism is a backlog item (same rule as the playbook inventory).
