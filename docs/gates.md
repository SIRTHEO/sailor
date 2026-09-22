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
- `cargo test -p sailor -j 1 --test no_engine_is_named_in_the_code --test no_product_home_is_written_into_the_code --test nothing_reserved_is_tracked --test the_repository_ships_no_workshop_flow --test no_push_publishes_a_private_name --test the_publication_boundary_holds --test no_workbench_tool_is_named_in_the_code`
- `cargo test -p sailor -j 1 --test no_forge_no_remote_no_trunk_is_named_in_the_code --test files_do_not_grow_out_of_scale --test a_source_file_holds_code_not_a_suite --test clippy_only_ever_gets_quieter`
- `cargo test -p sailor -j 1 --test production_code_does_not_panic_on_purpose --test every_child_process_starts_by_one_road --test tests_read_no_state_of_this_machine`
- `cargo test -p sailor -j 1 --test every_declared_check_names_a_test_that_exists --test every_ratchet_is_named_in_the_gates` — the two that keep this list honest in both directions
- `cargo clippy -p <every crate touched> --tests -j 1` — clean
- `git log main..HEAD --format=%B | grep -E '^(Co-Authored-By|Claude-Session):'` — empty
- Commits by path, project voice, no model named, no private names in fixtures

## B. When a `.flow.json` or `token-seeds.json` changes

- `cargo test -p sailor -j 1 --test a_flow_never_grows_what_it_sends_in_silence --test every_flow_path_the_code_names_exists --test take_the_next_fault --test take_the_next_work`
- `cargo test -p sailor -j 1 --test a_flow_step_is_not_a_shell_program` — the seed counts the steps that still reach for a shell
- `cargo test -p sailor -j 1 --test a_shipped_flow_calls_the_functions_it_defines --test a_step_may_not_name_a_field_after_what_it_points_at` — a step that defines shell it never runs, and a `with` field that covers the value its own pointer reaches for
- `cargo test -p sailor -j 1 --test a_check_that_failed_before_the_attestation_is_asked_again` — integration's merge step, run against a forge that answers from a file
- `cargo test -p flow -j 1`

## C. When `crates/actions` or a brake changes

- `cargo test -p actions -j 1 --no-fail-fast` and `cargo test -p flow -j 1` (both suites, always both)
- A first-execution timeout (`engine_timed_out` on a fresh temp-file engine) is rerun once and said aloud
- `cargo test -p actions -j 1 --test a_candidate_leaves_only_through_the_gate` — the last gate before a draft, integration or a release lets a candidate leave, proved without a forge or a binary in service

## D. When `crates/profiles`, the login probe or a launch changes

- `cargo test -p profiles -j 1`
- One real probe on a real home: `SAILOR_TEST_CLAUDE_HOME=<home> cargo test -p actions -j 1 --test <the probe test>` — pasted, not skipped

## E2. When a tree is taken down: `worktree_cmd`, the sweep or `workspace::standing`

- `cargo test -p sailor -j 1 --test a_tree_somebody_is_in_is_never_closed --test a_closed_tree_takes_its_index_identity_with_it` — a tree somebody is in is kept, whoever asks
- `cargo test -p workspace -j 1`
- `cargo test -p actions -j 1 --test a_tree_the_flow_closes_is_one_nobody_is_in` — the same decision a delivery flow makes, proved without a machine that has trees on it

## E. When `desktop/` changes

- `cd desktop && npm test` (the `Test Files` / `Tests` lines) and `npx tsc --noEmit`
- `cargo test --manifest-path desktop/src-tauri/Cargo.toml -j 1`
- `cargo test -p sailor -j 1 --test every_power_the_window_declares_is_one_it_can_reach`
- A walkthrough on the fixture store, with screenshots, for any change a person sees

## F. Before integration

- The branch's `HEAD` is recorded before a reviewer starts; the review names that commit; a different `HEAD` invalidates the review
- The combined trunk runs A plus every applicable letter, then the per-crate battery in a tab with line-by-line logging

## The six the trunk requires, and the day each said no

A required check that cannot refuse is worse than no check: the ruleset counts
it, the tree believes it, and the belief is free. Two of these six were exactly
that. `publication boundary` could not go green on any merge. `the style debt`
could not go red whatever it measured, and underneath that it was not
measuring: its clippy count had been zero on every run since it was written,
because the file sets `CARGO_TERM_COLOR: always` and no coloured line begins
`warning: `. One blindness had hidden the other for as long as both existed,
and neither was found by reading — the second surfaced only when a commit
built on purpose to be refused came back counted at zero.

So each ends in one of two states, never a third. **proved red**: a real commit
and the receipt of the run that concluded `failure` on it. **refused**: it
cannot be made red without touching the product, the reason is written, and a
fault number holds it. `scripts/gates-can-say-no.sh` reads this table, reads
the six from the ruleset itself, and exits 0 only when the two agree and every
receipt still holds on the forge.

The commits below are on no branch: they were written to be refused and their
branches were deleted the same day. The forge keeps them under the requests
that carried them, which is why the receipts can still be fetched.

| check | state | commit | receipt | what made it refuse |
| --- | --- | --- | --- | --- |
| `publication boundary` | proved red | `b24a7bab` | `105945273889` | an empty `.envrc`: a reserved path, which is a shape that needs no list |
| `sailor/private-names` | proved red | `b24a7bab` | `54512959547` | an empty `.envrc`: a reserved path, refused by the armed check that holds the list |
| `workspace tests` | proved red | `b24a7bab` | `105945273886` | a failing assertion |
| `desktop tests` | proved red | `b24a7bab` | `105945273862` | a failing assertion in the shell's own workspace, which `cargo test --workspace` never reaches |
| `clippy gate` | proved red | `b24a7bab` | `105945273758` | `approx_constant`, which is `clippy::correctness` and deny by default |
| `the style debt` | proved red | `01347a02` | `105946675134` | three blank lines where rustfmt allows one: 1676 places against a ceiling of 1675 |
