# How others run on many engines, and what each provider really gives

**02/09/2026.** Research by a fresh-context agent with web access, on the
question Theo put that evening: *how do the most-used projects of 2026 use many
AIs, not to dodge limits but to use the tokens the providers give, and how do
products heal and develop themselves.* Primary sources only (repositories,
documentation, release notes), read on 02/09/2026; star counts are of that
day. What could not be verified is marked.

## Outcome

None of the projects looked at «exploits free tiers» with a mechanism Sailor
does not already hold in embryo: they do **catalogue + check before the call +
ordered fallback + cooldown by error class + a register**. Three things are new
against what this repository knew on 31/08:

1. **Codex has a machine channel for its quota**: `account/rateLimits/read`
   on the app-server answers `usedPercent`, `windowDurationMins` (300 or
   10080), `resetsAt`, `planType`, `credits` (issue #24080, 22/05/2026).
2. **Gemini CLI's free path through Google login ended on 18/06/2026**
   (migration to Antigravity); what is left is the free API key at 250
   requests a day, Flash only.
3. **Anthropic wrote the rule out**: the subscription's OAuth is valid **only
   inside the unmodified `claude` binary**, also when a platform launches it —
   which is exactly Sailor's case — and it forbids passing requests through a
   proxy of one's own. The «local point in the middle» of 29/08 holds for
   engines with a key, not for subscription command lines. The monthly Agent
   SDK credit is suspended since 15/06/2026: `claude -p`, the SDK and GitHub
   Actions draw on the subscription.

## 1. How the others use many providers, by use

1. **OpenClaw** (388k stars). `model-failover`: first rotates the
   *authentication profiles* of the same provider (OAuth, then token, then key,
   then least used), then moves to the next model in
   `agents.defaults.model.fallbacks`. Cooldown by class: rate-limit and
   overload 30 s, 1 m, 5 m; billing sets a long `disabledUntil`; revoked auth
   disables with faster recovery. The profile is **pinned to the session** to
   keep the cache. State in SQLite: `lastUsed, cooldownUntil, errorCount,
   disabledUntil, disabledReason`. https://docs.openclaw.ai/concepts/model-failover
   — Anthropic blocked exactly this use of subscription OAuth on 04/04/2026
   (policy written 19–20/02/2026).
2. **cc-switch** (130.7k, MIT, Rust and Tauri). Writes the configuration
   files of eight command lines under `~/.cc-switch/`, a local proxy with
   auto-failover, circuit breaker, health monitoring, token and request
   accounting, per-application takeover. It also **translates formats**, the
   part refused on 30/08. https://github.com/farion1231/cc-switch
3. **OpenCode Zen** (opencode 203k, MIT). Its own gateway; seven models free
   «for a limited time», with a **data pact per model**: Nemotron logs «to
   improve NVIDIA products», Muse Spark «trains Meta models». No rotation
   documented. https://opencode.ai/docs/zen/
4. **Kilo Gateway** (27k, MIT). `kilo-auto/free` routes «to the best free
   model available»; the document warns it **may route to providers that log
   prompts and outputs**. https://kilo.ai/docs/getting-started/using-kilo-for-free
5. **Claude Code Router** (37k, MIT). Gateway on `127.0.0.1:3456`, credential
   pool, key rotation, ordered fallbacks, client keys with local request and
   token limits, a dashboard with resolved provider, model, credential, tokens
   and estimated cost. https://github.com/musistudio/claude-code-router
6. **Cloudflare AI Gateway, dynamic routing** (doc of 07/08/2026). **Rate
   Limit** nodes (requests per key per period) and **Budget Limit** nodes
   (cost per key per period) that divert to a fallback when exceeded;
   conditional, percentage and model nodes.
   https://developers.cloudflare.com/ai-gateway/features/dynamic-routing/
7. **Bifrost** (7.8k, Apache-2.0). Hierarchical budgets customer, team,
   virtual key, provider with `max_limit`, `reset_duration` (1m to 1Y),
   `calendar_aligned`; **cumulative check before the call, all must pass**;
   afterwards records real tokens and cost. https://docs.getbifrost.ai/features/governance/budget-and-limits
8. **Portkey gateway** (12.9k, MIT). `strategy.mode: fallback`,
   `on_status_codes: [429, 503]`, ordered `targets`.
   https://portkey.ai/docs/product/ai-gateway/fallbacks
9. **Vercel AI Gateway** (doc of 28/07/2026).
   `providerOptions.gateway.order/only/sort` with `sort: cost|ttft|tps`; bring
   your own key tried first, then the system credentials on credits; **BYOK
   spend does not count against budgets**.
   https://vercel.com/docs/ai-gateway/models-and-providers/provider-options
10. **Roo Code** (24k): rate limit **per profile** since 3.39.0 (08/01/2026);
    automatic fallback across profiles is an open request (#7068). **Cline**
    (67k): only `autoRetry` on 429; fallback is done by OmniRoute or 9router
    in front. **Goose** (53.9k): lead and worker through variables, a turn
    threshold, return to the lead on failure; issue #4036 *generalises* it,
    does not remove it — the sentence «removed it» in the document of 31/08
    could not be confirmed. **OpenHands** (86k): retry with backoff,
    tokens, cost and latency per call in the event stream,
    `max_budget_per_task`; no provider fallback. **Aider** (48.7k): no
    fallback.
11. **Copilot CLI** (11k). `-p` non-interactive; since 06/2026 it consumes AI
    credits per token (1 credit = 0.01 $).
    https://docs.github.com/copilot/concepts/agents/about-copilot-cli

## 2. What each provider gives, with its limit and its pact

Verified on a primary source unless noted.

| provider | path | limit | reading the quota | data pact |
|---|---|---|---|---|
| Claude Pro / Max | unmodified `claude`, own login | 5-hour window plus weekly, shared with Claude.ai | `rate_limits` (known) | OAuth only in the binary and native apps; Agent SDK monthly credit **suspended** since 15/06/2026. https://code.claude.com/docs/en/legal-and-compliance |
| Codex with ChatGPT | Free / Go / Plus / Pro | 5-hour window shared local and cloud, weekly «may apply» | `/status`; app-server `account/rateLimits/read` | «no training» stated only for Business and Enterprise; consumer not found. https://learn.chatgpt.com/docs/pricing |
| Gemini CLI | Google login **ended 18/06/2026**; free API key | 250 requests a day, **Flash only**; Standard 1500 and Enterprise 2000 are paid | `/stats model`, interactive; issues #15416 and #17081: disagrees with the server | not stated on the quota page. https://geminicli.com/docs/resources/quota-and-pricing/ |
| Copilot | Free plan | 50 chats a month, 2000 completions; Pro 10 $ gives 15 $ of credits | not found | not searched. https://github.com/features/copilot/plans |
| OpenRouter `:free` | key | 20 RPM; 50 RPD under 10 $ ever paid, 1000 RPD above | `GET /api/v1/key`: `limit_remaining, usage_daily/weekly/monthly, is_free_tier` | account setting «do not route to who trains», **separate for free and paid**; Laguna, Liquid and Muse Spark declare training. https://openrouter.ai/docs/api-reference/limits |
| Groq | free key | per model: gpt-oss-20b 30 RPM, 1K RPD, 8K TPM, 200K TPD; organisation level | `x-ratelimit-remaining-requests/-tokens`, `retry-after`; cache does not count | «we do not train on data» found on one model page only, weak. https://console.groq.com/docs/rate-limits |
| Cerebras | **needs a payment method** to activate; 5 $ expiring in 30 days | gpt-oss-120b and gemma-4-31b: 5 RPM, 30K TPM, **1M TPD** | not documented | not found. The «1M a day without a card» of blogs is **contradicted** by the docs. https://inference-docs.cerebras.ai/support/rate-limits |
| SambaNova | Free / Developer | Free 20 RPM, 20 RPD, 200K TPD; Developer 20M TPD, 60–240 RPM | `x-ratelimit-remaining-requests(-day)` and reset | not found. https://docs.sambanova.ai/docs/en/models/rate-limits |
| Mistral Experiment | key, phone verification | numbers only in the admin panel; «~1B tokens a month, 1 rps» is from blogs | panel | **trains by default on the free tier**, opt-out available. https://help.mistral.ai/en/articles/347617 |
| Ollama Cloud | account | 1 concurrent request, monthly «starting credits» on small models | not found | «prompts or responses never logged nor used to train». https://ollama.com/pricing |
| Hugging Face | HF token routed | 0.10 $ a month (PRO 2 $); stops when spent | usage page | the downstream provider's pact not reported. https://huggingface.co/docs/inference-providers/pricing |
| NVIDIA NIM | build.nvidia.com | ~40 RPM, 1000 credits | none | **forum only**, no official doc found |
| DeepSeek, Together | none permanent; 5M tokens once, 5 $ at sign-up | none | none | **not verified**, secondary sources |

## 3. Loops that heal themselves

1. **ralph-wiggum** (official Anthropic plugin). A `Stop` hook blocks the exit
   and re-sends the same prompt; it stops on `<promise>STRING</promise>` (exact
   match, one possible outcome), `--max-iterations`, `/cancel-ralph`; state is
   files and git. https://github.com/anthropics/claude-code/tree/main/plugins/ralph-wiggum
2. **autoresearch** (95k, 07/03/2026). Per turn: edit `train.py`, commit, a
   run on a **fixed budget of 5 minutes**, `grep val_bpb`, a line in
   `results.tsv` (`commit, val_bpb, memory_gb, status keep|discard|crash,
   description`); keep if better, else `git reset`; 10-minute timeout is a
   failure; «NEVER STOP»; a person writes `program.md`.
   https://github.com/karpathy/autoresearch/blob/master/program.md
3. **Claude Code hooks**. Thirty events; `Stop` exit 2 continues;
   `StopFailure` carries `rate_limit|overloaded|billing_error` but is
   informative only; `SubagentStop` filterable by type; **no hook reports the
   quota**. https://code.claude.com/docs/en/hooks
4. **claude-code-action** (8.8k). `@claude`, assignment, `schedule`,
   `workflow_dispatch`; `max_turns`, `timeout_minutes`; a progress comment
   with checkboxes; validated JSON output; auth **only** by API key, Bedrock,
   Vertex or Foundry. https://github.com/anthropics/claude-code-action
5. **Copilot cloud agent**. Ephemeral Actions environment, a **hard 59
   minutes** with warnings to wrap up, one branch and one PR per task, a
   session log, an `Agent-Logs-Url` trailer in commits (changelog
   20/03/2026), «Fix with Copilot» on a failed workflow opens a new session.
   https://docs.github.com/en/copilot/concepts/agents/cloud-agent/about-cloud-agent
6. **gh-aw** (5.1k, MIT, preview 13/02/2026). Markdown with front matter
   compiled to `.lock.yml`; the agent is **read-only**, writes only through
   `safe-outputs` with maxima per kind (one issue, three labels, three
   dispatches); engines Copilot, Claude, Codex, Gemini.
   https://github.github.com/gh-aw/reference/safe-outputs/
7. **Codex cloud**. `codex cloud exec --attempts 1-4` (best of N, a person
   picks); `codex exec --json` emits NDJSON on every state change, `-o`
   writes the last message. https://learn.chatgpt.com/docs/cli/reference
8. **Devin managed sessions**. A coordinator splits and delegates to child
   VMs, measures ACU per child, sleeps or terminates them; lessons go back to
   the playbook. https://docs.devin.ai/work-with-devin/advanced-capabilities
9. **OpenHands**: `max_iterations_per_run` 500, `max_budget_per_task`, a
   `StuckDetector`; **the model does not know the limit** (SDK issue #2406).
   **Aider**: lint and test after every edit, non-zero exit is corrected, no
   documented cap. **Live-SWE-agent** (arXiv 2511.13646): research, not a
   product. **Sweep**: 7.7k, last push 09/2025, dormant.

## 4. Event-driven interfaces for agent work

- **Temporal UI**: signal, update, cancel, terminate, reset; timeline,
  compact and JSON views, **pending activities with attempt and next retry**,
  animated dashed lines for what waits. https://docs.temporal.io/web-ui
- **OpenHands**: an event stream of actions and observations, persisted and
  replayable; policies `AlwaysConfirm|NeverConfirm|ConfirmRisky`, risk LOW,
  MEDIUM, HIGH, UNKNOWN, state `WAITING_FOR_CONFIRMATION`,
  `reject_pending_actions(message)`. https://docs.openhands.dev/sdk/guides/security
- **Dify 1.13**, Human Input node: a `human_input_required` event in the
  stream or a `paused` state; three-day timeout to a timeout branch; answer
  from the UI, an e-mail link without an account, or the service API.
  https://docs.dify.ai/en/use-dify/nodes/human-input
- **Windmill**: a suspended step with **signed** resume and cancel URLs, the
  number of approvals required, a timeout, a form whose values arrive as
  `resume["field"]`. https://www.windmill.dev/docs/flows/flow_approval
- **n8n**: the Wait node dumps state to the database and resumes on a
  webhook (`execution.resumeUrl`), a form or time; a Human-in-the-loop node
  above it with timeout and fallback branch. **Langflow**: events
  `add_message, token, end, vertex_starts/finishes`. **Cursor**: read-only
  view for colleagues, **take the desktop / give it back**, artefacts
  (screenshots, video, logs), a mandatory spend cap. **Copilot**: a session
  log with reasoning and setup output, «Stop session».

## What Sailor takes, and what it must not

**Descriptors.** A `quota_probe` capability per engine: `claude` reads
`rate_limits` (known); `codex` reads the app-server `account/rateLimits/read`
(closes half of «no channel for codex» of 31/08; it costs three to five
seconds of start-up, so the answer is cached); `gemini` has no documented
machine channel and `/stats` disagrees with the server, so it keeps
`unusable_when` only; keyed engines read `x-ratelimit-remaining-*` headers and
`GET /api/v1/key`. **The gemini descriptor must be re-verified**: the free
path through Google login no longer exists since 18/06.

**Profiles.** OpenClaw's per-profile state (`cooldownUntil, errorCount,
disabledUntil, disabledReason`) with cooldown by error class is the concrete
form of «an exhausted engine is not a broken engine»; the profile pinned to
the run is already item 5 of the plan. Rotation is across **different
providers and one's own subscriptions**, never across several accounts of the
same provider: Groq counts at organisation level, and the other thing is what
Anthropic blocked.

**Dispatch.** The cumulative check before the call, «every cap must pass»
(Bifrost), inside `candidates → Refused`; `on_status_codes` as the vocabulary
of the fallback; `sort: cost|ttft|tps` computed from the ledger, never from a
provider. The data pact is **per model**, not per provider (Zen, OpenRouter):
the column in `crates/models` goes per model, with the free and paid switch
kept separate as OpenRouter does.

**Ledger.** The `results.tsv` line (commit, metric, `keep|discard|crash`,
description) is the minimal form of a self-care run; Copilot's
`Agent-Logs-Url` trailer becomes a run-id trailer in Sailor's commits; gh-aw's
maxima per effect kind become a cap on effects per run beside the spend cap.

**Loop.** Three stops (promise, iterations, cancel) plus a wall of time with a
warning to the model (Copilot's 59 minutes; OpenHands shows the harm of not
telling it). Best of N with a blind judge is already the dispatch flow.
**Terminals and canvas.** An open step with signed close and cancel URLs and
a **timeout branch** (Windmill, Dify) cures «Waiting forever»; a view of the
waits with attempt and next retry (Temporal); refusal with a reason
(OpenHands); take and give back the terminal (Cursor).

**Must not.** (1) No proxy under a subscription command line: Anthropic's rule
admits the unmodified binary launched by a platform and forbids
intermediating the tokens; for Codex no explicit ban was found, but no
permission either. (2) No format translation: cc-switch and Claude Code
Router do it; the decision of 30/08 holds. (3) No number from a blog in the
catalogue: Cerebras and NVIDIA prove it. (4) No «free» tier without its pact
written beside it: Mistral and Muse Spark train by default.
