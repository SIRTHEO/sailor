import type { FlowEntry, Step, StepRun, ValueSchema } from "./flow";

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
// **QUESTO FLUSSO NON SI SALVA, ED È DICHIARATO.** Il motore registra sedici
// azioni; di quelle nominate qui ne esistono due, `shell_check` e — per la
// staffetta vera — nessun'altra. `pane_until_idle`, `pane_read`,
// `deposit_write`, `pane_send` e `signal_is_gone` sono la lista della spesa:
// descrivono la staffetta come dovrà essere, non come il motore la sa fare
// oggi. Premere «salva» su questo flusso lo rifiuta, col messaggio giusto.
//
// Il commento che stava qui diceva «il motore registra due azioni sole»: era
// vero fino al 28/08/2026 e nessuno l'ha più letto, mentre la stessa frase
// copiata in `flow.ts` è diventata sei nomi inventati nella cassetta dei
// passi (guasto 41). Qui la frase resta perché qui è ancora vera nella
// sostanza — questi nomi non esistono — ma il numero adesso è misurato.
//
// ── GLI SCHEMI, E PERCHÉ NON SONO PIÙ TUTTI `any` ──────────────────────────
//
// **CON `any` OVUNQUE, LA PROMESSA DELLE TRE FORME ERA INVISIBILE PER
// COSTRUZIONE.** Un nodo disegna una porta per tipo — cerchio testo, rombo
// struttura, quadrato valore — e uno schema `any` esce sempre quadrato: chi
// apriva la finestra fuori dal guscio nativo vedeva quaranta quadrati e non
// poteva accorgersi né della promessa né della sua rottura. Verde e muta.
//
// Gli schemi qui sotto non sono inventati per far vedere tre forme, e ognuno
// dice **da dove** viene, perché è l'unica parte verificabile della frase:
//
//  - l'**ingresso** di `shell_check` è `CheckSpec` in
//    `crates/actions/src/lib.rs` — `command`, `env`, `timeout_secs`, più
//    `accept`, `workdir`, `answer_shape` che qui non servono. È la stessa
//    struttura che i flussi di `flows/` riempiono a mano;
//  - l'**uscita** di `shell_check` NON sta in `CheckSpec` e non si può dedurre
//    da lì: `CheckSpec` è ciò che il passo riceve. Sta in
//    `ShellCheckAction::execute`, nei suoi due soli `ActionOutcome::Went`, che
//    scrivono `{"status"}` e `{"status", "answer"}`;
//  - `pane_send` non esiste ancora, ma `{ text }` è già scritto nel suo `with`
//    qui sotto: lo schema dice ciò che il dato dice già.
//
// **QUESTA DISTINZIONE È COSTATA UN CAMPO INVENTATO, ED È IL GUASTO 41 RIFATTO
// IN QUESTO FILE.** Sotto il commento che giurava di non aver inventato niente,
// l'uscita dichiarava `said`: una parola che in Rust esiste — è la variabile
// locale che tiene ciò che il comando ha stampato — ma che come **chiave JSON**
// appartiene ad altre azioni, non a questa. Chi ha scritto la riga ha guardato
// `CheckSpec`, ha visto il nome giusto e ha creduto di aver controllato: aveva
// controllato l'ingresso.
//
// La regola che ne resta, e che vale per la prossima volta più della parola
// corretta: **l'uscita di un'azione si legge dove l'azione la costruisce**, cioè
// nei rami `Ok(...)` del suo `execute`, mai nella struttura che descrive ciò che
// riceve. Le due cose hanno nomi simili e stanno nello stesso file, ed è per
// questo che si sbaglia.
//
// Il resto resta `any`, che è la verità: di quei passi non si sa ancora quale
// forma avrà l'ingresso, e disegnarne una sarebbe la stessa bugia da cui
// questo commento mette in guardia.
//
// LA MISURA VERA STA ALTROVE. Questi sono dati d'esempio, e restano tali:
// `ports.test.tsx` conta le porte sui **dieci file veri** di `flows/` e chiede
// che tutte e tre le forme e tutt'e due i pieni esistano davvero là. Quello è
// il controllo; questo è solo un esempio che smette di nascondere la promessa.

const anySchema: ValueSchema = { type: "any" };

/** Lo schema d'ingresso di `shell_check`, ricalcato su `CheckSpec`. */
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
 * Cosa esce da una verifica, letto in `ShellCheckAction::execute`.
 *
 * `status` c'è sempre. `answer` c'è **solo** se il passo ha dichiarato un
 * `answer_shape` e il comando è riuscito: senza forma dichiarata il testo
 * grezzo non esce dal passo affatto — «consegna `answer`, o niente», dice il
 * commento sopra il secondo `Went`. Perciò `answer` sta fra le proprietà e non
 * fra le `required`.
 *
 * `answer` è `any` e non `string`: quel valore ha la forma che il passo stesso
 * ha chiesto in `answer_shape`, e questo passo d'esempio non ne chiede nessuna.
 * Scriverci `string` sarebbe la stessa inesattezza di prima in un altro campo.
 *
 * `allow_extra` resta aperto di proposito. Chiuderlo direbbe «da questa azione
 * non uscirà mai un'altra chiave», e da qui — un file di dati d'esempio nella
 * finestra — quella promessa non la può tenere nessuno: la fa il motore, e
 * nessun controllo di questa parte la verificherebbe.
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

/** Cosa serve a mandare un testo a un pannello: il testo, e basta. */
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
          // 0. la storia, prima del momento.
          step("chain-brake", [], "shell_check", {
            input_schema: checkInput,
            output_schema: checkOutput,
          }),
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
            input_schema: paneSendInput,
          }),
          // 4. la prova che il mandato è arrivato: lo consuma il gancio d'avvio.
          step("signal-is-gone", ["send-clear"], "signal_is_gone"),
          // 5. un turno non parte da solo, e senza questo il resto non produce nulla.
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
  },
  {
    state: "loaded",
    flow: {
      id: "prima-corsa",
      description: "Il flusso più piccolo che esista: una verifica sola.",
      graph: {
        // Stessa azione del primo passo della staffetta, e nessuna voce in
        // `inputs`: le sue tre porte restano vuote, ed è vero — questo flusso
        // non dice con quale comando partire. È il caso che rende visibile la
        // differenza fra una porta piena e una vuota.
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
