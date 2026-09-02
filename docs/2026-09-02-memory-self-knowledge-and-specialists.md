# Memory, self-knowledge and specialists: how the others do it in 2026

**02/09/2026.** Research by a fresh-context agent with web access, on Theo's
words of the same night: *the product must study its memory system: the
database we built is not a RAG; maybe the AI should search and understand the
flows, or develop them, and save memories. With terminals and agents of many
kinds it is a system that knows itself very little. Claude sessions talk to
each other, but agents of different command lines working on one goal do not
know who does what.* And: *it cannot be that the general-purpose agent is
always the one launched; a project forces the AI to call specialised agents.*
Primary sources, read on 02/09/2026; star counts of that day; what could not
be verified is marked.

## Outcome, in one line

None of the most-used memory systems for *coding* agents in 2026 uses a
vector store as its first layer: Claude Code, Codex and Gemini CLI write
**Markdown files with a short index loaded at start and detail files read on
demand**; search over the repository is **grep or full-text plus an explorer
subagent**, and Amazon's paper measures agentic keyword search at 91–95 % of
RAG. Sailor's SQLite ledger is enough: it lacks a `memories` table with type,
provenance and `modified`, an FTS5 index, one generated file for the three
command lines, and a periodic consolidation flow. For «who does what», no
cross-CLI project solves it without a service; the lease already built is the
right form and only needs a **shared vocabulary** (OpenTelemetry, A2A) and to
be **injected at every agent's start**. For «force the specialists», the hard
mechanism is the platform's own (`tools: Agent(a, b)` without Edit or Write,
plus a PreToolUse hook exiting 2); the famous projects almost all do it by
prompt.

## 1. Memory for coding agents, by adoption

