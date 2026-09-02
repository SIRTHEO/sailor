import type { FlowEntry, Step, StepRun, ValueSchema } from "./flow";

// Sample data, until the window reads from the engine.

// The relay mirrors the real sequence written at the top of
// `crates/claude-hooks/src/relay.rs`, where the order of the calls is declared
// to be the behaviour and not negotiable. In particular the relay creates and
// closes NOTHING: it sends `/clear` and then the start to the same pane.

// **THIS FLOW DOES NOT SAVE, AND THAT IS DECLARED.** Several action names used
// here do not exist in the engine: they describe the relay as it will have to
// be, not as the engine can do it today. Pressing save rejects it, with the
// right message.

// ── THE SCHEMAS, AND WHY THEY ARE NOT ALL `any` ────────────────────────────
//
// **WITH `any` EVERYWHERE, THE PROMISE OF THE THREE SHAPES IS INVISIBLE BY
// CONSTRUCTION.** A node draws one port per type, and an `any` schema always
// comes out square: outside the native shell you would see forty squares and
// could notice neither the promise nor its breach. Green and mute.

// The schemas below are not invented to show three shapes, and each says WHERE
// it comes from, the only verifiable part of the claim:
//  - the INPUT of `shell_check` is `CheckSpec` in `crates/actions/src/lib.rs`;
//  - its OUTPUT is NOT there — that is what the step receives. It is in
//    `ShellCheckAction::execute`, in its two `ActionOutcome::Went` branches;
//  - `pane_send` does not exist yet, but `{ text }` is already in its `with`.

// **THE RULE THAT REMAINS, worth more than any corrected word: an action's
// OUTPUT is read where the action builds it**, in the `Ok(...)` branches of its
// `execute`, never in the struct describing what it receives. The two have
// similar names and live in the same file, which is why it goes wrong.

// The rest stays `any`, which is the truth: the input shape of those steps is
// not known, and drawing one would be the very lie this comment warns about.

// THE REAL MEASUREMENT IS ELSEWHERE. `ports.test.tsx` counts the ports on the
// REAL flow files and demands that all three shapes and both fills exist there.
// That is the check; this is only a sample that stops hiding the promise.

const anySchema: ValueSchema = { type: "any" };

/** The input schema of `shell_check`, mirrored from `CheckSpec`. */
const checkInput: ValueSchema = {
  type: "object",
  properties: {
    command: { type: "string" },
    env: { type: "object", properties: {}, required: [], allow_extra: true },
    timeout_secs: { type: "number" },
  },
  required: ["command", "timeout_secs"],
  allow_extra: true,
};

/**
 * What comes out of a check, read in `ShellCheckAction::execute`. `status` is
 * always there; `answer` only if the step declared an `answer_shape` and the
 * command succeeded, so it sits among the properties and not the `required`,
 * as `any`, its shape being whatever the step asked for. `allow_extra` stays
 * open because closing it would promise no other key ever comes out of this
 * action, and a sample-data file in the window cannot keep that promise: the
 * engine makes it, and nothing on this side would check it.
 */
const checkOutput: ValueSchema = {
  type: "object",
  properties: {
    status: { type: "one_of", values: ["passed", "failed", "timed_out"] },
    answer: { type: "any" },
  },
  required: ["status"],
  allow_extra: true,
};

/** What it takes to send a text to a pane: the text, and nothing else. */
const paneSendInput: ValueSchema = {
  type: "object",
  properties: { text: { type: "string" } },
  required: ["text"],
  allow_extra: true,
};

function step(
  id: string,
  deps: string[],
  action: string,
  extra: Partial<{
    max_attempts: number;
    with: Record<string, unknown>;
    input_schema: ValueSchema;
    output_schema: ValueSchema;
  }> = {},
): Step {
  return {
    id,
    deps,
    action,
    input_schema: extra.input_schema ?? anySchema,
    output_schema: extra.output_schema ?? anySchema,
    when: null,
    max_attempts: extra.max_attempts ?? 1,
    with: extra.with ?? null,
  };
}

