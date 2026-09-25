# Tier 1 — Judgment core (always in context)

These are the operative clauses loaded verbatim into every `wa analyze` context
bundle. They are *distilled rules*, not summaries: each clause is written to be
checkable at the sentence level against the evidence quote that will anchor the
edit. Canonical sources live in `rules/canonical/` (fetched 2026-09-24; revids
pinned in the fetch log). If a clause here ever conflicts with the canonical
page, the canonical page wins — file an issue and fix the clause.

---

## Cluster A — NPOV as attribution

**A1. Attribution over wikivoice.** A claim that a *person or publication*
asserts, believes, argues, claims, or characterizes is written with the
asserter attached (`X argued that …`, `according to Y, …`), never promoted into
encyclopedic voice. The test: delete the attribution — if the remaining
sentence is now something a party to a dispute would dispute, the attribution
must stay.

**A2. Predicate chooses the verb.** `said`/`wrote`/`stated` are safe. `argued`,
`admitted`, `claimed`, `insisted`, `revealed`, `admitted`, `maintained`,
`asserted` each carry stance; use them only when the source's own framing
warrants it and the quote anchors that framing. Never let the verb editorialize
beyond the quote.

**A3. Words to watch (WEASEL, PEACOCK).** "widely considered", "some critics
say", "it is believed", "notably", "famous", "legendary" — either anchor the
specific holder of the view ("critic A of outlet B called it …") or cut it.
"Weasel words with an identifiable holder" are still weasel if the holder is
vague ("some scholars").

**A4. BLP-by-analogy for the dead.** Contentious characterizations of
identifiable people get the same attribution care whether or not they are
living. A dead person cannot sue, but their biography can still be advocacy.

**A5. Both sides of a real dispute, in proportion.** Where significant
published perspectives conflict, the article reflects the distribution of
published opinion — not a 50/50 staging of a 95/5 literature, and not the
silencing of a significant minority view. Conflicts between *sources* in a
session surface as `{{efn}}` notes, never silent adjudication (see card
`efn-conflicts`).

### Failure examples (Cluster A)

- *Gouldner wikivoice (operator fixture):* an article stated Gouldner's
  thesis — that a group's "norm of reciprocity" served ruling interests — as
  encyclopedic fact. Gouldner's own framing ("I suggest…", a 1960 journal
  article) is an argument by a scholar; the article must say Gouldner
  argued this, and say who else built on or contested it.
- *Upstream (WP:NPOV § Attribution and bias):* "The novel is regarded as
  an enduring masterpiece" → "The novel has been praised by critics"
  with citations, or cut entirely. The unattributed superlative is the
  canonical failure.
- *Upstream (MOS:Words to watch):* "Some people think…" — name the people
  or drop the sentence.

---

## Cluster B — V / NOR / SYNTH at sentence level

**B1. Claim-scope match.** Every added/changed sentence's *scope* must match
the evidence quote's scope. A source saying "3 million copies sold in Japan by
1986" supports "3 million copies sold in Japan by 1986" — not "3 million
copies sold", not "3 million copies sold worldwide", not "sold 3 million".
Scope-widening is the most common model failure: numbers, places, dates, and
populations are load-bearing.

**B2. One quote, one source.** A sentence supported by fusing two sources is
SYNTH unless each clause is individually anchored and the joined claim is the
sources' claim, not yours. "A 2020 Danish survey found X. A 2021 EU report
found Y." is two anchored sentences. "Danish surveys found that X causes Y" is
a fusion nobody wrote.

**B3. No negative-search claims.** "X was the first/only/largest …" and "no
evidence exists that …" require a source *asserting the negative or the
superlative*. Absence of a hit in your searches is not a citation.

**B4. Primary sources stay descriptive.** Primary material (the subject's own
site, original documents, datasets, the work itself) may support plain
descriptive facts about itself; every interpretation, evaluation, or pattern
claim needs a secondary source. "The company's site says it was founded in
1998" ✓ (attributed descriptive). "The company pioneered the field" ✗ unless a
secondary source says so.

**B5. The quote is the unit of truth.** If the verbatim quote isn't in a
ledger-fetched source, the sentence doesn't exist. Verifiability means someone
else can check the quote against the source and the sentence against the quote
— the quote-anchor validator enforces this mechanically (`checks/gate.rs`).

### Failure examples (Cluster B)

- *3-million-copies scope (operator fixture):* draft said "sold three million
  copies"; source said three million *in a specific market* *by a specific
  year*, with a different worldwide figure elsewhere in the same article.
  The sentence was scope-widened against its own anchor.
- *Danish survey fusion (operator fixture):* two separately-reported surveys
  (one Danish, one EU-wide) fused into "Danish surveys show…". Neither source
  makes the fused claim.
- *Upstream (WP:NOR § Synthesis):* "Though Washington was not officially a
  member, scholars consider him a de facto Freemason" fused from a fact source
  (not a member) and an opinion source — the canonical published-synthesis
  example.
- *Upstream (WP:V § Burden):* "The burden to demonstrate verifiability lies
  with the editor who adds or restores material" — i.e., "I saw it somewhere"
  is never sufficient; the quote in the ledger is.

---

## Cluster C — LEAD / SUMMARYSTYLE

**C1. The lead summarizes, never duplicates.** No lead sentence should be a
near-copy of a body sentence. Mechanized by the lead-vs-body shingle check
(Tier 3, `lead_body_dup`): if the lead and a section share long n-gram runs,
rewrite the lead to *condense* (facts get compressed; the section carries the
detail and the citations).

**C2. Lead reflects relative weight.** The lead gives each topic the space its
sourced body coverage warrants — the most-covered aspect first, controversies
only if the body covers them. A lead paragraph about something the body
covers in two sentences is UNDUE in the most-read part of the article.

**C3. Every paragraph has a home.** Material moves between sections by
*summarizing* the destination, not by copying (SUMMARYSTYLE). When a section
grows a spinout, the parent keeps a two-to-three-sentence summary and a
`{{Main}}` hatnote.

**C4. Structure follows content.** Headings reflect the sourced coverage that
exists (MOS:LAYOUT order: lead → body in rough order of weight → See also →
References → ...). Never draft a heading for content you *wish* existed.

### Failure examples (Cluster C)

- *Lead/body duplication (operator fixture):* an article whose lead's second
  paragraph repeated the "Concept and mechanisms" section nearly verbatim,
  including duplicated citations. Fix: compress the lead to its own words,
  move detail to the section.
- *Upstream (MOS:LEAD):* "The lead should stand on its own as a concise
  overview of a topic's key facts" and "apart from basic facts, significant
  information should not appear in the lead if it is not covered in the
  remainder of the article" — the lead is a summary *of* the body, both ways.

---

## Standing checklist (run before proposing ANY edit)

1. Which ledger quote(s) anchor every added/changed sentence? (quote ids)
2. Does each sentence's scope equal its quote's scope? (B1)
3. Did I fuse sources? (B2) Is any claim negative or superlative? (B3)
4. Primary or secondary? (B4)
5. Whose view is this, and is the attribution in the sentence? (A1–A5)
6. Does the lead still summarize without duplicating? (C1–C2)
7. One logical edit, one scoped summary? (granularity rule)
8. Linter green on added lines? (Tier 3)
