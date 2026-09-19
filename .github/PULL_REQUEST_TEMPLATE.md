<!-- Write in the first person without naming a model, a session, or the tool that helped, and do not quote anyone's prompt or conversation. Where useful, credit a finding to the check that found it. -->

## Problem

<!-- The defect, the missing behaviour, or the decision this implements, in your own words, and why this is the right repair. Link the issue if there is one. -->

## Behaviour

<!-- What Sailor does after the change that it did not do before, as a user or a flow sees it, and what the change touches (the crates, `desktop/`, a `.flow.json`). -->

## Verification

<!-- The commands actually run and their verdict, with the `test result:` lines pasted. Say which test was seen to go red when the original defect was put back, and confirm that the ratchet is green on your `HEAD` (and which seed moved to what number, if one did). -->

- [ ] The ratchet is green on my `HEAD` (`cargo build -p sailor && ./target/debug/sailor ratchet`) — and if a seed moved, the commit says which and to what number.
- [ ] Every applicable gate from CONTRIBUTING's "Every gate you may hit" ran, with its `test result:` line pasted below (not just "passed").
