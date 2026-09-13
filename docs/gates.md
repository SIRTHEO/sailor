# Gates

Every worker, reviewer and integration run the same letters below: nobody
picks a lighter list for their own change, and a reviewer or the integration
reruns exactly what the branch already ran. A branch is not green until every
line that applies has run in the tree under review, with
`CARGO_TARGET_DIR=$PWD/target/own` and `-j 1`, and its `test result:` line is
in the report. A reviewer reruns them; the integration reruns the judges on
the combined trunk. Seeds move only in the allowed direction — the counters
suffixed `_TODAY` upward, comment ratios and long blocks downward.

## A. Always

- `cargo test -p sailor -j 1 --test the_battery_does_not_shrink_in_silence --test comments_do_not_crowd_out_the_code --test the_fault_table_holds_together`
- `cargo test -p sailor -j 1 --test the_words_a_user_reads_are_in_english --test a_product_name_in_prose_only_ever_falls --test identifiers_are_in_english`
- `cargo test -p sailor -j 1 --test no_engine_is_named_in_the_code --test no_product_home_is_written_into_the_code --test nothing_reserved_is_tracked --test the_repository_ships_no_workshop_flow --test no_push_publishes_a_private_name`
- `cargo clippy -p <every crate touched> --tests -j 1` — clean
- `git log main..HEAD --format=%B | grep -E '^(Co-Authored-By|Claude-Session):'` — empty
- Commits by path, project voice, no model named, no private names in fixtures

## B. When a `.flow.json` or `token-seeds.json` changes

- `cargo test -p sailor -j 1 --test a_flow_never_grows_what_it_sends_in_silence --test every_flow_path_the_code_names_exists --test take_the_next_fault --test take_the_next_work`
- `cargo test -p flow -j 1`

## C. When `crates/actions` or a brake changes

- `cargo test -p actions -j 1 --no-fail-fast` and `cargo test -p flow -j 1` (both suites, always both)
- A first-execution timeout (`engine_timed_out` on a fresh temp-file engine) is rerun once and said aloud

## D. When `crates/profiles`, the login probe or a launch changes

- `cargo test -p profiles -j 1`
- One real probe on a real home: `SAILOR_TEST_CLAUDE_HOME=<home> cargo test -p actions -j 1 --test <the probe test>` — pasted, not skipped

## E. When `desktop/` changes

- `cd desktop && npm test` (the `Test Files` / `Tests` lines) and `npx tsc --noEmit`
- `cargo test --manifest-path desktop/src-tauri/Cargo.toml -j 1`
- A walkthrough on the fixture store, with screenshots, for any change a person sees

## F. Before integration

- The branch's `HEAD` is recorded before a reviewer starts; the review names that commit; a different `HEAD` invalidates the review
- The combined trunk runs A plus every applicable letter, then the per-crate battery in a tab with line-by-line logging
