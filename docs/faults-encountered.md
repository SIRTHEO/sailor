# Faults still open

This page lists known defects in Sailor that are not yet fully repaired and
have a summary written for users, one line each: when the fault was first met,
what goes wrong, and where its repair stands. The full register, with how each
fault showed and what would have prevented it, is kept in the project's own
fault store, and this page is rendered from it by `sailor faults render --open`. To report a new defect, open
an issue with the [bug report form](../.github/ISSUE_TEMPLATE/1_bug_report.yaml);
for a vulnerability, follow [`SECURITY.md`](../SECURITY.md) instead.

| # | since | what goes wrong | status |
|---|---|---|---|
| 14 | 29/08 | Sailor halts flow runs with an ordinary fault when an engine reaches its weekly limit instead of handling quota exhaustion. | **closed in part** on 31/08 — with a mutant. |
| 37 | 31/08 | `sailor flow cost` severely underreports run costs for handed-off steps by relying on self-reported turn counts that omit harness consumption. | **closed in part** on 01/09 — with mutant. |
| 94 | 05/09 | Flow steps declared with `data: private` refuse to run on shipped engines because the engines' undeclared data privacy pact defaults to unknown. | **closed in part** on 05/09: the check exists. |
| 96 | 05/09 | Sailor fails flow steps with a JSON parse error instead of reporting an empty answer when an engine returns blank output. | **closed in part** on 06/09 — half of Sailor is cured: an **empty or whitespace-only** answer now has its own class, `empty_answer`, and never passes as a success, whatever the exit code and with or without `answer_shape`. |
| 104 | 06/09 | `sailor flow cost` reports run cost as unknown when an engine answers with a default model that is missing from the downloaded price list. | **open** |
| 116 | 06/09 | `sailor remaining` reports revoked credentials because it reads a stale on-disk file instead of the system keychain where the engine CLI stores active tokens. | **open** |
| 121 | 06/09 | Sailor's spend limiter undercounts multi-turn run costs because it calculates expenditure internally instead of reading the engine's declared cost. | **closed in part**, and with a correction: the cause written above was wrong. |
| 123 | 06/09 | The desktop interface fails to render flows containing newer condition types because its condition definitions are not kept in sync with the engine. | **open** |
| 124 | 06/09 | `sailor flow check` and several command-line outputs display messages in Italian instead of English. | **closed in part**. |
| 126 | 06/09 | `sailor session list` reports dead terminals as open because it does not verify whether tracked terminal processes are still running. | **closed in part** — the window now reads the census (`terminals_abandoned`) and lists such a terminal among the things waiting, with a refusal answering as a refusal. |
| 132 | 07/09 | `sailor session attach` fails when invoked from non-interactive or piped environments unless the target terminal is explicitly specified with `--tty`. | **open** |
| 139 | 08/09 | Flow steps using `prefer: fuel` silently ignore engines whose quota queries fail instead of reporting that the quota channel is unreachable. | **open** — found and verified on 08/09, not yet fixed by anyone; not on tonight's critical path, left for the daylight session |
| 142 | 08/09 | `sailor session list` overwrites existing terminal entries when an operating system reuses a tty, hiding earlier history from view. | **open** — found on 08/09 while a second session was relying on an overwritten row; the store keeps the history, the surface does not offer it |
| 147 | 09/09 | Sailor fails to open `sessions.db` when running in directories that prevent creating auxiliary database files. | **open** |
| 149 | 09/09 | Flow steps fail when Sailor prepends default engines from its strengths table ahead of user-specified tools, selecting engines that lack write permissions. | **open** |
| 150 | 09/09 | `sailor notes show` fails to retrieve notes displayed by `sailor notes list` when called from a different checkout of the repository. | **open** |
| 151 | 10/09 | Sailor panics with an unhandled error instead of exiting cleanly when command-line arguments contain invalid UTF-8 byte sequences. | **open** |
| 158 | 11/09 | The shipped multi-agent flows fail immediately during execution due to schema mismatches and invalid JSON field paths between steps. | **open** — worked around in the local copies «la-squadra-sulla-finestra» and «un-agente-su-un-compito»; the shipped pair is still broken |
| 159 | 11/09 | Querying a flow's last finished run silently reports that the run never completed instead of raising an error when given a malformed flow identifier. | **open** |
| 160 | 11/09 | `sailor profiles list` incorrectly displays profiles that authenticate via environment variables as unauthenticated because it only checks for on-disk files. | **open** — the endpoint profile works and only its reading is wrong |
| 161 | 11/09 | `sailor flow check` displays the engine chain declared in the flow definition rather than the effective chain that runs after default engines are prepended. | **open** — the step can now refuse the table, but `flow check` still prints the declared chain rather than the one that will run |
| 163 | 11/09 | The event watcher launches duplicate flow runs on subsequent matching events instead of resuming an existing run that parked on a pending condition. | **open** — two of the three halves are in. |
| 164 | 11/09 | Sailor fails to pass spend caps to engine CLI processes, allowing individual engine calls to overrun the flow's budget before the limit is enforced. | **closed in part** — cured for the engines that declare `native_spend_cap`, which on the development machine is the one that answers most calls. |
| 165 | 11/09 | When an engine exhausts its quota, Sailor fails over to a different engine rather than rotating to another authenticated account of the same engine. | **open** — the road exists: a chain entry written `engine@account` runs that call on that account, an account the machine does not have refuses the call instead of falling back, and a spent quota is set aside per account rather than per engine. |
| 169 | 2026-09-12 | `sailor remaining` erroneously reports that an account's token has expired when the engine is actually rate-limited or exhausted. | **open** |
| 172 | 2026-09-13 | `sailor profiles list` marks an engine profile as authenticated even though launching the engine still prompts for interactive login. | **open** |
| 179 | 2026-09-13 | Launching an engine with a newly initialized profile blocks on interactive trust dialogs and silently drops input typed before the prompt is ready. | **open** |
| 188 | 13/09 | Authenticating a profile binds to whichever account is active in the browser, causing `sailor remaining` to report identical quotas across different profiles. | **open** |
| 189 | 14/09 | `sailor flow publish` rejects flows containing steps with a field named `key` by misinterpreting the field name as an exposed credential. | **open** — reported by the lab agent, hands off to whoever fixes publish_cmd.rs |
| 190 | 14/09 | Flow steps are falsely marked as `engine_exhausted` when an engine echoes prompt text containing the word `quota` to standard error. | **open** |

**Thirty open faults are described on this page; twenty-nine more are kept only in the fault store.**
