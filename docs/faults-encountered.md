# Faults still open

This page lists the known defects in Sailor that are not yet fully repaired,
one line each: when the fault was first met, what goes wrong, and where its
repair stands. The full register, with how each fault showed and what would
have prevented it, is kept in the project's own fault store, and this page is
rendered from it by `sailor faults render --open`. To report a new defect, open
an issue with the [bug report form](../.github/ISSUE_TEMPLATE/1_bug_report.yaml);
for a vulnerability, follow [`SECURITY.md`](../SECURITY.md) instead.

| # | since | what goes wrong | status |
|---|---|---|---|
| 4 | 29/08 | An orphaned development process was holding a port and blocking startup | **closed in part** on 31/08, reopened on 01/09 — the store has a `processes` table, every process Sailor starts goes through it (`supervisor::child::Process::start` is the only path, and it records), and `sailor-live --list` / `--stop` are the two halves that were missing. |
| 5 | 28/08 | Tests that read a configuration file belonging to the development machine running them | **closed in part** on 05/09: the tests no longer read the development machine. |
| 10 | 28/08 | The same list of components written in two places in the program | **closed in part** on 05/09: the two instances found unguarded are closed at the source. |
| 14 | 29/08 | An exhausted engine (weekly limit) was recorded as an ordinary fault, and the run stopped | **closed in part** on 31/08 — with a mutant. |
| 18 | 29/08 | Sailor's home (`~/.config/sailor`) contains **a single file**, a signature. | **closed in part** on 01/09 — the kit now **reaches the engines**: `actions` takes `profiles`, and `ExternalEngineAction` overlays the active profile's environment (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `GEMINI_CLI_HOME`) **under** `spec.env`. |
| 37 | 31/08 | **`sailor flow cost` lies by 4.3 times on a run with handed-off steps, and doesn't say it loud enough.** | **closed in part** on 01/09 — with mutant. |
| 38 | 31/08 | **SocratiCode's dependency graph gives false orphans on Rust, and we rested a decision on it.** | **open** |
| 42 | 01/09 | **A resource that every session writes and none watches over causes damage silently, and in a single day we saw four faces of it.** | **closed in part** — the two procedural defenses have been in force since 01/09. |
| 68 | 02/09 | **Functions and tests exist only because the tree is written in Italian** | **open** — named, not yet done; outside the MVP's scope by its own list |
| 72 | 2026-09-04 | A port hardwired by hand into a configuration file nobody checks made semantic search mute across the whole machine. | **open** — the hardwired line was not corrected but REMOVED, on 04/09/2026 at 13:48 by another session, which removed OLLAMA_MODE and OLLAMA_URL from the user's settings file: without them OLLAMA_MODE=auto comes back into force, which detects the native one on 11434 and otherwise falls back to the managed container. |
| 85 | 05/09 | **The repository had no lifecycle with the remote.** | **closed in part** — 56 branches deleted after the content check, five copies closed, the rule written in `AGENTS.md` («A branch's lifecycle, and how it's named»). |
| 94 | 05/09 | **A shipped flow declares `data: private` and names three engines, and none of the three can run it.** | **closed in part** on 05/09: the check exists. |
| 96 | 05/09 | **An engine sent out, invoked with the line its descriptor declares, answered nothing** | **closed in part** on 06/09 — half of Sailor is cured: an **empty or whitespace-only** answer now has its own class, `empty_answer`, and never passes as a success, whatever the exit code and with or without `answer_shape`. |
| 98 | 05/09 | **An engine sent to read the tree spent its whole window reading, and never answered.** | **open** — measured: 154,271 tokens with no answer against 14,401 with the answer, ten times less |
| 101 | 05/09 | **Five of six agent worktrees were born on history more than a thousand commits old.** | **open** |
| 103 | 06/09 | **An engine asked for a JSON answer writes into the strings the code it's quoting, quotes included, and the answer is no longer JSON.** | **open** |
| 104 | 06/09 | **The price list can't price the model that actually answered.** | **open** |
| 110 | 06/09 | **`sailor flow new`, inside Sailor's own tree, writes where a judge refuses.** | **open** |
| 116 | 06/09 | **The quota reader looks at the file, and on the development machine the token lives in the keychain.** | **open** |
| 121 | 06/09 | **The cap sums the bill Sailor calculates and ignores the one the engine declares, and on a multi-turn run the two differ by 3.3 times.** | **closed in part**, and with a correction: the cause written above was wrong. |
| 123 | 06/09 | **The window keeps its own copy of the flow condition language, and nobody compares the two.** | **open** |
| 124 | 06/09 | **The comments were translated and the sentences the tool prints were not: `sailor` still answers in Italian.** | **closed in part**. |
| 126 | 06/09 | **Sailor records a terminal and never asks whether it is still there, so seventeen are «open» and seven exist.** | **closed in part** — the window now reads the census (`terminals_abandoned`) and lists such a terminal among the things waiting, with a refusal answering as a refusal. |
| 127 | 2026-09-06 | The window was judged on the browser preview, where the workspace column can never fill. | **open** |
| 129 | 2026-09-06 | The guard that keeps a machine's private names out of a public repository opens published files and reads them as text. | **open** |
| 132 | 07/09 | `sailor session attach`, invoked from a tool call whose process holds no terminal, refused with «there is no telling which terminal this process runs on: none of its three descriptors is a tty, and no process above it on the parent chain has one either. | **open** |
| 134 | 07/09 | The cure for fault 4 was written and never called. | **closed in part** — the power is called now, by a command and by a flow at the beat. |
| 135 | 07/09 | **The rules that govern whoever works are documents, and a launched session is not a step.** | **open** |
| 136 | 07/09 | **Two sessions committed into one working tree, and one session's HEAD moved under it mid-work.** | **open** |
| 139 | 08/09 | crates/toolbox/src/resolver.rs:258, fn fuel: a failed quota read collapses to an empty Vec via .and_then(\|reading\| reading.result.ok())...unwrap_or_default(). | **open** — found and verified on 08/09, not yet fixed by anyone; not on tonight's critical path, left for the daylight session |
| 140 | 08/09 | Three lib tests go red on a loaded machine and green on an idle one, with no code change between: profiles_cmd::the_list_asks_the_engine_whether_each_home_has_credentials, profiles_cmd::two_homes_of_one_command_line_do_not_get_the_same_verdict, flow_cmd::engines::a_flow_check_says_which_home_the_engine_starts_from_and_whether_it_has_credentials. | **open** — found and verified on 08/09 while checking an unrelated change did not break the suite; the tests are correct, the cap is what is fragile |
| 141 | 08/09 | A session_id does not identify a process. | **open** — found and verified on 08/09 by a live authorship dispute between two sessions; the arbitration is done, the surfaces are unchanged |
| 142 | 08/09 | The `terminals` table is keyed by `tty`, so a tty handed out again overwrites the earlier row entire — worktree, session_id and closed_at included. | **open** — found on 08/09 while a second session was relying on an overwritten row; the store keeps the history, the surface does not offer it |
| 143 | 08/09 | An agent cannot tell, from its own context, an act it performed from one it merely read. | **open** — found on 08/09 by the session that made the mistake, which named it better than the one that caught it; no surface changed |
| 145 | 09/09 | **The note every session is told to read first was held three times, and the tree's reading answered the oldest without a word.** | **closed in part** on 09/09/2026 by 028d0907: the older body is named as older when read. |
| 146 | 09/09 | **The repair flow closed a round on the engine's word, never on the tree.** | **closed in part** on 09/09/2026 by bd467fdd on sources, proven by fixture, negative case, skipped case and a deliberate bypass; 03f4328d then moved the command that judges the repair to the launch text, and the binary in service is stamped 03f4328d since the same evening, so the contract is in service. |
| 147 | 09/09 | **The register of terminals does not open where the store does.** | **open** |
| 149 | 09/09 | **A step's kind put an engine in front of the tools the step named, and that engine cannot write.** | **open** |
| 150 | 09/09 | **A note the list shows is one the reader refuses, from another checkout of the same repository.** | **open** |
| 151 | 10/09 | **A mandate with one byte that is not text made the command panic instead of refusing.** | **open** |
| 155 | 2026-09-10 | A release refused because a judge that reads every source of every crate reported it had gone blind on one of them, and the release ran the whole suite in parallel. | **closed in part**: seen a second time on 10/09/2026, so it is not a one-off. |
| 157 | 2026-09-11 | The relay emptied a person's working session in the middle of an investigation. | **closed in part**: the flow is switched off on the development machine by a file of the same name in the person's own flows, so nothing is emptied. |
| 158 | 11/09 | The two shipped flows that put a crew of agents to work — «put-the-crew-to-work» and its child «one-agent-on-one-task» — could never have run. | **open** — worked around in the local copies «la-squadra-sulla-finestra» and «un-agente-su-un-compito»; the shipped pair is still broken |
| 159 | 11/09 | history_ask's last_run query silently reports a run as never-closed when the identifier it is given is malformed, instead of saying it cannot look. | **open** |
| 160 | 11/09 | A profile that authenticates by a key in a variable is reported NOT AUTHENTICATED. | **open** — the endpoint profile works and only its reading is wrong |
| 161 | 11/09 | A step's engine chain is not the chain that runs. | **open** — the step can now refuse the table, but `flow check` still prints the declared chain rather than the one that will run |
| 163 | 11/09 | The reflex arc starts a new run on every matching event and never resumes the one it parked. | **open** — two of the three halves are in. |
| 164 | 11/09 | A run's spend cap never reached the command line, so it could only ever stop the call after the one that went over. | **closed in part** — cured for the engines that declare `native_spend_cap`, which on the development machine is the one that answers most calls. |
| 165 | 11/09 | A machine with seven accounts runs every call on two of them. | **open** — the road exists: a chain entry written `engine@account` runs that call on that account, an account the machine does not have refuses the call instead of falling back, and a spent quota is set aside per account rather than per engine. |
| 168 | 11/09 | The guard that keeps a machine's private names out of the public repository has never once guarded a push. | **closed in part** — the push is guarded. |
| 169 | 2026-09-12 | The quota reader turned a stale access token into a claim about the account, and the claim was false in both directions. | **open** |
| 172 | 2026-09-13 | **Authenticated is not invocable.** | **open** |
| 179 | 2026-09-13 | **A fresh engine home answers three dialogs before it takes input.** | **open** |
| 186 | 2026-09-13 | **A work branch could move backwards at the moment its terminal changed hands.** | **closed in part** — the hook is early feedback, while pins preserve the delivered commits when a process bypasses it. |
| 187 | 2026-09-14 | **The window's beat, opened on a scratch ledger, took down a worktree a live terminal was working in.** | **open** |
| 188 | 13/09 | A profile's home re-authorised the account a browser already had open during login, and `sailor profiles list` kept saying "authenticated" — true of the home, not of the account the profile is named for. | **open** |
| 189 | 14/09 | sailor flow publish refuses a flow whose store_write step carries a field literally named «key» (the store row identifier, e.g. | **open** — reported by the lab agent, hands off to whoever fixes publish_cmd.rs |
| 190 | 14/09 | **An engine that echoes its prompt is judged exhausted by the words of that prompt.** | **open** |

**Fifty-eight faults are still open.**
