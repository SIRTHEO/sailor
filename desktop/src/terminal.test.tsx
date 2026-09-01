// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { Terminal as Emulator } from "@xterm/xterm";
import stylesheetSource from "./styles.css?raw";
import { belowThreshold, contrastPairs, parseStylesheet, type Stylesheet } from "./contrast";
import { Terminals } from "./Terminals";
import { routingNote } from "./TerminalPane";
import {
  decodeBytes,
  encodeBytes,
  keyBytes,
  keyStroke,
  livenessOf,
  livenessWord,
  OutputBus,
  type KeyAction,
  type TerminalSummary,
} from "./terminal";

/**
 * **LE QUATTRO COSE CHE LA METÀ REACT DEL TERMINALE PUÒ SBAGLIARE IN SILENZIO.**
 *
 * Un accento perso in mezzo a un'uscita lunga, un tasto mandato allo
 * smistamento invece che al programma, un terminale morto disegnato vivo, e
 * un'accoppiata di contrasto sotto soglia. Nessuna delle quattro fa rumore: la
 * finestra si disegna lo stesso, e chi guarda crede a quello che legge.
 *
 * **SI PROVA CONTRO IL CONTRATTO, NON CONTRO IL PONTE.** Il ponte Rust nasce in
 * un altro cantiere mentre questo file si scrive. Il guscio è finto — le
 * risposte e i due eventi sono scritti a mano secondo
 * `docs/2026-09-01-il-contratto-del-terminale.md` — e i componenti sono quelli
 * veri, emulatore compreso.
 */

afterEach(cleanup);

let sheet: Stylesheet;

