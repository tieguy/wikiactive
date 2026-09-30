# ADR-0001: Skip the TALK provenance note by default

**Status:** Accepted 2026-09-29 (recorded 2026-09-30)

**Context:** The end-of-session protocol included a TALK-page provenance note per
article session. Provenance is already carried by the review artifacts, per-edit
diffs, and the disclosure log. During the Kidder close-out the operator found the
extra note was usually writing to an audience that wasn't there.

**Decision:** Default to skipping the TALK note. Write one only when a talk-page
audience genuinely needs the reasoning — for example, a contested fact.

**Alternatives considered:**
- *Always write the note* (the original protocol) — rejected: redundant to the
  diffs and disclosure log; adds talk-page noise (Kidder close-out finding).
- *Never write one* — rejected: the contested-fact case exists, and silencing it
  removes the one audience that does need the reasoning.
- *Default-skip with an escalation trigger* (chosen) — keeps the channel for the
  case that needs it, stops the ritual.

**Consequences:** Reviewers should not flag the absence of a TALK note. Provenance
stays discoverable via the disclosure log (`User:LuisVilla/wikiactive/log`) and
per-edit diffs. The escalation judgment stays with the operator.

**Open questions:** none — the trigger ("genuinely needs the reasoning") is
deliberately left as operator judgment.

**Provenance:** operator decision 2026-09-29, Kidder close-out, recorded in
PLAYBOOK "End-of-session artifacts."
