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
| 14 | 29/08 | Sailor halts flow runs with an ordinary fault when an engine reaches its weekly limit instead of handling quota exhaustion. | **closed in part** |
| 37 | 31/08 | `sailor flow cost` severely underreports run costs for handed-off steps by relying on self-reported turn counts that omit harness consumption. | **closed in part** |
| 94 | 05/09 | Flow steps declared with `data: private` refuse to run on shipped engines because the engines' undeclared data privacy pact defaults to unknown. | **closed in part** |
| 96 | 05/09 | Sailor fails flow steps with a JSON parse error instead of reporting an empty answer when an engine returns blank output. | **closed in part** |
| 104 | 06/09 | `sailor flow cost` reports run cost as unknown when an engine answers with a default model that is missing from the downloaded price list. | **open** |
| 116 | 06/09 | `sailor remaining` reports revoked credentials because it reads a stale on-disk file instead of the system keychain where the engine CLI stores active tokens. | **open** |
| 121 | 06/09 | Sailor's spend limiter undercounts the cost of multi-turn runs, so a run can spend past its cap | **closed in part** |
| 123 | 06/09 | The desktop interface fails to render flows containing newer condition types because its condition definitions are not kept in sync with the engine. | **open** |
| 124 | 06/09 | `sailor flow check` and several command-line outputs display messages in Italian instead of English. | **closed in part** |
| 126 | 06/09 | `sailor session list` reports dead terminals as open because it does not verify whether tracked terminal processes are still running. | **closed in part** |
| 132 | 07/09 | `sailor session attach` fails when invoked from non-interactive or piped environments unless the target terminal is explicitly specified with `--tty`. | **open** |
| 139 | 08/09 | Flow steps using `prefer: fuel` silently ignore engines whose quota queries fail instead of reporting that the quota channel is unreachable. | **open** |
| 142 | 08/09 | `sailor session list` overwrites existing terminal entries when an operating system reuses a tty, hiding earlier history from view. | **open** |
| 147 | 09/09 | Sailor fails to open `sessions.db` when running in directories that prevent creating auxiliary database files. | **open** |
| 149 | 09/09 | Flow steps fail when Sailor prepends default engines from its strengths table ahead of user-specified tools, selecting engines that lack write permissions. | **open** |
| 150 | 09/09 | `sailor notes show` fails to retrieve notes displayed by `sailor notes list` when called from a different checkout of the repository. | **open** |
| 151 | 10/09 | Sailor panics with an unhandled error instead of exiting cleanly when command-line arguments contain invalid UTF-8 byte sequences. | **open** |
| 158 | 11/09 | The shipped multi-agent flows fail immediately during execution due to schema mismatches and invalid JSON field paths between steps. | **open** |
| 159 | 11/09 | Querying a flow's last finished run silently reports that the run never completed instead of raising an error when given a malformed flow identifier. | **open** |
| 160 | 11/09 | `sailor profiles list` incorrectly displays profiles that authenticate via environment variables as unauthenticated because it only checks for on-disk files. | **open** |
| 161 | 11/09 | `sailor flow check` displays the engine chain declared in the flow definition rather than the effective chain that runs after default engines are prepended. | **open** |
| 163 | 11/09 | The event watcher launches duplicate flow runs on subsequent matching events instead of resuming an existing run that parked on a pending condition. | **open** |
| 164 | 11/09 | Sailor fails to pass spend caps to engine CLI processes, allowing individual engine calls to overrun the flow's budget before the limit is enforced. | **closed in part** |
| 165 | 11/09 | When an engine exhausts its quota, Sailor fails over to a different engine rather than rotating to another authenticated account of the same engine. | **open** |
| 169 | 2026-09-12 | `sailor remaining` erroneously reports that an account's token has expired when the engine is actually rate-limited or exhausted. | **open** |
| 172 | 2026-09-13 | `sailor profiles list` marks an engine profile as authenticated even though launching the engine still prompts for interactive login. | **open** |
| 179 | 2026-09-13 | Launching an engine with a newly initialized profile blocks on interactive trust dialogs and silently drops input typed before the prompt is ready. | **open** |
| 188 | 2026-09-14 | sailor flow list does not report when a local flow overrides a shipped flow, so background work stops and requests keep waiting without explanation. | **closed in part** |
| 189 | 2026-09-14 | sailor terminal mandate accepts engine values that no descriptor declares, causing deposited mandates to fail later when processed. | **open** |
| 190 | 14/09 | Flow steps are falsely marked as `engine_exhausted` when an engine echoes prompt text containing the word `quota` to standard error. | **open** |
| 192 | 13/09 | Authenticating a profile binds to whichever account is active in the browser, causing `sailor remaining` to report identical quotas across different profiles. | **open** |
| 193 | 14/09 | `sailor flow publish` rejects flows containing steps with a field named `key` by misinterpreting the field name as an exposed credential. | **open** |

**Thirty-two open faults are described on this page; thirty-two more are kept only in the fault store.**