beforeAll(() => {
  sheet = parseStylesheet(stylesheetSource);
  // xterm chiede al browser due cose che jsdom non ha. Sono impalcature, non
  // finzioni sul codice provato: in una finestra vera le fornisce il browser.
  (window as unknown as { matchMedia: unknown }).matchMedia = () => ({
    matches: false,
    media: "",
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent: () => false,
  });
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

// ── i byte non sono testo ────────────────────────────────────────────────

describe("i byte che vanno alla shell", () => {
  test("UN ACCENTO PARTE IN UTF-8, non nel latin-1 di `btoa`", () => {
    // `btoa("à")` non fallisce e non avverte: risponde il byte 0xE0, che nella
    // shell è un'altra lettera. Il difetto si vede solo sulle parole accentate,
    // cioè su quelle che in questo repo si scrivono tutto il giorno.
    const mine = encodeBytes(keyBytes("à"));
    expect(mine).toBe("w6A=");
    expect(mine).not.toBe(btoa("à"));
    expect(Array.from(decodeBytes(mine))).toEqual([0xc3, 0xa0]);
  });

  test("ogni byte fra 0 e 255 torna indietro identico", () => {
    // Il ritorno a capo, l'escape, il Ctrl-C e i byte alti di un carattere
    // multibyte passano tutti di qui.
    const every = new Uint8Array(256);
    for (let value = 0; value < 256; value += 1) every[value] = value;
    expect(Array.from(decodeBytes(encodeBytes(every)))).toEqual(Array.from(every));
  });
});

describe("i byte che escono dal processo", () => {
  test("UNA LETTERA SPEZZATA FRA DUE EVENTI SI RIMETTE INSIEME SULLO SCHERMO", async () => {
    // È la ragione per cui il contratto dice base64 e non stringa. I due eventi
    // sono quelli che il ponte manderà — `è` vale 0xC3 0xA8, e uno pseudo-
    // terminale può consegnarli in due letture — e il giudice è il buffer
    // dell'emulatore vero, cioè ciò che una persona leggerebbe.
    const term = new Emulator({ cols: 20, rows: 4 });
    await write(term, decodeBytes(encodeBytes(new Uint8Array([0xc3]))));
    await write(term, decodeBytes(encodeBytes(new Uint8Array([0xa8, 0x21]))));
    expect(term.buffer.active.getLine(0)?.translateToString(true)).toBe("è!");
    term.dispose();
  });
});

function write(term: Emulator, bytes: Uint8Array): Promise<void> {
  return new Promise((done) => term.write(bytes, () => done()));
}

// ── invio e tasti sono due strade diverse ────────────────────────────────

function kinds(actions: KeyAction[]): string[] {
  return actions.map((action) => action.kind);
}

function submittedLines(actions: KeyAction[]): string[] {
  return actions.flatMap((action) => (action.kind === "submit" ? [action.line] : []));
}

describe("dove va un tasto", () => {
  test("NESSUN TASTO CHE NON SIA INVIO FINISCE NELLO SMISTAMENTO", () => {
    // Il difetto da cui difende: mandare tutto a `submit` farebbe esaminare
    // ogni freccia e ogni Ctrl-C da un elenco di regole che non li riguarda, e
    // un editor dentro il terminale diventerebbe inservibile.
    const keys = ["l", "s", " ", "-", "à", "\x7f", "\x03", "\x1b[A", "\x1b[B", "\t", "\x04", "\x15"];
    for (const key of keys) {
      for (const draft of ["", "cargo test"]) {
        for (const mode of ["compose", "raw"] as const) {
          const stroke = keyStroke(mode, draft, key);
          expect(submittedLines(stroke.actions), `«${key}» in ${mode} con «${draft}»`).toEqual([]);
        }
      }
    }
  });

  test("Invio consegna la riga intera, e non ne manda un byte alla shell", () => {
    // Se anche un solo carattere fosse già partito, `terminal_submit` — che la
    // riga la scrive lui — la farebbe finire scritta due volte: «lsls».
    const stroke = keyStroke("compose", "cargo test -p terminal", "\r");
    expect(submittedLines(stroke.actions)).toEqual(["cargo test -p terminal"]);
    expect(kinds(stroke.actions)).not.toContain("press");
    expect(stroke.draft).toBe("");
  });

  test("comporre una riga non manda niente alla shell, un carattere alla volta", () => {
    let draft = "";
    const sent: string[] = [];
    for (const key of "ls -la") {
      const stroke = keyStroke("compose", draft, key);
      draft = stroke.draft;
      sent.push(...kinds(stroke.actions));
    }
    expect(draft).toBe("ls -la");
    expect(sent).not.toContain("press");
    expect(sent).not.toContain("submit");
  });

  test("IN UN EDITOR INVIO È UN TASTO, non una riga da smistare", () => {
    // È il caso che il contratto nomina per esteso, ed è tutto il motivo per
    // cui `submit` e `press` sono due comandi e non uno.
    const stroke = keyStroke("raw", "", "\r");
    expect(kinds(stroke.actions)).toEqual(["press"]);
    expect(submittedLines(stroke.actions)).toEqual([]);
  });

  test("Ctrl-C arriva sempre a chi sta girando, anche a riga piena", () => {
    // Un modo di fermare quello che gira non si toglie a nessuno, in nessun
    // modo: è l'unica eccezione al «mentre componi non parte niente».
    for (const mode of ["compose", "raw"] as const) {
      const stroke = keyStroke(mode, "un comando lunghissimo", "\x03");
      expect(kinds(stroke.actions)).toContain("press");
      expect(stroke.draft).toBe("");
    }
  });

  test("un Invio a vuoto è un a capo, non una riga da smistare", () => {
    const stroke = keyStroke("compose", "", "\r");
    expect(kinds(stroke.actions)).toEqual(["press"]);
  });

  test("a riga vuota il terminale è un passaggio diretto; a riga piena dice di no", () => {
    // La freccia in su a riga vuota riprende il comando di prima dalla shell.
    expect(kinds(keyStroke("compose", "", "\x1b[A").actions)).toEqual(["press"]);
    // A riga piena non parte e dice perché: partire sfaserebbe lo schermo,
    // perché quella riga la tiene la finestra e non la `readline` della shell.
    const refused = keyStroke("compose", "ls", "\x1b[A");
    expect(kinds(refused.actions)).toEqual(["ignored"]);
    expect(refused.draft).toBe("ls");
  });

  test("la cancellazione toglie un carattere alla riga e uno allo schermo", () => {
    const stroke = keyStroke("compose", "lsx", "\x7f");
    expect(stroke.draft).toBe("ls");
    expect(stroke.actions).toEqual([{ kind: "echo", text: "\b \b" }]);
  });
});

describe("dove è finita la riga, detto a chi guarda", () => {
  test("una riga dirottata nomina la regola e il flusso, non solo il flusso", () => {
    // Senza il nome della regola, chi guarda sa che la riga non è stata
    // eseguita e non ha modo di risalire alla riga di JSON che l'ha deciso.
    const said = routingNote("? trova i residui", {
      kind: "flow",
      flow: "smista-il-lavoro",
      text: "trova i residui",
      rule: "marked-request",
    });
    expect(said).toContain("marked-request");
    expect(said).toContain("smista-il-lavoro");
    expect(said).toContain("non è stata eseguita");
  });

  test("un comando dice che è andato alla shell", () => {
    expect(routingNote("ls", { kind: "command" })).toContain("shell");
  });
});

// ── vivo, morto, o non lo so più ─────────────────────────────────────────

function summary(over: Partial<TerminalSummary>): TerminalSummary {
  return {
    id: "t1",
    workspaceRoot: "/home/someone/personal/sailor",
    workspaceName: "sailor",
    alive: true,
    processId: 4242,
    ...over,
  };
}

describe("com'è messo un terminale", () => {
  test("l'evento vince sull'elenco, che è vecchio di un giro di domande", () => {
    const closed = new Map([["t1", "uscita 0"]]);
    expect(livenessOf(summary({ alive: true }), closed, true)).toEqual({
      state: "closed",
      status: "uscita 0",
    });
  });

  test("l'elenco che lo dà per chiuso non inventa un esito", () => {
    expect(livenessOf(summary({ alive: false }), new Map(), true)).toEqual({
      state: "closed",
      status: null,
    });
  });

  test("SENZA IL CANALE DEGLI EVENTI NON SI DICE «VIVO»: si dice «non lo so più»", () => {
    // È il guasto 12 rifatto nella finestra. Se `terminal_closed` non può
    // arrivare, «vivo» è un'affermazione che questa schermata non può fare, e
    // un pannello che la fa lo fa per sempre — la morte non arriverà mai.
    const mine = livenessOf(summary({ alive: true }), new Map(), false);
    expect(mine.state).toBe("unknown");
    expect(mine.state === "unknown" && mine.why).toContain("non lo saprebbe");
  });

  test("col canale attaccato e l'elenco che lo dà vivo, è vivo", () => {
    expect(livenessOf(summary({}), new Map(), true)).toEqual({ state: "alive" });
  });

  test("i tre stati hanno tre parole diverse: il colore non porta lo stato da solo", () => {
    const words = [
      livenessWord({ state: "alive" }),
      livenessWord({ state: "closed", status: null }),
      livenessWord({ state: "unknown", why: "x" }),
    ];
    expect(new Set(words).size).toBe(3);
  });
});

describe("l'uscita arriva al pannello giusto", () => {
  test("i byte vanno a chi si è iscritto per quell'id, e a nessun altro", () => {
    const bus = new OutputBus();
    const mine: number[] = [];
    bus.subscribe("t1", (bytes) => mine.push(...bytes));
    expect(bus.deliver("t1", new Uint8Array([1, 2]))).toBe(true);
    // BYTE PER UN TERMINALE SENZA PANNELLO SONO BYTE PERSI, e chi li perde in
    // silenzio mostra uno schermo vuoto dove c'era un'uscita.
    expect(bus.deliver("t2", new Uint8Array([9]))).toBe(false);
    expect(mine).toEqual([1, 2]);
  });
});

// ── sullo schermo ────────────────────────────────────────────────────────

interface Call {
  command: string;
  args: Record<string, unknown> | undefined;
}

interface FakeShell {
  /** Fa arrivare un evento come lo manderebbe il ponte. */
  emit: (event: string, payload: unknown) => void;
  /** Quali comandi sono stati chiesti, in ordine, con cosa. */
  calls: Call[];
  /** Solo i nomi, per le asserzioni che non guardano gli argomenti. */
  asked: string[];
  /** Gli argomenti dei soli `command` chiesti, in ordine. */
  argsOf: (command: string) => Array<Record<string, unknown> | undefined>;
  stop: () => void;
}

/**
 * Finge il guscio: i sei comandi e i due eventi del contratto.
 *
 * **`listen` C'È O NON C'È, ED È UN PARAMETRO.** Senza il canale la schermata
 * deve dire «non lo so più» invece di «vivo», e senza poterlo togliere quella
 * riga non si potrebbe provare.
 */
function pretendShell(answers: Record<string, unknown>, withEvents = true): FakeShell {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  const handlers = new Map<string, Array<(event: { payload: unknown }) => void>>();
  const calls: Call[] = [];
  const asked: string[] = [];
  const shell: Record<string, unknown> = {
    core: {
      invoke: (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        asked.push(command);
        return Promise.resolve(answers[command]);
      },
    },
  };
  if (withEvents) {
    shell.event = {
      listen: (event: string, handler: (event: { payload: unknown }) => void) => {
        const list = handlers.get(event) ?? [];
        list.push(handler);
        handlers.set(event, list);
        return Promise.resolve(() => {
          handlers.set(event, (handlers.get(event) ?? []).filter((one) => one !== handler));
        });
      },
    };
  }
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = shell;
  return {
    calls,
    asked,
    argsOf: (command) => calls.filter((call) => call.command === command).map((call) => call.args),
    emit: (event, payload) => {
      for (const handler of handlers.get(event) ?? []) handler({ payload });
    },
    stop: () => {
      (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
    },
  };
}

const TWO: TerminalSummary[] = [
  { id: "t1", workspaceRoot: "/home/someone/personal/sailor", workspaceName: "sailor", alive: true, processId: 4242 },
  { id: "t2", workspaceRoot: "/home/someone/other-repo/work/packages", workspaceName: "packages", alive: true, processId: 4243 },
];

/**
 * Il DOM disegnato, misurato tutto: `:root` porta i ruoli.
 *
 * **CHI MISURA VA MISURATO.** La scena dichiara quante accoppiate si aspetta di
 * aver trovato: una prova che non guarda niente passa, ed è il modo esatto in
 * cui questo controllo tornerebbe a essere una decorazione.
 */
function measure(atLeast: number): string[] {
  const pairs = contrastPairs(document.documentElement, sheet);
  expect(pairs.length).toBeGreaterThanOrEqual(atLeast);
  return belowThreshold(pairs);
}

describe("la schermata dei terminali", () => {
  test("l'elenco viene da `terminal_list`, e ogni scheda porta la sua parola", async () => {
    const shell = pretendShell({ terminal_list: TWO });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      expect(shell.asked).toContain("terminal_list");
      expect(screen.getAllByText("vivo").length).toBeGreaterThan(0);
      expect(measure(20)).toEqual([]);
    } finally {
      shell.stop();
    }
  });

  test("UN TERMINALE MORTO SI VEDE MORTO: `terminal_closed` cambia lo stato mostrato", async () => {
    // È il guasto 12 rifatto nella finestra: un pannello che resta uguale
    // quando il processo dentro è finito. Prima dell'evento la scheda dice
    // «vivo»; dopo l'evento dice «finito», e l'elenco non è ancora cambiato.
    const shell = pretendShell({ terminal_list: TWO });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      expect(screen.queryByText("finito")).toBeNull();

      await act(async () => {
        shell.emit("terminal_closed", { id: "t1", status: "uscita 130" });
      });

      expect(screen.getAllByText("finito").length).toBeGreaterThan(0);
      // L'esito si legge, non solo la parola: «finito» senza sapere come non
      // dice se il processo è andato o è stato ucciso.
      expect(screen.getByText("uscita 130")).toBeTruthy();
      expect(measure(20)).toEqual([]);
    } finally {
      shell.stop();
    }
  });

  test("SENZA CANALE DEGLI EVENTI la schermata non dichiara vivo nessuno", async () => {
    const shell = pretendShell({ terminal_list: TWO }, false);
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      expect(screen.queryByText("vivo")).toBeNull();
      expect(screen.getAllByText("non lo so più").length).toBeGreaterThan(0);
      expect(measure(20)).toEqual([]);
    } finally {
      shell.stop();
    }
  });

  test("APRIRE DICHIARA LA CARTELLA: senza spazio di lavoro non si apre niente", async () => {
    // Non esiste un terminale generico a cui poi si dice dove andare: la
    // cartella è parte di cosa il terminale è, e va nella chiamata che lo apre.
    const shell = pretendShell({ terminal_list: [], terminal_open: TWO[0] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      const button = await screen.findByRole("button", { name: "Apri un terminale" });
      expect((button as HTMLButtonElement).disabled).toBe(true);

      const field = screen.getByPlaceholderText("/home/someone/personal/sailor");
      fireEvent.change(field, { target: { value: "/home/someone/personal/sailor" } });
      expect((screen.getByRole("button", { name: "Apri un terminale" }) as HTMLButtonElement).disabled).toBe(false);

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Apri un terminale" }));
      });
      expect(shell.asked).toContain("terminal_open");
    } finally {
      shell.stop();
    }
  });

  test("APRIRE PORTA LA CARTELLA FINO AL COMANDO, non solo fino al componente", async () => {
    // Il campo si compila e si guarda cosa è arrivato al ponte: senza questa
    // riga, `terminal_open` potrebbe essere chiamato senza `workspaceRoot` e
    // la prova sopra resterebbe verde.
    const shell = pretendShell({ terminal_list: [], terminal_open: TWO[0] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      const field = await screen.findByPlaceholderText("/home/someone/personal/sailor");
      fireEvent.change(field, { target: { value: "/home/someone/other-repo/work" } });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Apri un terminale" }));
      });
      expect(shell.argsOf("terminal_open")).toEqual([
        { workspaceRoot: "/home/someone/other-repo/work", program: undefined, cols: 80, rows: 24 },
      ]);
    } finally {
      shell.stop();
    }
  });

  test("un elenco vuoto non si confonde con un motore che non risponde", async () => {
    // «Zero» e «non posso vedere» non sono la stessa cosa, e sono le due frasi
    // fra cui questa schermata deve saper scegliere.
    const shell = pretendShell({ terminal_list: [] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByText(/Nessun terminale aperto/);
      cleanup();
      render(
        <div className="app">
          <Terminals native={false} />
        </div>,
      );
      expect(screen.getByText(/Non riesco a chiedere/)).toBeTruthy();
    } finally {
      shell.stop();
    }
  });
});

