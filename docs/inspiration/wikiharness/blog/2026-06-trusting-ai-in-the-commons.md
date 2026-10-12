---
title: "Why Trusting AI in the Commons Is Harder — and What Software Already Knows"
date: 2026-06-23
description: "Trusting machine-produced work in the knowledge commons is harder than in software—there's no failing test for a poorly sourced claim—but software has spent decades getting good at exactly this discipline. Second in a series on harnesses for the open knowledge commons."
tags: [llm, wikipedia, open-source, harness, open-knowledge, commons, validation, trust]
draft: true
# source: TBD — unpublished draft; set published URL on lu.is before release
---

# Trusting AI For The Commons: Learning from Software

*Second in a series on harnesses for the open knowledge commons. The first post defined what a harness is. This one is about the component where a commons harness will earn or lose credibility: validation.* 

## The key question
For Wikipedians "how can we trust this?" is the whole question. So it is worth a post on its own to get specific on how harnesses _might_ help with trust, if they're designed and built well.

## Two true things that pull in opposite directions

Two key things about trusting machine-produced work in the knowledge commons  point in opposite directions.

The first: building trust in any knowledge work, including open knowledge, is **harder** here than in software. When a program is wrong, a test can often say so unambiguously — the function returns the wrong number, the build breaks. When a *claim* is wrong, or poorly sourced, or subtly slanted, there is usually no equivalent of a failing test. "Is this sentence adequately sourced?" has no [unit test](https://en.wikipedia.org/wiki/Unit_testing).

The second: software has spent **decades** getting better at exactly the discipline we now need — trusting work produced by a system so complex humans can't fully predict it or understand it. It is imperfect, but incredibly rich. Knowledge work should start borrowing from the best of that immediately, because when implemented well, it will help our knowledge ecosystems become more robust in a number of ways.

The job of this post is to hold both of these points: explaining what I think we can borrow from software; then discuss what software hasn't reached because it is a different domain from the knowledge commons. Once we think through those tensions, I'll have (hopefully) made the sharpest case for *harnesses over prompting*: a harness is where verification discipline lives, and prompting alone throws most of it away.

## Borrow the solved part

Software engineering has a mature answer to "how do you trust code a machine (or a stranger, or a past version of yourself) produced?" The newest and best work on AI-assisted coding (sometimes called "harness engineering" or "loop engineering") takes that discipline, and reimplements it at speed and volume. (For a good, long, recent read, here's [one from Google](https://www.kaggle.com/whitepaper-the-new-SDLC-with-vibe-coding) and [one from OpenAI](https://openai.com/index/how-agents-are-transforming-work/).) 

A few pieces of this discipline are worth naming:

**Everything lives in version control.** The reason engineers can change code fearlessly — rewrite, refactor, delete — is that every change is recorded, reviewable, and reversible; nothing is ever truly lost. Wikipedia knows this in its bones, via [Be Bold](https://en.wikipedia.org/wiki/Wikipedia:Be_bold). Git gave software the same easy and fast safety net that page history gives Wikipedia, and as a result much of the past decade+ of software development has been about putting *as much as possible* into git.

**Tests** check deterministic behavior. Given this input, the output is exactly that. Pass or fail, no judgment required. Software has been formalizing the process of testing (often as "test-driven development") for a long time.

**Evaluations** check the parts that *aren't* deterministic. Is this search result relevant to the query? Is this translation faithful to the original? Did the spam filter catch the junk without burying real mail? These are judgments about quality, not binaries about correctness, which makes them messy. Software has tackled this in many ways under names like acceptance testing and benchmarking; machine learning has called it model evaluation and now often uses the clipped shorthand of "evals."

**Continuous integration and regression suites** run those checks automatically, over and over, so you know when version N+1 quietly broke something version N got right. Doing this religiously has become a key part of modern software engineering.

If I had to compress that discipline into a single line: *trust processes — and make the processes shared, automated, and inspectable.* 

## Prompt-centric LLM usage kills process

"Trusting process" is exactly what chat-style prompting alone can't do. A "skill" — procedural knowledge you load into the model's context, like a carefully written instruction file — can *instruct* the model to check its own work. But "ask a programmer to check their own work" is exactly what software has been struggling valiantly to get away from, by building testing tools and frameworks instead. 

Just chatting fails all of the qualities that software has been working towards. It isn't *shared*: the check lives in one private context and vanishes when the window closes. Is isn't *automated*, in the most important sense: it runs only if and when the model agrees. It isn't *inspectable*, because it leaves no trail anyone else can see, re-run, or argue with. 

As I said in the first post, prompting is a rocket engine bolted to a surfboard. The harness is the car — with brakes, and a dashboard, and a maintenance log.

## The harder half: where ground truth comes from

Software's process leans on a luxury the commons often lacks: a ground truth to check against. A test knows the right answer; a contested claim's sourcing rarely has one so clean. So the borrowed discipline only carries us so far — which is the harder half this post promised at the outset. None of this is wholly new to open knowledge: scholarly peer review reached for part of the same instinct long ago — put a claim before independent checkers instead of trusting the author — though without anything automated, and with little that was inspectable after the fact. But this is not a discipline the whole knowledge commons shares, and the place that has thought hardest about the ground-truth gap in particular is the Wikimedia world. What its research community offers is less a model to copy than a hard-won vocabulary for *where ground truth comes from* — and a warning about trusting it too much.

Wikipedia's [ORES service](https://dl.acm.org/doi/10.1145/3415219) — for years the quality-control model behind much of the project's anti-vandalism tooling — was trained two different ways, and the distinction is exactly the found-traces-versus-judgment one this post is built on. Some of its training data came from **"found traces"**: a damaging edit tends to get reverted, so the revert is treated as an implicit signal that the edit was bad. Other training data came from **manual labels**: actual Wikipedians judging edits through a purpose-built labeling interface. Cheap-but-noisy implicit signal versus expensive gold-standard judgment. Found traces are abundant — every revert in the project's history is one — but a revert can happen for many reasons that have nothing to do with the edit's quality. Manual labels are scarce but say what you actually mean.

This is not an academic distinction, and there's a recent result that should give anyone pause. ORES is now being succeeded by newer ["Revert Risk" models](https://meta.wikimedia.org/wiki/Machine_learning_models/Production/Language-agnostic_revert_risk), trained purely on trace data — an edit's revert history as ground truth. When researchers built **[Wikibench](https://arxiv.org/abs/2402.14147)**, a deliberately *community-curated* evaluation dataset, and tested both, the older ORES model — the one trained on explicit human labels — aligned *better* with community judgment than the newer trace-trained one, reversing the result the development team's own trace-based evaluation had shown.

Sit with that. Two reasonable evaluations, two opposite answers, and the thing that decided it was *which ground truth you trusted*. For a post about trust, there is no cleaner cautionary tale: validation is only as good as the standard you measure against, and choosing that standard is itself a judgment, not a technicality.

## The ladder: where ground truth runs out

Once you have the found-traces-vs-manual-labels distinction in hand, a decade of Wikimedia verification work lines up as a ladder. As you climb it, the questions get more useful — and ground truth gets scarcer, which is to say the work shifts from test-shaped toward eval-shaped.

**Rung one: is this edit damaging?** Vandalism and edit-quality detection. This is the bottom rung because trace data is abundant (every revert is a label) and the question is close to test-shaped. The ORES edit-quality models live here; so do the Wikidata vandalism detectors from the [WSDM Cup 2017 challenge](https://arxiv.org/abs/1712.05956), which introduced a shared dataset and drew multiple competing systems.

**Rung two: does this sentence need a citation?** The ["Citation Needed" work](https://dl.acm.org/doi/10.1145/3308558.3313618) (Redi, Fetahu, Morgan, and Taraborelli, 2019) built a taxonomy of *why* Wikipedia requires inline citations, then a large crowdsourced dataset of sentences labeled against it, and models to predict whether a statement needs a source. ["Citation Detective"](https://wikiworkshop.org/2020/papers/Wiki_Workshop_2020_paper_12.pdf) (Chou and colleagues, 2020) turned that into a public dataset spanning English Wikipedia at scale — and, encouragingly for anyone starting in biomedicine, found that Medicine and Biology articles are among the best-sourced on the project.

**Rung three: does this citation actually support the claim it's attached to?** This is the hardest rung, and the closest to what a cite-checking harness needs to do. The reference work here is [SIDE](https://www.nature.com/articles/s42256-023-00726-1) (Petroni and colleagues, published in *Nature Machine Intelligence* in 2023): a system that flags Wikipedia citations unlikely to support their claims and proposes better ones from the web, trained on existing Wikipedia references — learning, in the authors' framing, from the combined wisdom of thousands of editors. For the citations it was most confident were unverifiable, human reviewers preferred its suggested replacement over the original about 70% of the time. That's impressive — and notice that even here the "ground truth" is ultimately a crowd of humans expressing a preference, because at this rung there is no checksum to appeal to.

The ladder is itself the argument. At the bottom, ground truth is cheap and the checks are nearly tests. At the top — *does the source support the claim* — ground truth thins almost to nothing, and you are doing evals against contested human judgment. A commons harness earns its keep precisely on the top rung, where no single label source is sufficient and you need the whole toolbox at once.

## A note on the diff as the unit

One more piece of prior art deserves its own mention, because it validates an instinct that runs through this whole project: that the *edit* — the diff — is the natural atomic unit of verification, and an edit's fate is a form of ground truth.

The [VitaminC dataset](https://aclanthology.org/2021.naacl-main.52/) (Schuster, Fisch, and Barzilay, 2021) is built from more than 100,000 Wikipedia revisions in which someone changed an underlying fact, yielding several hundred thousand claim-and-evidence pairs that are nearly identical except that one version supports a given claim and the other contradicts it. It even defines a task for spotting the revisions that change a fact in the first place. This is the "edit fate as ground truth" idea made concrete and public — the diff as the thing you reason about, not the finished sentence in isolation. ([FEVER](https://aclanthology.org/N18-1074/), from Thorne and colleagues in 2018, is the older and larger fact-verification benchmark most of this work descends from, if you want the canonical ancestor.)

For anyone wanting the full landscape rather than these highlights, there's a 2024 survey — ["Wikimedia data for AI: a review of Wikimedia datasets for NLP tasks and AI-assisted editing"](https://arxiv.org/abs/2410.08918) — that catalogs essentially all of it in one place.

## What this buys us, and what it doesn't

The optimistic reading of all this prior art: the commons is not starting from zero on the trust problem. There is a decade of labeled data, shared benchmarks, and hard-won methodology to inherit. A harness can orchestrate many of these checks at once — a deterministic test where one exists, an independent model's second opinion, a consensus vote across several models, the editor's own judgment at the points that matter — and reach a level of rigor no single reviewer could sustain by hand. That orchestration is the move. No one strategy is sufficient; the harness is what lets you run all of them and weigh the results.

The honest reading: on the top rung, where the commons's hardest and most valuable questions live, the ground truth is *us* — human editors, disagreeing, revising, sometimes reversing ourselves. No amount of orchestration changes that. The harness can make human judgment far more effective; it cannot replace it. What it can do is make every check it runs inspectable and contestable — so that trust rests not on the model's confidence, but on a verification process the community can see, argue with, and improve.

That is the difference between an unharnessed model writing an article and a governed system helping to build one. And it is why "trust the system that checks the model" is a very different proposition from "trust the model."

---

*Next: design criteria for a commons harness — the standards any of this has to meet.*

---

## Image

- Candidate: Boston Public Library, *19th Century American Trade Cards* collection (Flickr). Caption: "What happens if your harness doesn't validate." (Confirm exact rights line on the specific photo before publishing; pull card title for attribution.)
