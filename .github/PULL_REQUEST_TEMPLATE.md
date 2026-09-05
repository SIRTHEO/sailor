<!-- Short is fine. Both sections are not. `CONTRIBUTING.md` has the long version. -->

## What it resolves

<!-- The defect, the missing behaviour, or the decision this implements — in
your own words, and why this is the right repair. Link the issue or the fault
number if there is one. -->

## How it was tested

<!-- The commands you ran and their verdict. Say which test you saw go red when
you put the original defect back: a test never seen to fail is not a test. -->

```sh
cargo test --workspace --no-fail-fast
```

- [ ] The ratchet is green on my `HEAD` (`cargo build -p sailor && ./target/debug/sailor ratchet`) — and if a seed moved, the commit says which and to what number.