export const SAMPLE: FlowEntry[] = [
  {
    state: "loaded",
    flow: {
      id: "relay",
      description:
        "Azzera una sessione piena e le rimette in mano il lavoro, sul posto.",
      graph: {
        steps: [
          // 0. the history, before the moment.
          step("chain-brake", [], "shell_check", {
            input_schema: checkInput,
            output_schema: checkOutput,
          }),
          // 1. never cut a turn in half.
          step("pane-is-idle", ["chain-brake"], "pane_until_idle"),
          // 1b. positive check: never a keystroke onto a pending question.
          step("prompt-is-empty", ["pane-is-idle"], "pane_read"),
          // 2. the baton — handover, resume and brief — before acting.
          step("write-the-baton", ["prompt-is-empty"], "deposit_write"),
          // 3. the session restarts empty in place: same pane.
          step("send-clear", ["write-the-baton"], "pane_send", {
            max_attempts: 3,
            with: { text: "/clear" },
            input_schema: paneSendInput,
          }),
          // 4. proof the brief arrived: the start hook consumes it.
          step("signal-is-gone", ["send-clear"], "signal_is_gone"),
          // 5. a turn does not start by itself, and without this nothing else
          //    produces anything.
          step("send-the-start", ["signal-is-gone"], "pane_send", {
            max_attempts: 3,
            with: { text: "riprendi dal punto di ripresa che hai ricevuto" },
            input_schema: paneSendInput,
          }),
        ],
        skippable_dependencies: [],
      },
      inputs: {
        "chain-brake": {
          command: 'test -n "$CLAUDE_CODE_SESSION_ID"',
          env: {},
          timeout_secs: 10,
        },
      },
    },
    origin: "yours",
  },
  {
    state: "loaded",
    flow: {
      id: "prima-corsa",
      description: "Il flusso più piccolo che esista: una verifica sola.",
      graph: {
        // Same action as the relay's first step, and no `inputs` entry: its
        // three ports stay hollow, and that is true — this flow does not say
        // which command to start with. It is the case that makes the difference
        // between a filled and a hollow port visible.
        steps: [
          step("working-tree-is-clean", [], "shell_check", {
            input_schema: checkInput,
            output_schema: checkOutput,
          }),
        ],
        skippable_dependencies: [],
      },
      inputs: {},
    },
    origin: "this project",
  },
  // A broken flow does NOT vanish from the list: it shows, marked, with the
  // reason. **AND ITS ORIGIN IS A PLACE ON A DISK**, never `built in`: the
  // shipped flows travel inside the binary, alike on every machine, and a test
  // in `flow::system` reads all of them before a release. One could still
  // reach a window, kept with its reason rather than dropped — but not one
  // machine at a time, which is the scene this was drawing.
  {
    state: "broken",
    broken: {
      name: "notte",
      reason: "campo sconosciuto `retries` alla riga 14 (forse `max_attempts`?)",
    },
    origin: "this project",
  },
];

export const SAMPLE_RUN = new Map<string, StepRun>([
  ["chain-brake", { step_id: "chain-brake", state: "went", attempt: 1 }],
  ["pane-is-idle", { step_id: "pane-is-idle", state: "went", attempt: 1 }],
  ["prompt-is-empty", { step_id: "prompt-is-empty", state: "went", attempt: 1 }],
  ["write-the-baton", { step_id: "write-the-baton", state: "went", attempt: 1 }],
  ["send-clear", { step_id: "send-clear", state: "went", attempt: 2 }],
  ["signal-is-gone", { step_id: "signal-is-gone", state: "went", attempt: 1 }],
  [
    "send-the-start",
    {
      step_id: "send-the-start",
      state: "running",
      attempt: 1,
      held_by_pid: 41822,
      elapsed_secs: 134,
    },
  ],
]);
