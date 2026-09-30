# Linter rules — prior-art survey (2026-09-30)

Rule-enforcement plan, item 6 Part B. Deliverable: compare the rules in
`rules/linter.toml` with the community's existing mechanical-rule
corpora; end with a short list of rules to add, each with its source
definition. No code in Part B.

## Sources (fetched 2026-09-30 with `wm-fetch`, per the plan)

- **Check Wikipedia** — the error list at
  `https://en.wikipedia.org/wiki/Wikipedia:WikiProject_Check_Wikipedia/List_of_errors`
  (113 numbered errors, each typed Accessibility / Display problem /
  Syntax error / Source readability / Against convention / Incorrect
  content, with tool-support columns for WPCleaner, AWB, autoFormatter).
  The live toolforge interface (`checkwiki.toolforge.org`) declined
  requests (403); the on-wiki list is the same data.
- **AutoWikiBrowser general fixes** —
  `https://en.wikipedia.org/wiki/Wikipedia:AutoWikiBrowser/General_fixes`
  (the default-on, "uncontroversial" fix set, ~40 named fixes).

## 1. Our rules vs the community definitions

| Our rule | Community counterpart | Match? |
|---|---|---|
| `refname-autonumber` (`:N` ref names) | None at this granularity. CW 104 ("unbalanced quotes in ref name or illegal character") governs ref-name *syntax*; our rule bans the VisualEditor autonumber *convention* per house CITEVAR (`rules/house-rules.toml` `[citevar] ban_autonumber_refnames`). | Fork-specific; compatible. |
| `sfn-usage` (no `{{sfn}}`) | None — CW/AWB treat sfn as legitimate. Ours is house CITEVAR (`ban_templates = ["sfn"]`). | Fork-specific by design. |
| `page-pages-consistency` (single value in `|pages=`) | Adjacent only: AWB Citation templates fixes field *typos*, not the page/pages contract. | Fork-specific; no conflict. |
| `named-ref-with-pinpoint` (`\|page=` inside a named ref) | None found in either corpus. House CITEVAR (`pinpoint_template = "rp"`). | Fork-specific. |
| `heading-spacing` (`== X ==`) | Same FAMILY, different member: CW 7/8/19/105 govern heading *structure* (start/end `=`, single-`=`, hierarchy); none checks interior spacing. | Ours complements theirs — see add-list (heading-hierarchy). |
| `semicolon-prose` | None; deliberately a house drafting guard (per PLAYBOOK: never attributed to the MOS). | Fork-specific by design. |
| `tense-drift` | None in CW/AWB (both are syntax-level). Community definition is MOS:TENSE; ours is a mechanized proxy (`is now/are now/is currently/are currently`). | Proxy, documented as such. |
| `national-variety-mix` | None in CW/AWB. Community definition WP:ENGVAR / MOS:SPELLING; ours checks twin markers. (Item 6A removed the root-word false blocks.) | Proxy. |
| `italic-mismatch` | CW 38 is different (HTML `<i>` vs `''`), a display concern; ours enforces *consistent* italicization of a work's name (MOS:ITALICS spirit). | Different targets; no conflict. |
| `see-also-duplication` | AWB has no such fix; MOS:NOTSEEALSO / "most strongest link" practice is the definition. | Proxy, warn-level. |
| `lead-body-duplication` | None; MOS:LEAD ("lead summarizes, never duplicates"). 8-word shingle is our mechanization. | Proxy, warn-level. |

Bottom line: nothing in our set duplicates a community rule; the four
citation rules and the two drafting guards are house policy, the style
rules are mechanized MOS proxies the corpora do not attempt.

## 2. Their rules that apply to text THIS tool drafts

The failure surface that matters is a model-drafted or model-spliced
block. From CW's 113:

- **CW 61 / AWB `RefsAfterPunctuation`** — reference before punctuation
  (WP:REFPUNC). We INSERT citations; this is our most likely mechanical
  citation-placement defect.
- **CW 10, 43, 46** — unbalanced `]`, template end, `[` (the pairing
  family). A splice that breaks pairing is the likeliest structural
  failure of block replacement. AWB's equivalent is `FixSyntax`
  (`FixUnbalancedBrackets`).
- **CW 32 / AWB `FixSyntax`** — double pipe in a link (`[[foo||bar]]`).
- **CW 113 / AWB `FixLinkWhitespace`-adjacent** — newline inside a
  wikilink.
- **CW 64 / AWB `SimplifyLinks`** — `[[Dog|Dog]]` (link equals
  linktext).
- **CW 93 / AWB `FixSyntax`** — `http://http://` double scheme (a
  paste-path defect).
- **CW 25** — heading hierarchy skips (a drafted `==== X ====` under a
  `== Y ==`).
- **CW 3 / AWB `AddMissingReflist`** — refs present but no reference
  list (whole-page; marginal for single-block edits but real when we
  add the first ref to a sectionless stub).
- **CW 78** — duplicated reference list (same marginal case).

Not applicable to our drafting: the accessibility family (CW 26/38/39/
40/42/55/63/66 — we do not emit HTML style tags), DEFAULTSORT (AWB
`SetDefaultSort`, CW 6/37/88/89 — out of scope for prose edits),
interwiki/category placement (CW 45/51/52/53), people categories
(`FixPeopleCategories`), and typo/dates/units fixers (AWB `FixDates`,
RETF, `FixTemperatures` — those normalize existing text; our paraphrase
gate already governs drafted wording).

## 3. Rules to add (each with its source definition)

1. **`ref-punctuation`** (added-lines, error) — a `<ref>` must follow
   terminal punctuation, not precede it. Source: CW 61 "Reference
   before punctuation"; AWB `RefsAfterPunctuation` (WP:CITEFOOT /
   WP:PAIC).
2. **`unbalanced-markup`** (drafted-lines, error) — in the drafted run,
   `[[ ]]`, `{{ }}`, `<ref>…</ref>` pairs must balance. Source: CW 10
   "Square brackets with no correct end", CW 43 "Template with no
   correct end", CW 46; AWB `FixSyntax`/`FixUnbalancedBrackets`.
3. **`link-pipe-defects`** (drafted-lines, error) — `[[foo||bar]]`
   (CW 32 / AWB `FixSyntax`) and a newline inside a wikilink (CW 113).
4. **`link-equals-linktext`** (drafted-lines, warn) — `[[Dog|Dog]]` and
   `[[Dog|Dogs]]` → `[[Dog]]s`. Source: CW 64 "Link equal to linktext";
   AWB `SimplifyLinks` (MOS:PIPES).
5. **`heading-hierarchy`** (drafted-lines, error) — a drafted heading
   must not skip a level below its nearest parent heading. Source: CW 25
   "Headline hierarchy" (accessibility).

Deliberately NOT added now: `double-scheme-urls` (CW 93 — rare in
drafted prose; revisit if URL drafting appears), `missing-reflist`
(CW 3 — whole-page scope interacts with single-block edits; needs its
own design), and AWB's date/typo/units normalizers (they rewrite
existing text, which is the previous author's style, not ours).

All five additions fit the existing checker architecture
(`rules/linter.toml` + `check_rule`/`check_rule_on` + the item-6A
run-joining), so implementation is additive: declare the rule with
samples, add the arm, extend `KNOWN_RULE_IDS`, and the config-derived
AC.4 test generates the case.
