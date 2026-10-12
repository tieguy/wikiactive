# ADRs — reading note

These ADRs (`0001`–`0011`) record wikiharness's architectural decisions from the 2026-06-01/02 design pass, made for the **TypeScript MVP**. Read them as a frozen historical design record, with two pieces of current-state context:

- **The TypeScript implementation is archived.** Where an ADR reports `Status: Built` (e.g. `0009`) or describes `packages/…` paths in the present tense (e.g. `0008`, `0011`), that code is preserved at the `code-archive-final` tag (`git checkout code-archive-final -- code-archive/`) — it is no longer in the working tree. The decisions stand; the code that realized them moved to the archive.
- **Active coding moved to SP42 (Rust).** Per `CLAUDE.md`, SP42 cites these ADRs as design and governance *reference/evidence*, not as binding wikiharness contracts. Current SP42 collaboration state lives in `docs/collab/`.

Convention reminder (from `CLAUDE.md`): bare backtick numbers cite the **numbered design docs** (`` `02` ``, `` `07 §3` ``); ADRs are cited as `` `adr/0005` ``. Update an ADR only when a decision actually changes — and when you do, update the relevant numbered doc(s) too and keep cross-references consistent.
