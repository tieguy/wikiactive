---
description: Use when about to claim any work is complete, fixed, or passing, before committing or declaring a phase done. Requires running the verification command and reading its output in the current turn before any success claim. Evidence before assertions, always.
---
<!-- Ported from ed3dai/ed3d-plugins verification-before-completion (CC-BY-SA 4.0;
     lineage via obra/superpowers, MIT). -->

# Verification Before Completion

Claiming work is complete without verification is dishonesty, not efficiency.

**Iron law: no completion claims without fresh verification evidence.** If you have
not run the verification command in this turn, you cannot claim it passes.

## Gate function

Before claiming any status or expressing satisfaction:

1. **Identify** the command that proves the claim (`cargo test`, `cargo clippy
   --all-targets`, `wa check <slug>`, ...).
2. **Run** it, fresh and complete.
3. **Read** the full output, check the exit code, count the failures.
4. **Verify** the output confirms the claim. No → state the actual status with
   evidence. Yes → state the claim with the evidence attached.
5. Only then make the claim.

## Claims and their required evidence

| Claim | Requires | Not sufficient |
|---|---|---|
| Tests pass | Test output with 0 failures | Previous run, "should pass" |
| Linter clean | `cargo clippy` output, 0 errors | fmt passing, extrapolation |
| Build succeeds | Build command exit 0 | clippy passing |
| Bug fixed | Original symptom test passes | "Code changed" |
| Regression test works | Red-green verified (fails without fix) | Test passes once |
| Subagent completed | Diff/ output inspected | Subagent reports success |
| Requirements met | Line-by-line AC checklist | Tests passing |

## Red flags — stop

- "should", "probably", "seems to"
- Satisfaction expressed before verification
- About to commit or hand back without verification
- Trusting a subagent's success report without checking its evidence
- "Just this once"

Skip any step and you are not verifying, you are asserting. Run the command, read
the output, then make the claim.
