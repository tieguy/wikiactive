# Wikimedia Evaluation Datasets and Validation Prior Art

*Analysis doc. Status: DRAFT (2026-06-23). Source material for blog post 2 (trust/validation) and for later funder/project docs. Reference figures marked TODO need checking against the primary papers before they're relied on in public writing.*

## Why this doc exists

A commons harness lives or dies on validation, and the hardest validation questions — "is this claim sourced?", "does this source support this claim?" — are eval-shaped, not test-shaped (no deterministic ground truth). The good news is that the Wikimedia ecosystem has a decade of labeled datasets, shared benchmarks, and hard-won methodology to inherit. This doc catalogs that prior art and organizes it into a "ladder" that doubles as an argument: as the questions get more useful, ground truth gets scarcer, and the work shifts from tests toward evals.

## The key concept to inherit: where ground truth comes from

The single most useful idea from the existing Wikimedia ML literature is not a model — it's a vocabulary for the *source* of labeled data, and a warning about trusting any one source too much.

ORES (the long-running quality-control service) was trained two ways:

- **Found traces** — implicit signals already in the data. A damaging edit tends to get reverted, so the revert is treated as a label. Abundant (every revert in project history is one) but noisy: reverts happen for many reasons unrelated to edit quality.
- **Manual labels** — Wikipedians explicitly judging edits through a purpose-built interface (Wiki Labels). Scarce and expensive, but they say what you actually mean.

This is the tests-vs-evals distinction in Wikimedia clothing, and it is the spine of the trust argument.

## The cautionary tale: Wikibench

ORES is being succeeded by **Revert Risk** models (served on Lift Wing), trained purely on trace data — an edit's revert history as ground truth.

When researchers built **Wikibench**, a deliberately community-curated evaluation dataset, and tested both, the older ORES model (trained on explicit human labels) aligned *better* with community judgment than the newer trace-trained Revert Risk model — reversing the result the development team's own trace-based evaluation had shown. (TODO: verify the reported AUC figures — ~0.84 vs ~0.79 — against Kuo et al. 2024 before quoting.)

The lesson, and the reason this belongs at the center of any trust discussion: two reasonable evaluations produced opposite answers, and the deciding factor was *which ground truth you trusted*. Choosing the standard is itself a judgment, not a technicality.

## The ladder

As you climb, questions get more useful and ground truth gets scarcer.

### Rung 1 — "Is this edit damaging?"
Vandalism / edit-quality detection. Bottom rung: trace data is abundant (every revert is a label), the question is near test-shaped.
- ORES edit-quality models (damaging, goodfaith, reverted).
- Wikidata: WDVD (Heindorf et al., 2016) and the WSDM Cup 2017 Wikidata Vandalism Detection task (shared dataset, multiple competing systems).

### Rung 2 — "Does this sentence need a citation?"
- **Citation Needed** (Redi, Fetahu, Morgan, Taraborelli, WWW 2019): a taxonomy of *why* inline citations are required, a large crowdsourced dataset of sentences labeled against it, and models predicting whether (and why) a statement needs a source.
- **Citation Detective** (Chou et al., 2020): applies the Citation Need model at scale to produce a public dataset across English Wikipedia. Notably found Medicine and Biology among the best-sourced topics — relevant to a biomedicine starting point.

### Rung 3 — "Does this citation actually support the claim?"
The hardest rung, and the closest to what a cite-checking harness must do.
- **SIDE** (Petroni et al., *Nature Machine Intelligence* 2023): flags Wikipedia citations unlikely to support their claims and recommends better ones from the web, trained on existing Wikipedia references ("the combined wisdom of thousands of editors"). Uses the SPHERE web corpus and the WAFER test set. For the top ~10% most likely to be flagged unverifiable, crowdworkers preferred SIDE's suggested alternative over the original ~70% of the time.
- Note even here the "ground truth" bottoms out in a crowd of humans expressing a preference — there is no checksum to appeal to at this rung.

**The ladder is the argument.** At the bottom, ground truth is cheap and checks are nearly tests. At the top — does the source support the claim — ground truth thins to contested human judgment. A commons harness earns its keep on the top rung, where no single label source suffices and orchestrated validation (deterministic checks where available, adversarial critique, multi-model voting, self-critique, human-in-the-loop) is the only way to reach real rigor.

## The diff as the atomic unit

A separate strand of prior art validates the instinct that the *edit* (the diff) is the natural unit of verification, and an edit's fate is a form of ground truth.

- **VitaminC** (Schuster, Fisch, Barzilay, NAACL 2021): built from 100,000+ Wikipedia revisions that modify an underlying fact, yielding 400,000+ contrastive claim-evidence pairs that are near-identical except that one supports a claim and the other doesn't. Defines a task for identifying factual revisions. The "edit fate as ground truth" idea made concrete and public (HuggingFace: `tals/vitaminc`).
- **FEVER** (Thorne et al., NAACL 2018): the older, larger fact-verification benchmark most of this work descends from — claims labeled Supported / Refuted / NotEnoughInfo against Wikipedia. The canonical ancestor.

## Umbrella reference

- **"Wikimedia data for AI: a review of Wikimedia datasets for NLP tasks and AI-assisted editing"** (arXiv 2410.08918, 2024) — catalogs essentially all of the above in one place. The one-stop related-work citation; the items above are the ones worth discussing individually.

## Interpretive notes for downstream writing

- **The found-traces/manual-labels distinction + Wikibench** is the rhetorical keystone for a skeptical Wikipedian audience. Lead with it in post 2.
- **SIDE is the closest prior art** to SP42 cite-checking — engage it directly, both as foundation and as the thing being extended toward governance and provenance.
- **SIDE is also a quiet extraction example**: Meta AI, trained on volunteer editors' combined labor, lead author's contact now a startup address (samaya.ai). The OpenStreetMap pattern in miniature. Hold this for the commercial-extraction discussion in a later post, not post 2.

## References (verify all before public use)

- Halfaker & Geiger, "ORES: Lowering Barriers with Participatory Machine Learning in Wikipedia," CSCW 2020.
- Wikimedia Research, Revert Risk models (language-agnostic and multilingual); served via Lift Wing; API on api.wikimedia.org.
- Kuo et al., "Wikibench: Community-Driven Data Curation for AI Evaluation on Wikipedia," CHI 2024 (arXiv 2402.14147). *(verify AUC figures)*
- Redi, Fetahu, Morgan, Taraborelli, "Citation Needed: A Taxonomy and Algorithmic Assessment of Wikipedia's Verifiability," WWW 2019 (arXiv 1902.11116).
- Chou, Gonçalves, Walton, Redi, "Citation Detective: a Public Dataset to Detect Citation Need on Wikipedia," Wiki Workshop 2020.
- Petroni et al., "Improving Wikipedia verifiability with AI," Nature Machine Intelligence 2023 (arXiv 2207.06220). SPHERE corpus; WAFER test set.
- Schuster, Fisch, Barzilay, "Get Your Vitamin C! Robust Fact Verification with Contrastive Evidence," NAACL 2021 (arXiv 2103.08541).
- Thorne, Vlachos, Christodoulopoulos, Mittal, "FEVER: a Large-scale Dataset for Fact Extraction and VERification," NAACL 2018.
- Heindorf, Potthast, Stein, Engels, "Vandalism Detection in Wikidata," CIKM 2016; WSDM Cup 2017 Wikidata Vandalism Detection task.
- "Wikimedia data for AI: a review of Wikimedia datasets for NLP tasks and AI-assisted editing," arXiv 2410.08918, 2024.