// ── il filo, non i pezzi ─────────────────────────────────────────────────

/**
 * **LE DUE PROVE PER CUI QUESTO CANTIERE ESISTE, E CHE MANCAVANO.**
 *
 * `keyStroke`, `decodeBytes` e `OutputBus` erano provate come funzioni pure e
 * mai attraverso il filo che le collega. Un giudice l'ha misurato rimettendo
 * due difetti — ogni tasto anche a `onSubmit`, e i byte del bus che non
 * arrivano all'emulatore — e la batteria è rimasta verde tutte e due le volte:
 * l'intero `onData` di `TerminalPane` non veniva mai eseguito, e nessuna prova
 * emetteva `terminal_output`.
 *
 * Qui i tasti sono eventi veri di tastiera sulla textarea che xterm ascolta,
 * l'uscita è l'evento del contratto, e ciò che si guarda è **quale comando è
 * arrivato al ponte** e **cosa c'è nel buffer dell'emulatore disegnato**.
 */

/** La tastiera del pannello a schermo: xterm ascolta su una textarea nascosta. */
function keyboardOf(): HTMLTextAreaElement {
  const area = document.querySelector(".pane:not([hidden]) .xterm-helper-textarea");
  expect(area, "il pannello a schermo non ha una textarea: l'emulatore non si è montato").toBeTruthy();
  return area as HTMLTextAreaElement;
}

