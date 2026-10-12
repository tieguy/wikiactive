---
title: "What Is a Harness?"
date: 2026-06-22
description: "LLMs are engines. Harnesses are everything else—the wheels, brakes, dashboard, GPS—that turn a raw engine into a useful vehicle. First in a series on harnesses for the open knowledge commons."
tags: [llm, wikipedia, open-source, harness, open-knowledge, commons]
source: https://lu.is/2026/06/what-is-a-harness.md
---

![A Viking Age copper-alloy harness fitting (selbågskrön) from Aska, Östergötland, Sweden, 800–1100 CE](/images/harness-shm-selbagskron.jpg)

*Harnesses are a very old technology. Here, a [Viking Age harness fitting (selbågskrön)](https://samlingar.shm.se/object/1CA941EF-5E9D-4A0C-8920-A933C20FC767), Historiska museet/SHM. Photo: Christer Åhlin & Gunnar Andersson. [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).*

*First in (hopefully?) a series on harnesses for the open knowledge commons. Today: what is a harness anyway?*

## The future (and present) of LLMs isn't chat

Hundreds of millions of people use "AI" as chat (like ChatGPT) or search (like they used the box on google.com for two decades). In other words, for most people "AI" looks like "put a question in a box, and get back an answer." That's not wrong—but it is importantly incomplete.

This post will try to define and think through the LLM usage model many software developers are beginning to default to, and explore as a design space: "harnesses".

## The engine (LLMs) and the car (a harness)

Software developers who have used the leading tools for AI-assisted coding, like Claude Code, know that the large language model itself is only part of what makes the experience work. In the core of these tools, a model writes code—but there is an increasingly complex infrastructure around that model. This infrastructure decides which files to read and load, writes and run tests, keeps records, checks permissions, and generally keeps track of what is going on. In these posts, I'll call that infrastructure a **harness**.

To put it another way: the LLM is an engine. The harness is everything else that turns a raw engine into a useful vehicle—the wheels, the brakes and seatbelts (very important!), the dashboard, the GPS, etc.

The distinction between LLM and harness matters because most public conversation about "AI" collapses the two. Debates about what AI can and can't do, whether it's trustworthy, whether it is overhyped: these are often really debates about specific harnesses wrapped around specific models. [A 2024 model in a well-designed harness can match or outperform a 2026 model](https://aisle.com/blog/ai-cybersecurity-after-mythos-the-jagged-frontier). A brilliant model in a lazy harness can easily produce sloppy work—and in fact you can think of early "chat" models as very lazy harnesses, sort of like attaching a rocket engine to a surfboard.

## Why does this matter to open knowledge?

The question "can LLMs improve the quality of Wikipedia?" is hard to answer in the abstract, and usually pointless to argue about. A much better question, I think, is: "what would a harness for contributing to Wikipedia look like, could it provably improve Wikipedia's content, and how could it support the community's values?" That's a question that is tractable, testable, and worth arguing about.

This (hopefully!) series will try to work through that question. To do it constructively, we first need shared vocabulary for what a harness is and what it contains.

## A rough anatomy of harnesses

Harnesses vary, and are in early days. There are some common themes, though. In particular, I find a helpful way of thinking about it is to look at three levels:
- **inner loop**: the tight cycle where direct creation actually happens
- **outer scaffolding** (or sometimes **outer loop**): the persistent infrastructure that makes the inner loop safe, auditable, and improvable over time. An assumption in software, which may or may not be true in open knowledge, is that the inner loop runs many times for each run of the outer loop.
- **the ecosystem**: arguably, a special case of the outer scaffolding is the outside "world" in which the harness operates (such as GitHub in code or Google Book Search or Internet Archive in knowledge) which provides additional constraints or opportunities.

Not all harnesses include all of these components; it's still such early days that when I first started this draft there wasn't even a good definition I could find for it in coding, much less other forms of knowledge work. (If you're interested in coding, part of why I picked this back up is because of [this good summary](https://dineshkarthik.me/blogs/vibe-coding-vs-agentic-engineering).)

### The inner loop

In a harness's inner loop, work happens in a cycle, roughly: plan → act → validate → replan. The components that implement this cycle:

- **Planning** translates user intent into actionable units. A user typing "improve this article" becomes a list of specific checks to run and edits to propose.
- **Coordination** dispatches work to sub-agents, tools, and sometimes different models. When the harness uses more than one model — say, an open-weight model for bulk work and a stronger proprietary model for hard cases — coordination includes **routing**: deciding which model handles which subtask.
- **Integration** is the domain-specific knowledge of how to interact with the world. For a code harness, this is the Unix command line, git, language toolchains. For an office harness, it's the guts of docx and xlsx. For a commons harness, it could be wrappers around MediaWiki's APIs, Wikidata's data model, Flickr, and bibliographic identifier systems like DOI.
- **Context management** decides what goes into the model's window for this specific task. It's distinct from memory: context is ephemeral and task-scoped. For code, codebase indexing, retrieval, file selection all live here. In practice this is often the component that most determines whether the harness feels smart or stupid.
- **Validation** checks whether the work actually succeeded. More on this below — it's the component with the most interesting design space, and is most important for building trust.
- **Error recovery and escalation** handle failure. Retry, reroute, degrade, or stop and ask a human. Getting this wrong produces the "agent spins in circles until it runs out of budget" failure mode.

### The outer scaffolding

Around the inner loop sits infrastructure that persists across tasks and sessions:

- **User interface.** This is most often chat, but not necessarily. Different domains call for different kinds of integrations, review queues, dashboards, custom visualizations. The UI shapes what kinds of work are easy to ask for and easy to trust.
- **Memory.** Persistent state across sessions: what the harness has learned about this codebase, this contributor, this project's conventions. Distinct from context management — memory is what the harness remembers, context is what the model sees right now.
- **Permissions and guardrails.** What the agent can do unattended versus what requires human approval. Sandboxing, read-only modes, approval gates for destructive actions. This must, as much as possible, live in deterministic code (and outside the inner loop), because the LLM cannot ultimately be trusted.
- **Observability.** Logs, traces, diffs, audit trails. What the agent did and why, inspectable after the fact. Essential for trust, essential for debugging, essential for community review.
- **Evaluation of the harness itself.** Test sets and benchmarks for the harness. How do you know version N+1 is an improvement over N?

### The outside world

A special case of the outer scaffolding is the literal outside world. A harness can act to the extent that the outside world—the environment into which the harness is deployed—allows it. For developers, think of Debian's unstable/testing/stable progression. A code harness might operate against a scratch branch before a main branch. A commons harness might write to a staging mirror before production Wikipedia. This substrate question turns guardrails from "can we trust this edit" into the more tractable "what's the promotion pathway from experimental to canonical."

## Validation is a design space, not a checkbox

One component deserves more attention, because it's where harnesses for the commons will earn or lose credibility: validation.

In software development, validation often has a ground truth to check against — the tests either pass or they don't. For claims in a knowledge commons, ground truth is rarer. "Is this sentence well-sourced?" has no unit test. And yet a harness can still bring a meaningful range of approaches to bear:

- **Ground-truth validation** where available — does the ISBN checksum, does the DOI resolve, does the claim survive a Wikidata schema check.
- **Adversarial validation** — a second agent, possibly a different model, tries to break or critique the output. (This can be complex, or can be as simple as "comment on this like the most critical jerk you can be".)
- **Multi-model voting or ensembling** — have several models independently evaluate. If they have consensus, that is useful signal; if they disagree, that's a flag for human review.
- **Human-in-the-loop** — the escalation case, expensive but sometimes necessary.

The interesting argument for harnesses isn't that any single strategy is sufficient. It's that *all of them together*, orchestrated, can reach a level of rigor that no individual reviewer could apply at scale. Choosing which strategy to use when is a design decision, not a one-time commitment. (Future posts will return to this.)

## Open incrementalism

It's worth noting that a well-designed harness makes "moving toward open components" a tractable, incremental goal rather than an all-or-nothing switch. Because a harness is, by design, modular, open components can be swapped in as they mature. This will be particularly important for open models, which can already be used quite reliably for citation checking but probably not for other drafting tactics yet.

This ability to incrementally become open is, I think, one of the strongest arguments for taking the harness framing seriously in commons work. It transitions the question from "is this chatbot open" (rare, especially with high quality) to "is this a system that we can push steadily towards open, component by component". That's a question I feel comfortable we can move forward on.

## Why the harness framing matters for the commons

Commons communities have good reasons for skepticism about LLMs. Wikipedia's editors have seen hallucinated citations, fabricated articles, promotional drift, and genuine damage to existing content. Many have concluded that LLMs are categorically incompatible with the community's values around verifiability, neutrality, and human-scale deliberation.

The harness framing doesn't dismiss any of that—it reframes the question. The issue with an LLM-generated Wikipedia article isn't that *a model wrote it*; it's that the model wrote it with no planning, no validation, no provenance, no memory, no observability, no guardrails, no evaluation — in other words, with no harness. It's not surprising the result is unacceptable. An unharnessed engine running at full throttle in a neighborhood is always going to do damage.

The productive question for open, then, is whether it's possible to build harnesses rigorous enough to meet community standards — and whether doing so is worth the effort. That's an empirical question, and one I hope to answer with more experiments.

The next post turns to what such a harness would need to satisfy. Before we can evaluate any candidate MVP, we need to be clear about the standards it's being asked to meet.

---

*Next: design criteria for a commons harness.*
