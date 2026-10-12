# ADR-0004 — Reuse posture, the LLM-enabled niche, and two scope guards

Status: **Accepted** — owner-ratified 2026-06-02 (walkthrough). Both scope guards (no-CV;
anti-rubber-stamping) stand. **WikiChat call resolved (2026-06-02):** its Stanford-hosted
endpoint is permitted **for experimentation only**; because the code is open (Apache-2.0)
and self-hostable there is no lock-in — re-instantiate it ourselves before it touches any
credibility/production path (see Consequences).
Date: 2026-06-01 (ratified 2026-06-02)

## Context

A broad prior-art survey (`docs/12-prior-art-and-reuse.md`) of LLM and non-LLM
tooling for Wikipedia/Wikidata/Commons + GLAM/knowledge-graph integration. The
consistent finding: the *infrastructure* (reconciliation protocol, batch write,
upload, constraint checking, citation generation, model serving) is mature and
open, while the *unsolved* problem everywhere is **matching/validation that
requires judgment**, which existing tools punt to humans. Two cautionary histories
loom large: **Computer-Aided Tagging** (Google-Vision depicts; shut down 2023,
~1.3M edits reverted) and the **Future Audiences** experiments + **TomWikiAssist**
(community pushback / block). The owner added two explicit steers in discussion.

## Decisions

1. **Reuse infrastructure; build only the judgment brain.** We integrate mature
   tools rather than reinvent them: pywikibot(+`pywikibot-sdc`) as the write path,
   QuickStatements 3.0 / Wikibase REST for batch Wikidata writes, the
   **Reconciliation Service API** (we expose our matcher *as* a reconciliation
   endpoint), Mix'n'match as candidate source + work queue, Citoid for citations,
   `wbcheckconstraints`/ShEx/Recoin/LiftWing as guardrails, Pattypan/OpenRefine/ISA
   as human front-ends, WikiChat as a grounding backend. (Full table in `12 §A`.)

2. **The LLM is applied only at the precision-at-velocity gap.** The reusable
   pipeline is: deterministic ID join → cheap candidate generation → **LLM re-ranks
   & disambiguates with context, emitting a graded judgment + rationale, or abstaining** →
   auto-accept only where **measured ensemble agreement** (not a self-reported score) clears
   its policy threshold / route the rest to human review → write via reused
   infrastructure, constraint-validated first. The LLM's demonstrated edge is
   contextual disambiguation and long-tail/semantic matching that string and
   fixed-label-space ML tools cannot do at both speed and precision.

3. **SCOPE GUARD — text & metadata only; no image parsing, no computer vision.**
   *(Owner steer.)* WikiHarness never reasons over image pixels. Media are matched
   by their **textual surface** — catalog records, descriptions, existing
   `depicts`/SDC, the depicted object's Wikidata item, and the article text — not by
   what they look like. Rationale: pixel understanding is a strictly harder problem
   than text, it is precisely where the ecosystem's worst failure (CAT) occurred,
   and it is not our edge. No multimodal image input is permitted in any tool.
   "Image→article" and "depicts" capabilities are recast as **text→entity** and
   **metadata→article** matching.

4. **SCOPE GUARD — anti-rubber-stamping is a design + measurement requirement.**
   *(Owner steer; the deeper, transferable CAT lesson.)* A human-in-the-loop does
   **not** by itself make low-precision suggestions safe — CAT was human-reviewed and
   still net-negative because plausible suggestions were rubber-stamped at scale,
   and this dynamic applies to *text* matching too. Therefore we encode:
   - **Acceptance is not a success metric; acceptance *correctness* and durability
     are.** High accept-rate with low independent correctness is a red flag, surfaced
     loudly (`11`).
   - **Evidence-forward review UI** — the human sees the located quote / catalog
     record / graph context first, not a bare verdict.
   - **First-class abstention; no fabricated confidence numbers** — "no confident match" /
     "insufficient evidence" is the preferred failure mode (precision over recall). We do
     **not** surface a model-generated confidence/support % (false precision); the honest
     signals are the **graded categorical verdict**, **measured ensemble agreement**, and
     abstention (`03`/`07`).
   - **Diligence honeypots** — planted gold items measure reviewer diligence.
   - **Throttle to genuine review capacity** — rate caps (Add-a-Fact style).

5. **Community-UX guardrails from the field's failures.** Disclosed, reversible,
   single-decision structured-task UX (the Add-a-Link/Image format the community
   already trusts); route not-yet-trusted edit classes to **talk pages / review
   queues** with structured citations (the Add-a-Fact pattern), including
   reliable-source and spam-blocklist checks; position WikiHarness explicitly as a
   **WikiProject AI Cleanup ally** (verification/matching), never a prose generator.

## Consequences

- Most "tools" in `07` become thin LLM-judgment layers over reused services; the
  reuse map and write paths are updated accordingly.
- The GLAM roadmap phase (`09`) leads with **metadata-grounded** reconciliation and
  image→article matching, with image parsing explicitly out of scope.
- The evidence harness (`11`) gains explicit **anti-rubber-stamping metrics**
  (acceptance-correctness, durability, reviewer-diligence honeypots) and named
  benchmarks (ALCE, Citation Needed, EUFCC-340K, Mix'n'match queues, the owner's
  `hard-patrol-problems.md` taxonomy), with ProVe/SIDE/Add-a-Fact as baselines.
- **WikiChat hosted endpoint — resolved (owner, 2026-06-02):** permitted **for
  experimentation only**, where it is helpful (decision 1, `12 §A`). The Principle-6
  posture (`adr/0006`) is preserved two ways: experiments make no credibility claim, and
  the code is genuinely open (Apache-2.0), so if it ever becomes load-bearing we
  **self-host / re-instantiate it** rather than depend on Stanford's server. Guardrails:
  it must **not** sit in any published-evidence (`11`) or production write path while still
  externally hosted, and we **verify self-hostability** (retrieval index + infra) before
  promoting it past experiments. (`13` first flagged this.)

## Non-goals / explicitly excluded

- Any computer-vision / multimodal *image-content* analysis (decision 3).
- Treating human acceptance as proof of quality (decision 4).
- Autonomous prose generation or unattended editing (carried from ADR-0001).