/** Ciò che si legge sullo schermo del pannello a schermo. */
function paneScreenText(): string {
  return document.querySelector(".pane:not([hidden]) .xterm-rows")?.textContent ?? "";
}

/** Un tasto che non è una lettera: xterm lo ricava da `keydown`. */
function tapKey(area: HTMLElement, init: KeyboardEventInit): void {
  area.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
}

/** Una lettera: xterm la ricava da `keypress`. */
function typeLetter(area: HTMLElement, letter: string): void {
  const code = letter.charCodeAt(0);
  area.dispatchEvent(
    new KeyboardEvent("keypress", {
      key: letter,
      keyCode: code,
      charCode: code,
      bubbles: true,
      cancelable: true,
    }),
  );
}

describe("il filo fra il ponte e lo schermo", () => {
  test("L'USCITA ARRIVA ALL'EMULATORE DEL PANNELLO, e una lettera spezzata si ricompone lì", async () => {
    // Il mutante che questa prova deve uccidere: i byte del bus che non
    // arrivano mai all'emulatore. Lo schermo resterebbe vuoto su un'uscita
    // viva, e nessuna delle prove sulle funzioni pure se ne accorgerebbe.
    const shell = pretendShell({ terminal_list: [TWO[0]] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /sailor/ });

      await act(async () => {
        // I due eventi che il ponte manderebbe: `è` vale 0xC3 0xA8, e uno
        // pseudo-terminale può consegnarli in due letture.
        shell.emit("terminal_output", { id: "t1", bytes: encodeBytes(new Uint8Array([0xc3])) });
        shell.emit("terminal_output", { id: "t1", bytes: encodeBytes(new Uint8Array([0xa8, 0x21])) });
      });

      await waitFor(() => {
        expect(paneScreenText()).toContain("è!");
      });
    } finally {
      shell.stop();
    }
  });

  test("l'uscita di un terminale non finisce nel pannello di un altro", async () => {
    const shell = pretendShell({ terminal_list: TWO });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      await act(async () => {
        shell.emit("terminal_output", { id: "t2", bytes: encodeBytes(keyBytes("nel secondo")) });
      });
      // Il primo è quello a schermo, e non deve aver ricevuto niente.
      await waitFor(() => {
        expect(paneScreenText()).not.toContain("nel secondo");
      });
    } finally {
      shell.stop();
    }
  });

  test("UN TASTO VA A `terminal_press`, INVIO A `terminal_submit`, E MAI TUTTI E DUE", async () => {
    // Il mutante che questa prova deve uccidere: ogni tasto mandato **anche** a
    // `onSubmit`. È il bivio del contratto, e senza questa prova saltarlo non
    // faceva rumore.
    const shell = pretendShell({
      terminal_list: [TWO[0]],
      terminal_press: null,
      terminal_submit: { kind: "command" },
    });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /sailor/ });
      const keys = keyboardOf();

      // COME NASCE: UN TERMINALE NASCE TERMINALE, e la lettera è ciò che lo
      // dimostra. Una freccia, un Ctrl-C e un Invio andrebbero a `press` anche
      // componendo, a riga vuota: solo la `x` distingue i due modi, ed è la
      // riga che tiene ferma la decisione «predefinito `raw`».
      expect(screen.getByText("i tasti vanno diritti al processo")).toBeTruthy();
      await act(async () => {
        typeLetter(keys, "x");
        tapKey(keys, { key: "ArrowUp", keyCode: 38 });
        tapKey(keys, { key: "c", keyCode: 67, ctrlKey: true });
        tapKey(keys, { key: "Enter", keyCode: 13 });
      });
      expect(shell.argsOf("terminal_press").map((args) => args?.bytes)).toEqual([
        encodeBytes(keyBytes("x")),
        encodeBytes(keyBytes("\x1b[A")),
        encodeBytes(keyBytes("\x03")),
        encodeBytes(keyBytes("\r")),
      ]);
      expect(shell.asked).not.toContain("terminal_submit");

      // Lo smistamento su questa riga lo chiede chi lo vuole.
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "componi una riga da smistare" }));
      });

      // Comporre non manda niente a nessuno: se anche un carattere partisse,
      // `terminal_submit` — che la riga la scrive lui — la farebbe eseguire due
      // volte.
      await act(async () => {
        typeLetter(keys, "l");
        typeLetter(keys, "s");
      });
      expect(shell.argsOf("terminal_press").length).toBe(4);
      expect(shell.asked).not.toContain("terminal_submit");

      // Invio: una chiamata sola, con la riga intera, e nessun tasto in più.
      await act(async () => {
        tapKey(keys, { key: "Enter", keyCode: 13 });
      });
      expect(shell.argsOf("terminal_submit")).toEqual([{ id: "t1", line: "ls" }]);
      expect(shell.argsOf("terminal_press").length).toBe(4);

      // E dove è finita si legge sullo schermo.
      await waitFor(() => {
        expect(screen.getByText(/è andata alla shell/)).toBeTruthy();
      });
    } finally {
      shell.stop();
    }
  });

  test("una riga dirottata non arriva alla shell, e lo dice con la regola", async () => {
    const shell = pretendShell({
      terminal_list: [TWO[0]],
      terminal_press: null,
      terminal_submit: {
        kind: "flow",
        flow: "smista-il-lavoro",
        text: "trova i residui",
        rule: "marked-request",
      },
    });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /sailor/ });
      const keys = keyboardOf();
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "componi una riga da smistare" }));
      });
      await act(async () => {
        typeLetter(keys, "?");
        tapKey(keys, { key: "Enter", keyCode: 13 });
      });
      expect(shell.argsOf("terminal_submit")).toEqual([{ id: "t1", line: "?" }]);
      // NIENTE È ARRIVATO ALLA SHELL: una riga dirottata non si esegue.
      expect(shell.asked).not.toContain("terminal_press");
      await waitFor(() => {
        expect(screen.getByText(/marked-request/)).toBeTruthy();
      });
    } finally {
      shell.stop();
    }
  });

  // `terminal_resize` RESTA SENZA PROVA, E SI DICE PERCHÉ. Il filo parte da un
  // `ResizeObserver` e da `fit()`, che misurano riquadri veri: in jsdom ogni
  // riquadro è alto e largo zero, `fit()` non produce nessuna taglia nuova e
  // `refit` esce senza chiamare niente. Una prova qui potrebbe solo asserire
  // «zero chiamate o più», che è vera comunque — e una riga così è il modo in
  // cui questo controllo tornerebbe a essere una decorazione. Si verifica
  // aprendo la finestra.

  test("chiudere un terminale lo chiede al motore, con il suo id", async () => {
    const shell = pretendShell({ terminal_list: [TWO[0]], terminal_close: null });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /sailor/ });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Chiudi questo terminale" }));
      });
      expect(shell.argsOf("terminal_close")).toEqual([{ id: "t1" }]);
    } finally {
      shell.stop();
    }
  });
});
