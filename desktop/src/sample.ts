import type { FlowEntry, StepRun } from "./flow";

// Dati di esempio finché la finestra non legge dal motore.
//
// La staffetta è ricalcata sulla sequenza vera, quella scritta in cima a
// `crates/claude-hooks/src/relay.rs` — dove l'ordine delle chiamate è
// dichiarato «il comportamento, e non è negoziabile». In particolare la
// staffetta NON crea e NON chiude niente: manda `/clear` e poi l'avvio allo
// stesso pannello. Chi la disegna come «apre una sessione nuova» sta
// descrivendo la versione di prima del 19/08/2026, che apriva 47 sessioni in
// più in due giorni.
//
// Oggi il motore registra due azioni sole (`external_engine`, `shell_check`):
// le altre nominate qui sono la lista della spesa, e finché non esistono
// questo flusso non parte.

const anySchema = { type: "any" } as const;

function step(
  id: string,
  deps: string[],
  action: string,
  extra: Partial<{ max_attempts: number; with: Record<string, unknown> }> = {},
) {
  return {
    id,
    deps,
    action,
    input_schema: anySchema,
    output_schema: anySchema,
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
          // 0. la storia, prima del momento.
          step("chain-brake", [], "shell_check"),
          // 1. non si tronca un turno a metà.
          step("pane-is-idle", ["chain-brake"], "pane_until_idle"),
          // 1bis. prova positiva: mai un tasto su una domanda in sospeso.
          step("prompt-is-empty", ["pane-is-idle"], "pane_read"),
          // 2. il testimone — consegna, ripresa e mandato — prima di agire.
          step("write-the-baton", ["prompt-is-empty"], "deposit_write"),
          // 3. la sessione riparte vuota sul posto: stesso pannello.
          step("send-clear", ["write-the-baton"], "pane_send", {
            max_attempts: 3,
            with: { text: "/clear" },
          }),
          // 4. la prova che il mandato è arrivato: lo consuma il gancio d'avvio.
          step("signal-is-gone", ["send-clear"], "signal_is_gone"),
          // 5. un turno non parte da solo, e senza questo il resto non produce nulla.
          step("send-the-start", ["signal-is-gone"], "pane_send", {
            max_attempts: 3,
            with: { text: "riprendi dal punto di ripresa che hai ricevuto" },
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
  },
  {
    state: "loaded",
    flow: {
      id: "prima-corsa",
      description: "Il flusso più piccolo che esista: una verifica sola.",
      graph: {
        steps: [step("working-tree-is-clean", [], "shell_check")],
        skippable_dependencies: [],
      },
      inputs: {},
    },
  },
  // Un flusso rotto NON sparisce dall'elenco: si vede, marcato, col motivo.
  {
    state: "broken",
    broken: {
      name: "notte",
      reason: "campo sconosciuto `retries` alla riga 14 (forse `max_attempts`?)",
    },
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
