# 12 — Prior Art & Reuse Strategy

> **Staging — WHAT (converging).** Vision / scope / posture / findings: close to settled, changes deliberately. Architecture & mechanics (the *how*) live in `06`–`10` and stay tentative until base-framework sign-off — see [Staging](../README.md#staging).

A broad-net survey of what already exists (LLM and non-LLM) for improving
Wikipedia / Wikidata / Commons and integrating GLAM + knowledge-graph data, and a
disciplined decision about what to **reuse**, what to **borrow**, and where the
genuine **LLM-enabled niche** is. Sourced from four parallel research passes plus
the owner's own prior surveys in `open-graph-next` (`all-the-opens/docs/
wikidata-quality-research.md`, `wikidata-SIFT/docs/`). Mid-2026 snapshot.

## The thesis (the wedge)

The owner's own landscape survey states it best:

> *"No existing tool is systematically working through this backlog. The few
> deployed quality tools are either academic prototypes that never crossed the
> deployment gap, defensive-only (ORES), or deployed but barely used."*
> — `all-the-opens/docs/wikidata-quality-research.md`

Every lane of research confirmed the same shape:

- The **infrastructure** — reconciliation protocol, batch write, upload, constraint
  checking, citation generation, model serving — is **mature, open, and should be
  reused wholesale.** Rebuilding any of it is wasted effort.
- The **unsolved problem** everywhere is **matching and validation that require
  judgment**: which entity is the right homonym, does this source actually support
  this claim, which image illustrates this article, is this statement factually
  right. Existing tools punt exactly this to humans (Mix'n'match's manual backlog,
  OpenRefine's candidate lists, the constraint-violation reports nobody triages).
- That judgment gap, **at high velocity *and* high precision**, is the LLM-shaped
  niche — and it is mostly unoccupied.

So WikiHarness's design rule: **the LLM is the judgment brain; everything around it
is reused infrastructure.** We plug the LLM in *behind existing facades* (the
Reconciliation Service API, structured-task accept/reject UX, talk-page review
queues) so we inherit ecosystem compatibility and community trust instead of
fighting for them.

## A. Reuse as-is (do NOT reinvent)

| Need | Reuse | Why / notes | License |
|---|---|---|---|
| **Wiki read/write plumbing** | **pywikibot** (+ `pywikibot-sdc` for Structured Data) | Canonical MediaWiki automation; the "hands." Our agent supplies the judgment. | MIT |
| **Wikidata batch writes** | **QuickStatements 3.0** (Wikibase REST API) | Mature batch write path; statements/refs/qualifiers/ranks. Generate QS or call the REST API; never build a writer. | OSS |
| **Entity reconciliation interface** | **Reconciliation Service API** (W3C CG) + `wikidata.reconci.link` | Standard protocol; exact-ID join scoring; type/property filters. **Expose our LLM matcher *as* a reconciliation endpoint** so OpenRefine et al. consume it for free. | BSD / open |
| **Candidate generation & catalog matching** | **Mix'n'match** (4,500+ catalogs) | Pull external-catalog candidates; push confirmed matches back. Its unmatched/preliminary backlog is a ready-made work queue *and* benchmark. (API undocumented — budget reverse-engineering.) | OSS |
| **Citation generation** | **Citoid** | URL/DOI/PMID/ISBN/QID → filled citation template. Backend for `enrich_citation`. | — |
| **Bulk GLAM upload (human front door)** | **Pattypan** (v26.02, Feb 2026) and **OpenRefine + Commons/SDC** | Spreadsheet & reconciliation-driven upload. We *fill* their inputs (normalized metadata, depicts QIDs, captions) rather than replace them. | MIT / BSD |
| **Crowd depicts/caption UX** | **ISA Tool** | Proven campaign HITL UX for SDC tagging; we **pre-seed its cards with ranked candidates**. (Verify maintainer health.) | OSS |
| **Constraint / quality guardrails** | **WikibaseQualityConstraints** (`wbcheckconstraints` API), **ShEx/EntitySchemas**, **Recoin**, **ORES/LiftWing item-quality** | Validate writes *before* submitting; prioritize which items/statements to work on. Don't rebuild constraint logic (the owner's explicit guidance). | open |
| **External-ID exact joins** | VIAF/ISNI/GND/LCNAF/ULAN/ORCID properties | When an authority ID exists, linking is deterministic and beats any fuzzy method — **always try first.** (VIAF had 2024–25 outages; check liveness.) | mixed |
| **Grounded Wikipedia retrieval** | **Stanford WikiChat** hosted endpoint (25 langs) | Reuse as a grounding backend; adopt its "keep only facts grounded in retrieval" discipline. **Hosted endpoint: experiments only — self-host before any credibility/production use (`adr/0004`).** | Apache-2.0 |
| **Model serving / edit-risk signals** | **WMF LiftWing** | Consume edit-quality/revert-risk scores; don't re-derive. | open |
| **Book-scan sourcing, archiving** | Internet Archive Search-Inside / Open Library / Wayback (see `09`) | Already in the roadmap. | open |

**Bibliographic subdomain** (if/when needed): WikiCite + Scholia + Manske's
`Papers` + Author Disambiguator + IA `Fatcat` — identifier-keyed; reuse, don't
rebuild. (Note: `SourceMD` is deprecated.)

## B. Reference designs to study (borrow the pattern)

**Broken out into its own review doc: [`13-reference-designs.md`](13-reference-designs.md)**,
which assesses each system (SIDE/Sphere, WikiChat, Add-a-Fact, Add-a-Link/Image,
WINELL, ProVe, your own SIFT, CheckPlease) for *what to borrow, what to skip, and
whether it's worth bothering with*, with confidence flags. One-line summary: borrow
SIDE's **loop**, Add-a-Fact's **talk-page structured-citation pattern**,
Add-a-Link/Image's **reversible single-decision UX**, and WikiChat's **grounding
discipline**; treat ProVe as the **deployment-gap precedent** to cross; mine WINELL
for ideas only; SIFT is the spine we port, not a reference.

## C. Reusable datasets & benchmarks (for the evidence harness, `11`)

| Asset | Use |
|---|---|
| **Citation Needed** dataset + model ([Redi et al.](https://github.com/mirrys/citation-needed-paper)) | Cheap sentence-level "needs a citation" **triage front-end** before spending LLM/retrieval budget. |
| **ALCE** ([code](https://github.com/princeton-nlp/ALCE)) | Citation **precision/recall** metric — "does the cited source actually support the claim." Adopt as a core eval metric (`05`). |
| **EUFCC-340K** ([arXiv](https://arxiv.org/pdf/2406.02380)) | 340K faceted GLAM metadata records — benchmark for media↔KG matching / depicts. |
| **Sphere** corpus + BM25/DPR indices | Open retrieval **baseline** (frozen 2021; baseline only, not production). |
| **Mix'n'match** unmatched/ambiguous queues | Ready-made **reconciliation benchmark + live work queue**. |
| **SIFT** (500 edits) + **citation-checker** (~189 pairs) labeled sets | Seed eval sets (already in our lineage). |
| **citation-checker `ccs compare`** ([`comparing-benchmark-runs.md`](https://github.com/tieguy/citation-checker-script/blob/main/docs/comparing-benchmark-runs.md)) | The **regression-gate harness to port** (`05`): a two-run control-vs-treatment diff over labeled cells with a flip taxonomy (improvement / regression / lateral), **intersection-only** comparison, and a configurable noise floor. Also supplies the **bias-control reporting discipline** for `11`'s comparative numbers (incl. the on-wiki outcome deltas) — *not* a source for the on-wiki loop's data collection (that is SP42 recent-changes/diff plumbing). |
| **`hard-patrol-problems.md`** (owner's repo) | A taxonomy of 10 categories where humans/models genuinely disagree — our **eval taxonomy** and a realistic ceiling on autonomy. |

### C-bis. Datasets for MVP-3 ("words to watch" rule codification, `adr/0007`)

The MVP-3 candidate rules already have labeled data — and all exploit the same
"use Wikipedia's own structure as ground truth" trick as the citation work
(**distant supervision from inline cleanup templates** + the **NPOV edit history**).

| Rule | Dataset / benchmark | Notes |
|---|---|---|
| **MOS:PEACOCK / puffery** | [**wikipedia-puffery-detection**](https://github.com/abertsch72/wikipedia-puffery-detection) (Bertsch & Bethard, WNUT 2021) — code + data from `{{Puffery}}`/`{{Peacock}}` templates | Distant-supervised positives from templates + an expert-annotated gold test set (~balanced, ~hundreds); **RoBERTa baseline ~0.963 F1**, and crucially **citation features help** (puffery often lacks a cite) — a direct synergy with our citation core. Public baseline to beat. |
| **MOS:WEASEL** | [**WikiWeasel / CoNLL-2010 Shared Task 1**](https://rgai.inf.u-szeged.hu/node/118) — **20,756 sentences** across 4,530 articles, sentence-level uncertain/weasel labels | The largest *purpose-built, sentence-level* benchmark, with published shared-task baselines (precision/recall/F1) to compare against. Plus Ganter & Strube (2009) distant supervision from `{{weasel-word}}` tags. Strongest standalone eval data. |
| **MOS:EDITORIALIZING / broad NPOV** | [**Wiki Neutrality Corpus (WNC)**](https://nlp.stanford.edu/pubs/pryzant2020bias.pdf) (Pryzant 2020) — **~180k** biased→neutral *parallel* sentence pairs; [**Recasens 2013**](https://aclanthology.org/P13-1162/) word-level bias-inducing labels (framing + epistemological); Hube & Fetahu / **WikiBias** | Largest data by far, but **broad** (framing ⊇ peacock-ish; epistemological ⊇ weasel/loaded-verbs), i.e. less of a single crisp rule. Excellent as auxiliary/transfer data and for the "neutralize" rewrite variant. |
| **WP:CLAIM / MOS:SAID (loaded verbs)** | Subset of **Recasens 2013** *epistemological bias* (factives/assertives/implicatives) + WNC | No dedicated standalone set; lives inside the NPOV corpora above. |

**Decision (`adr/0007`):** **peacock first** (public code+data+baseline, citation
synergy), with **weasel as the immediate follow-on enhancer** reusing the same
`detect → vote → propose` pipeline — the reuse demonstrates the framework composes,
and weasel brings the larger WikiWeasel/CoNLL-2010 benchmark. The broad NPOV sets
(WNC, Recasens, WikiBias) are auxiliary/transfer data and the basis for a possible
later "neutralize the sentence" rewrite variant.

## D. The genuine LLM-enabled niche (where we build)

Every existing matcher sits on one side of a sharp tradeoff:
- **String/heuristic** tools (reconci.link, Mix'n'match auto-match, DBpedia
  Spotlight, OpenTapioca) are **fast but low-precision on ambiguous entities** — and
  punt to humans (the visible manual backlogs).
- **Embedding/transformer EL** (ReFinED, BLINK) is **more accurate but a fixed
  label space** — needs index/GPU, degrades zero-shot on the long tail and freshly
  created entities, English-primary, and **cannot weigh free-form contextual
  evidence** ("the composer, not the footballer, because the sentence mentions a
  symphony").

> **Hard scope boundary: text & metadata only — we do not parse image content.**
> Pixel-level understanding is a strictly harder problem than text, it is where the
> ecosystem's worst failure happened (CAT, below), and it is not where our edge is.
> WikiHarness reasons over an image's *textual* surface — its catalog record,
> filename/description, existing `depicts`/SDC, the Wikidata item of the depicted
> object, the article text — **never its pixels.** No computer vision, no multimodal
> image input. An image is matched by what is *written about* it, not by what it
> *looks like*. This makes the GLAM work tractable, testable, and aligned with our
> precision-first posture.

The LLM occupies the quadrant neither can: **high-velocity *and* high-precision
matching with genuine contextual disambiguation.** Capabilities *demonstrated* in
the literature as newly enabled by LLMs (kept within the text/metadata boundary):

1. **Long-tail entity linking/reconciliation** — LLMs close the head-vs-tail gap
   older EL couldn't ([long-tail EL eval](https://arxiv.org/pdf/2505.03473)).
2. **Semantic claim→source verification** (not keyword retrieval) — WikiChat-class
   grounding handles tail/recent facts static pipelines miss ([WikiChat](https://arxiv.org/abs/2305.14292)).
3. **Real-time "is this fact on Wikipedia / where does it belong"** targeting +
   structured citation (WMF Add a Fact, Llama3-70B).
4. **Metadata-grounded `depicts` resolution** — turning a catalog record's *text*
   into specific Wikidata Q-IDs (text→entity), not labels from pixels.
5. **Image→article matching via *metadata*** — pairing a Commons file to an article
   using the file's structured data + the depicted object's Wikidata context + the
   article's text. "Add an Image" only recycles *pre-existing* P18/P373/interwiki
   links and can't serve the unlinked tail; we serve it **without ever looking at
   the image**, only its description and graph neighbors. The least-served
   opportunity in the ecosystem — and reachable purely as a text problem.

**Architecture pattern (applies to reconciliation, depicts, and citation alike):**
`deterministic ID join → cheap candidate generation (reconci.link / Wikidata
search / Mix'n'match / GLAM API) → LLM re-ranks & disambiguates with context,
emitting a graded judgment + rationale (or abstaining) → auto-accept only where measured
ensemble agreement clears threshold, route the rest to human review → write via
QuickStatements/pywikibot-sdc, validated against
constraints/ShEx first.` The LLM is applied **only** at the precision-at-velocity
gap; every other stage is reused infrastructure.

## E. Cautionary tales → hard design guardrails

These are the most valuable findings: the field's failures tell us exactly what
not to do.

1. **Computer-Aided Tagging (CAT) — shut down Sept 2023, ~1.3M edits mass-reverted.**
   WMF's Google-Cloud-Vision depicts-suggester failed on **precision + cultural
   relevance + provenance**, and a human-in-the-loop *did not save it* — low-
   precision suggestions got **rubber-stamped at scale**, net-negative. Two
   distinct lessons:
   → **(a) Stay out of image parsing entirely.** CAT's root problem was reading
   *pixels*. Text is hard enough; images are harder and not our edge. We match media
   by **text/metadata only** (see the scope boundary in §D). This sidesteps CAT's
   failure mode at the source rather than trying to do CV "better."
   → **(b) The rubber-stamping dynamic transfers to *text* and is the deeper
   danger.** Even a perfectly text-based, metadata-grounded suggestion stream will
   be rubber-stamped if review is cheap and suggestions look plausible. So
   anti-rubber-stamping (next item) is a permanent requirement, not a CV-specific
   one.
   ([CAT](https://commons.wikimedia.org/wiki/Commons:Structured_data/Computer-aided_tagging),
   [mass revert](https://commons.wikimedia.org/wiki/User:SchlurcherBot/Mass_revert_computer-aided_tagging))

   **Anti-rubber-stamping (its own principle).** A high *acceptance* rate is **not**
   evidence of quality — CAT had human acceptance and still got reverted. Therefore:
   - **Measure acceptance *correctness*, not acceptance.** The signal that matters is
     whether *accepted* edits are independently correct and *durable* (survive on
     wiki), not how many a reviewer clicked through. Acceptance high + independent
     correctness low is a **red flag**, surfaced loudly (`11`).
   - **Make review expensive enough to be real.** The confirmation UI is
     **evidence-forward**: it shows the located quote / the catalog record / the
     graph context *first*, not a bare yes/no, so the human is reviewing evidence,
     not a verdict.
   - **First-class abstention; no fabricated confidence numbers.** A tool returning "no
     confident match" is the **preferred** failure mode; we tune for precision and reward
     abstention over a plausible-but-wrong guess. We do **not** surface a model
     self-estimated confidence/support % (false precision); the honest signals are the
     graded categorical verdict, measured ensemble agreement, and abstention (`03`/`07`).
   - **Diligence honeypots.** Periodically inject known-bad/known-good gold items
     into the review stream to *measure reviewer diligence*; a reviewer who accepts
     planted errors is rubber-stamping, and we detect it.
   - **Throttle to review capacity.** Never surface more proposals than can be
     genuinely (not reflexively) reviewed; rate caps (Add-a-Fact style).
2. **Add a Fact (community pushback), Citation Needed ext (~0 retention),
   TomWikiAssist (autonomous agent → blocked).** Across all of these, **model
   accuracy was not the bottleneck — community trust, disclosure, reversibility, and
   workflow retention were.**
   → **Guardrails:** disclosed, human-confirmed, reversible, structured-task
   assistance; rate caps; reliable-source/spam-list checks; design for *editor
   adoption inside existing workflows*, not standalone end-user gadgets.
   ([Add a Fact](https://meta.wikimedia.org/wiki/Future_Audiences/Experiment:Add_a_Fact),
   [TomWikiAssist](https://www.thewikipedian.net/p/tomwikiassist-the-ai-agent-who-complained))
3. **WP:LLM (Mar 2026) + WikiProject AI Cleanup.** The community will *delete and
   block* undisclosed AI content.
   → **Guardrail:** position WikiHarness explicitly as a **Cleanup ally** for
   verification/matching/structured tasks — **never a prose generator.** (Aligns
   with `02` and `11`.)

These guardrails are not new constraints — they *are* our existing design
(human-confirm-each-edit, provenance, anti-hallucination, evidence-first). The
prior art independently validates every one of them, and adds four concrete ones to
encode: **(a) abstention as a first-class output** (a tool may return "no
confident match" and that must be the *preferred* failure mode); **(b)
reliable-source / spam-blocklist checks** on any proposed citation source; **(c) a
text/metadata-only scope — no image parsing / no CV** (§D boundary); and **(d)
anti-rubber-stamping by construction** — measure acceptance *correctness* and
durability (not click-through), evidence-forward review UI, **no fabricated confidence
numbers** (graded verdict + measured agreement only), and diligence honeypots (`11`).

## F. Competitive / related projects (situational awareness)

- **Browser-extension neighbors:** Entity Explosion (`fuddl/wd`, the "closest
  existing project" — matches URLs→Wikidata, stops at Wikidata), Wikibase for Web,
  `wd-olid` (Open Library↔Wikidata). The owner's own `all-the-opens`/Jenifesto
  extension differentiates by traversing the **identifier network** (VIAF/GND/GBIF)
  and doing **live identifier validation**.
- **Mismatch Finder** (WMDE) — ingests bulk Wikidata-vs-external discrepancies for
  review ("deployed but barely used") — a natural **output sink** for our
  validation findings.
- **Wikidata Bridge** (edit Wikidata from Wikipedia infoboxes) — *on hiatus since
  Feb 2025*; a UX gap we could inform.

## G. How this updates the plan

- **Tool catalog (`07`)**: add reuse targets — Reconciliation Service API facade,
  QuickStatements 3.0/Wikibase REST write path, `wbcheckconstraints` pre-validation,
  Mix'n'match candidate source, pywikibot-sdc, **abstention** + **source-reliability
  check** as first-class tool behaviors.
- **Roadmap (`07`/`09`)**: the GLAM phase leads with **metadata-grounded depicts +
  image→article relevance matching** (the niche), explicitly designed against the
  CAT failure; reconciliation is exposed as a standard endpoint.
- **Evidence harness (`11`)**: adopt **ALCE** (citation precision/recall),
  **Citation Needed** (triage), **EUFCC-340K** (GLAM), Mix'n'match queues, and
  `hard-patrol-problems.md` taxonomy as named benchmarks; compare against ProVe /
  SIDE / Add-a-Fact baselines.
- **Accepted in** `docs/adr/0004-reuse-and-llm-niche.md` (owner-ratified 2026-06-02).