| project | what it keeps, how it reads and writes | vector store | source |
|---|---|---|---|
| everything-claude-code (246k, MIT) | «instincts» with a confidence score; Markdown memory under `.ecc/memory/`; SessionStart and SessionEnd hooks load and save | no | https://github.com/affaan-m/everything-claude-code |
| claude-mem (93k, Apache-2) | five hooks turn tool use into «observations» in SQLite with FTS5, **plus Chroma** and an always-on HTTP worker with a viewer | hybrid; **resident worker** | https://github.com/thedotmack/claude-mem |
| OpenHands (86k) | a condenser (`LLMSummarizingCondenser(max_size, keep_first)`); condensation is **a visible event in the stream** (PR #7311) | no | https://docs.openhands.dev/sdk/guides/context-condenser |
| mem0 (65k) | extraction and update with ADD, UPDATE, DELETE decisions; −91 % p95 latency against full context | **mandatory** (Qdrant or pgvector) | https://arxiv.org/abs/2504.19413 · https://docs.mem0.ai/open-source/configuration |
| Claude Code auto memory | four types (`user`, `feedback`, `project`, `reference`) in front matter; `MEMORY.md` index **loaded for its first 200 lines or 25 KB**, topic files on demand; `modified` ISO field; per repository, shared across worktrees; «Claude skips anything it can derive from the codebase»; memories are context, not enforced configuration — to block an action use a PreToolUse hook | no | https://code.claude.com/docs/en/memory |
| Codex memories | `~/.codex/memories/` with summaries, durable entries, recent inputs, evidence; extraction and consolidation in the background only when the chat is idle; secrets redacted; keys `memories.generate_memories`, `use_memories`, `min_rate_limit_remaining_percent`, `extract_model`, `consolidation_model`; «treat these files as generated state» | no | https://learn.chatgpt.com/docs/customization/memories (release date and 30-day pruning: **secondary sources only**) |
| Gemini CLI | `save_memory` edits Markdown (`~/.gemini/GEMINI.md`, repository `GEMINI.md`, a private folder per project); concatenation only; `/memory show` | no | https://geminicli.com/docs/tools/memory/ |
| Aider (49k) | repo map: tree-sitter symbols ranked on a graph, a `--map-tokens` budget of 1k | no | https://aider.chat/docs/repomap.html |
| Zep, Graphiti (31k) | three subgraphs (episodes, entities, communities); **bi-temporal** `t_valid`, `t_invalid`, `t_created`, `t_expired`; a contradiction sets `t_invalid`; retrieval by cosine, **BM25** and BFS, fused with RRF or MMR | yes, plus a graph DB | https://arxiv.org/abs/2501.13956 |
| Letta (25k) | a memory block is `label, description, value, limit, read_only`, always in context, shareable across agents, last write wins; archival is a semantic DB | blocks no; archival yes | https://docs.letta.com/guides/agents/memory-blocks |
| MemOS (11k) | MemCube with provenance and versioning; plaintext, activation, parametric | yes | https://arxiv.org/abs/2507.03724 |
| LangGraph store | namespace, key, JSON; `put`, `get`, `search`; semantic search needs `IndexConfig(embed=…)` | optional; filtering without embeddings **not verified** | https://docs.langchain.com/oss/python/langchain/long-term-memory |
| Devin Knowledge | an entry is a trigger description, content and the repositories it applies to; Devin proposes it from feedback, a person approves it | not declared | https://docs.devin.ai/product-guides/knowledge |
| A-MEM (1.2k) | notes with keywords, tags, context, links; old notes evolve | yes | https://arxiv.org/abs/2502.12110 |
| Cursor memories | per project, in Settings › Rules, beta | — | https://cursor.com/changelog/1-0 (sidecar model and approval: **secondary only**) |
| Anthropic «dreaming» | three-phase consolidation with a reviewable diff | — | **not verified**: no page on anthropic.com nor in the docs index; third-party blogs only |

## 2. Self-knowledge: who is doing what, across processes and command lines

| project | concrete data model | source |
|---|---|---|
| Claude Code agent teams (experimental) | `~/.claude/teams/{team}/config.json` with `members[]` (name, agent id, agent type; the lead is `team-lead`; «teammates can read this file to discover other team members»); mailboxes `inboxes/{agent}.json`; `~/.claude/tasks/{team}/` persists; states pending, in progress, completed with dependencies; «task claiming uses file locking»; hooks `TeammateIdle`, `TaskCreated`, `TaskCompleted` block with exit 2. One team per session, a fixed lead, **Claude only** | https://code.claude.com/docs/en/agent-teams |
| beads and Gas Town (27k, 18k, MIT) | issues in embedded **Dolt** (`issues.jsonl` is only an export); `bd ready` (no open blocker), `bd update <id> --claim` «atomically claim a task»; Gas Town: a Mayor, polecats with «persistent identity but ephemeral sessions», convoys; Claude, Copilot, Codex, Gemini, Cursor | https://github.com/steveyegge/beads · https://github.com/steveyegge/gastown |
| OpenAI Symphony (27k, Apache-2, 27/04/2026) | the tracker is the control plane: eligible if `state in active_states and not in terminal_states`; one workspace per issue reused; `WORKFLOW.md`; `max_concurrent_agents` 10; a stall of five minutes terminates and retries with exponential backoff; «without requiring a persistent database», but it is a polling service | https://github.com/openai/symphony/blob/main/SPEC.md |
| OpenTelemetry GenAI agent spans | `invoke_agent`, `create_agent`; `gen_ai.operation.name`, `gen_ai.provider.name` required; `gen_ai.agent.name/id/description`, `gen_ai.conversation.id` conditional; **all in Development status** | https://github.com/open-telemetry/semantic-conventions-genai/blob/main/docs/gen-ai/gen-ai-agent-spans.md |
| Langfuse (34k) | observations, traces, sessions; the agent graph is **inferred from nesting and timing**; a server | https://langfuse.com/docs/observability/features/agent-graphs |
| A2A 1.0 | an agent card (`name, description, url, skills[], capabilities, version, provider`); task states `SUBMITTED, WORKING, INPUT_REQUIRED, AUTH_REQUIRED, COMPLETED, FAILED, CANCELED, REJECTED` | https://a2a-protocol.org/latest/specification/ |
| Temporal visibility | `WorkflowType, ExecutionStatus, StartTime` and custom attributes; SQL-like filters; a server | https://docs.temporal.io/visibility |
| claude_code_agent_farm (916) | `active_work_registry.json`, `agent_locks/{agent}_{ts}.lock`, `completed_work_log.json`, `planned_work_queue.json`; stale lock after two hours; a heartbeat file; «implemented entirely by means of the prompt file» | https://github.com/Dicklesworthstone/claude_code_agent_farm |
| claude-squad, vibe-kanban, Crystal | tmux and a worktree per instance, **no coordination between instances**; vibe-kanban is sunsetting and needs a server | https://github.com/smtg-ai/claude-squad · https://github.com/BloopAI/vibe-kanban |
| oh-my-claudecode state | `.omc/state/sessions/{id}/`, `.omc/state/team/<name>/`, `agent-replay-*.jsonl` | https://github.com/Yeachan-Heo/oh-my-claudecode/blob/main/docs/REFERENCE.md |

No source covers several command lines **without** a service (Symphony, Gas
Town on Dolt, Langfuse, Temporal) or without staying inside Claude (agent
teams, cross-session messaging).

## 3. Forcing the specialists

| project | mechanism | does it enforce | source |
|---|---|---|---|
| Superpowers (281k, MIT) | a session-start hook injects `using-superpowers`; the skill `subagent-driven-development` dispatches a fresh subagent per task with two-stage review | no: «mandatory workflows» by prompt | https://github.com/obra/superpowers |
| everything-claude-code (246k) | 68 agents, 286 skills, hooks «can enforce deterministic checks outside the prompt» | delegation not forced | https://github.com/affaan-m/everything-claude-code |
| ruflo, formerly claude-flow (70k, MIT) | a router at «89 % accuracy», queen-led hierarchy, 27 hooks, optional MCP server, HNSW memory | by orchestrator; **vector memory** | https://github.com/ruvnet/ruflo |
| BMAD (53k, MIT) | persona agents and workflows through `bmad-*` commands | no | https://github.com/bmad-code-org/BMAD-METHOD |
| oh-my-claudecode (39k, MIT) | 29 agents in three tiers (Haiku, Sonnet, Opus), 21 hooks on 11 events (`pre-tool-enforcer.mjs`, `persistent-mode.mjs` on Stop); **«the orchestrator never implements»** | the rule is a prompt; no hook forbids Edit or Write to the main agent | https://github.com/Yeachan-Heo/oh-my-claudecode |
| wshobson/agents (39k, MIT) | 94 plugins, 202 agents, 183 skills; tiers Fable, Opus, Sonnet, Haiku | delegation by `description` | https://github.com/wshobson/agents |
| compound-engineering, VoltAgent, SuperClaude, PAI, Agent OS | agent files with front matter, commands, hooks, standards | none enforces | the respective repositories |
| barkain/claude-code-workflow-orchestration (83) | a PreToolUse hook on Bash, Edit, Write emits a **growing stderr** when the main agent does not delegate | «no hard blocks» | https://github.com/barkain/claude-code-workflow-orchestration |

**The hard mechanism belongs to the platform, not to the plugins.** In a
Claude Code agent definition, `tools: Agent(worker, researcher), Read, Bash`
is an allow list: «if the agent tries to spawn any other type, the request
fails», and «if you omit `Agent` entirely, the agent cannot spawn any
subagents»; it holds for `claude --agent`. A PreToolUse hook with exit 2
«blocks whether or not you print JSON». Sources:
https://code.claude.com/docs/en/sub-agents and https://code.claude.com/docs/en/hooks.
The «beautiful project» Theo remembers is in all likelihood
**oh-my-claudecode** («the orchestrator never implements», tiers of agents) or
**Superpowers**. For Codex and Gemini no documented equivalent of the hard
block was found (not verified).

## 4. Searching flows and knowledge: structured against embeddings

- **Claude Code** has no index: Glob, Grep, Read and an Explore subagent.
  «Early versions of Claude Code used RAG and a local vector db, but we found
  pretty quickly that agentic search generally works better» (a secondary
  quotation, https://vadim.blog/claude-code-no-indexing/). **Cursor** in 2026
  keeps both: a Merkle tree and syntactic chunks embedded asynchronously
  (https://cursor.com/blog/secure-codebase-indexing, 27/01/2026) and «Instant
  Grep» with an Explore subagent (https://cursor.com/docs/context/codebase-indexing).
- **The measure.** A ReAct agent with `pdfgrep`-class tools against a
  managed RAG: faithfulness 94.5 %, context recall 88.1 %, answer correctness
  91.5 % of RAG; on FinanceBench it **beats** RAG (30.4 % against 24.2 %);
  «particularly useful in scenarios requiring frequent updates».
  https://arxiv.org/abs/2602.23368
- **Hybrid in one file.** Zep uses BM25 beside cosine; the SQLite FTS5 plus
  sqlite-vec plus RRF pattern (vstash, https://arxiv.org/abs/2604.15484).
  **SocratiCode**, used on this repository: Qdrant in Docker, AST chunks,
  semantic plus BM25 fused with RRF; its «61 % less context, 84 % fewer calls»
  is self-declared. https://github.com/giancarloerra/socraticode
- Sourcegraph Cody and Continue: pages unreachable, **not verified**.

Structured search wins on exact identifiers, on a small corpus that changes
often, and on relational questions («which flows use `work_claim`», «which
step fails most»): that is SQL, not similarity.

## What Sailor takes, and what it must not

1. **A `memories` table in the ledger** (the `store` exists): `type` in
   {user, feedback, project, reference} as Claude Code; `label, value, limit,
   read_only` as Letta; `modified` ISO; **provenance** `run_id, step_id,
   session` as MemCube; `t_valid, t_invalid` as Zep instead of deleting.
   Written by a step of surface `remember` (decision of 31/08). **No vector
   store.**
2. **A short index plus detail on demand.** A `MEMORY.md`-equivalent of at
   most 200 lines or 25 KB, generated from the ledger and **injected into all
   three command lines from one file**: Claude imports `@AGENTS.md`, Codex
   reads `AGENTS.md`, Gemini through `context.fileName`
   (https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/gemini-md.md).
   Sailor already launches the processes with `launch.env`. No vector store.
3. **FTS5 over `runs`, `steps`, `events`, `faults`, `store` and the text of
   the `.flow.json` files** (id, actions, descriptions), plus relational SQL
   views. It is the «interrogable ledger» item. Embeddings only if one day
   the count passes the thousands, with sqlite-vec in the same file; **not
   now**.
4. **Consolidation as a periodic flow, not a daemon**: raw to summary, with a
   **reviewable diff** and the states `deprecated` and `to be re-decided`
   taken by a person (Codex's «generated state»; decision of 29/08). Secrets
   redacted before the first write, as Codex does. No vector store.
5. **The lease becomes the local agent card.** Add to `work_claim` and to the
   census the OpenTelemetry names (`gen_ai.agent.name/id`,
   `gen_ai.conversation.id`, `gen_ai.operation.name`) and the A2A states
   (`working, input_required, completed, failed`) as columns, so a future
   export is free; agent teams' `members[]` is the reading format for
   agents. The lease renewal is already the heartbeat: neither the agent
   farm's heartbeat file nor Symphony's stall timeout is needed.
6. **Inject `work_survey` at every agent's start**: a SessionStart hook for
   Claude (`additionalContext`) and, for Codex and Gemini, a line in the
   generated file of item 2. It closes «agents of different command lines do
   not know who does what» without a port.
7. **Force the specialists.** Generate `.claude/agents/*.md` from Sailor's
   descriptors; the coordinator starts as `claude --agent` with `tools:
   Agent(<list>), Read, Bash` and **without Edit or Write**, plus a PreToolUse
   hook exiting 2 when the flow declares `gate` (as `refuse_when_shared`:
   first a warning, then a wall). For Codex and Gemini it stays uncovered
   and is declared so.
8. **Not to take**: claude-mem (an always-on HTTP worker plus Chroma), ruflo
   (an MCP server plus HNSW), SocratiCode as a dependency (Docker plus
   Qdrant), mem0 (a mandatory vector store), Zep (a graph DB), Langfuse,
   Temporal, Symphony and vibe-kanban (services), beads (Dolt). No source
   shows an always-on service to be unavoidable on one machine: Claude Code,
   Codex and Gemini do everything with files, and Symphony rebuilds its
   state by re-reading the tracker, which for Sailor is the ledger.

Not verified, in short: Anthropic's «dreaming»; the release date and 30-day
pruning of Codex memories; Cursor's sidecar; filtering without embeddings in
LangGraph; the current state of Sourcegraph and Continue; a hard block on
delegation in Codex or Gemini.
