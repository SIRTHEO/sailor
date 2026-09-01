// @vitest-environment jsdom
import stylesheetSource from "./styles.css?raw";
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { FlowBandNode, StepNode, type FlowBandData, type StepNodeData } from "./StepNode";
import { Now, RunGroup } from "./Now";
import { History } from "./History";
import { Installed } from "./Installed";
import { Manual } from "./Manual";
import type { OpenRun } from "./engine";
import type { Step, StepRun, StepState } from "./flow";
import { belowThreshold, contrastPairs, parseStylesheet, type Stylesheet } from "./contrast";

/**
 * **IL DIVIETO 6, MISURATO SUL DOM DISEGNATO, DENTRO `npm test`.**
 *
 * Il divieto in testa a `styles.css` — nessuna accoppiata testo/sfondo sotto
 * 4,5:1 — non aveva niente che lo interrogasse. Un verificatore l'ha dimostrato
 * rimettendo `--muted: #94a3b8`: due caratteri, ventitré accoppiate sotto
 * soglia sullo schermo, e `vitest`, `tsc` e `identifiers_are_in_english` tutti
 * e tre verdi. Questa prova è ciò che rende rossa quella modifica.
 *
 * **TRE SCENE, NON UNA.** «Zero sotto soglia» era vero solo sulla schermata a
 * riposo: bastava un clic su un flusso perché le altre corsie si spegnessero e
 * ne comparissero cinque. Quindi si misura a riposo, con un flusso a fuoco, e
 * su un nodo per ognuno dei sei stati — compresi quelli che i dati d'esempio
 * non producono.
 */

afterEach(cleanup);

// React Flow misura il proprio riquadro all'avvio: fuori da un browser vero
// non c'è chi lo faccia, e senza questi due la tela non si monta affatto.
class NoResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

let sheet: Stylesheet;

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = NoResizeObserver;
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
  sheet = parseStylesheet(stylesheetSource);
});

/**
 * React Flow tiene un nodo `visibility: hidden` finché non l'ha misurato, e a
 * misurarlo è un `ResizeObserver` che qui non esiste: senza questo la tela
 * intera resterebbe fuori dalla misura, e il controllo direbbe «zero sotto
 * soglia» perché non ha guardato niente. In un browser vero quei nodi sono
 * visibili — è la riga qui sotto a dire la verità, non l'attributo.
 */
function revealCanvasNodes(): void {
  for (const node of Array.from(document.querySelectorAll<HTMLElement>(".react-flow__node"))) {
    if (node.style.visibility === "hidden") node.style.visibility = "visible";
  }
}

/**
 * Il DOM da misurare è tutto il documento: `:root` porta i ruoli.
 *
 * **CHI MISURA VA MISURATO.** La scena dichiara quante accoppiate si aspetta di
 * aver trovato: una prova che non guarda niente passa, ed è il modo esatto in
 * cui questo controllo tornerebbe a essere una decorazione.
 */
function measure(atLeast: number): string[] {
  revealCanvasNodes();
  const pairs = contrastPairs(document.documentElement, sheet);
  expect(pairs.length).toBeGreaterThanOrEqual(atLeast);
  return belowThreshold(pairs);
}

describe("il foglio di stile, letto come regole", () => {
  test("NESSUN COLORE DENTRO UNA REGOLA-@, che questo motore non guarda", () => {
    // Se qualcuno ne scrive uno, la misura qui sotto diventerebbe cieca senza
    // diventare rossa: è esattamente il difetto che questa prova chiude.
    expect(sheet.colorsInsideAtRules).toBe(0);
  });

  test("le regole lette sono tante quante il foglio ne ha", () => {
    // Un parser che si ferma alla prima stranezza direbbe «zero sotto soglia»
    // per il motivo sbagliato. Il numero esatto non conta; l'ordine sì.
    expect(sheet.rules.length).toBeGreaterThan(200);
  });
});

/**
 * Porta la finestra sulla tela dei flussi.
 *
 * **LA FINESTRA NON SI APRE PIÙ DI LÌ**, e queste due scene lo hanno scoperto
 * cadendo: dal 31/08/2026 si apre su «Adesso», e la tela sta dietro un posto
 * che va scelto. Erano scese da 79 accoppiate a 8 senza che una riga del
 * disegno fosse peggiorata — la misura era diventata cieca, e lo ha detto.
 */
function goToFlows(): void {
  const place = screen.getByRole("button", { name: /^Flussi/ });
  fireEvent.click(place);
}

describe("la prima schermata: cosa sta succedendo adesso", () => {
  test("le corse aperte restano leggibili, tutte e due gli stati", () => {
    // Si disegna `RunGroup` e non `Now`: fuori dal guscio nativo `Now` non ha
    // un deposito a cui chiedere e mostrerebbe una frase sola. Misurare quella
    // frase e chiamarla «la prima schermata» è il modo esatto in cui questo
    // controllo tornerebbe a essere una decorazione.
    const runs: OpenRun[] = [
      {
        run_id: "run-01JZ",
        entity: "esamina-la-repo",
        state: "waiting",
        open_steps: 0,
        open_now: [],
        since: 0,
        started_here: true,
      },
      {
        run_id: "run-01K0",
        entity: "",
        state: "working",
        open_steps: 3,
        open_now: [
          { step_id: "implementa", attempt: 1, open_for_secs: 412 },
          { step_id: "prove", attempt: 2, open_for_secs: 31 },
        ],
        since: 0,
        started_here: false,
      },
    ];
    render(
      <div className="app">
        <RunGroup title="Aspettano te" note="ferme finché non fai qualcosa" runs={runs} now={9000} onOpen={() => {}} />
      </div>,
    );
    expect(screen.getByText("aspetta te")).toBeTruthy();
    expect(screen.getByText("senza nome")).toBeTruthy();
    // Il tentativo che non e' il primo: un passo aperto alla seconda volta
    // vuol dire che il primo giro e' caduto, ed e' quello che una riga verde
    // nasconde.
    expect(screen.getByText("2ª volta")).toBeTruthy();
    expect(measure(20)).toEqual([]);
  });
});

/**
 * Finge il guscio nativo, con risposte scritte a mano.
 *
 * **SI MISURANO I COMPONENTI VERI, NON UN LORO FRAMMENTO.** Estrarre la
 * tabella per poterla disegnare da sola misurerebbe una cosa che nella finestra
 * non esiste: basterebbe un colore messo sul contenitore attorno perché la
 * misura restasse verde e lo schermo no. Qui la risposta del motore è finta e
 * il componente è quello.
 */
function pretendShell(answers: Record<string, unknown>): () => void {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: (command: string) => Promise.resolve(answers[command]) },
  };
  return () => {
    (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
  };
}

describe("la storia delle esecuzioni", () => {
  test("una corsa rotta, una andata e una aperta restano leggibili", async () => {
    const stop = pretendShell({
      execution_history: [
        {
          run_id: "r1", kind: "flow", entity: "sviluppa-sailor", status: "failed",
          started_at: 1000, ended_at: 1100, duration_secs: 100, total_cost_micros: 412000,
          error: "il passo «prove» è caduto: 1 failed", steps_total: 5, steps_went: 3,
          steps_broke: 1, steps_retried: 2, steps_open: [],
          tokens: { input_tokens: 1, output_tokens: 1, cached_tokens: 0, cache_write_tokens: 0, cost_micros: 412000, calls: 1, calls_without_tokens: 0, calls_without_cost: 0 },
          tokens_by_model: {},
          calls: [
            {
              call_id: "c1", step_id: "implementa", purpose: "implementa", cli: "claude-code",
              requested_model: "sonnet", actual_model: "claude-sonnet-4-6", input_tokens: 12000,
              output_tokens: 3000, cached_tokens: 90000, cache_write_tokens: 400, total_tokens: 105400,
              turns: 8, cost_micros: 412000, declared_cost_micros: 409000, error_type: null,
              started_at: 1000, ended_at: 1100,
            },
            {
              // La chiamata che non ha detto niente: «non detto» e non zero.
              call_id: "c2", step_id: "prove", purpose: "prove", cli: "codex",
              requested_model: "", actual_model: "", input_tokens: null, output_tokens: null,
              cached_tokens: null, cache_write_tokens: null, total_tokens: null, turns: null,
              cost_micros: null, declared_cost_micros: null, error_type: "uscita 3",
              started_at: 1050, ended_at: 1090,
            },
          ],
        },
        {
          run_id: "r2", kind: "flow", entity: "relay", status: "succeeded",
          started_at: 900, ended_at: 950, duration_secs: 50, total_cost_micros: 0,
          error: null, steps_total: 2, steps_went: 2, steps_broke: 0, steps_retried: 0, steps_open: [],
          tokens: { input_tokens: 0, output_tokens: 0, cached_tokens: 0, cache_write_tokens: 0, cost_micros: 0, calls: 0, calls_without_tokens: 0, calls_without_cost: 0 },
          tokens_by_model: {}, calls: [],
        },
        {
          run_id: "r3", kind: "flow", entity: "", status: "running",
          started_at: 800, ended_at: null, duration_secs: null, total_cost_micros: 3000,
          error: null, steps_total: 4, steps_went: 1, steps_broke: 0, steps_retried: 0,
          steps_open: [{ step_id: "implementa", attempt: 1, started_at: 800, open_for_secs: 300 }],
          tokens: { input_tokens: 0, output_tokens: 0, cached_tokens: 0, cache_write_tokens: 0, cost_micros: 3000, calls: 1, calls_without_tokens: 1, calls_without_cost: 0 },
          tokens_by_model: {}, calls: [],
        },
      ],
    });
    try {
      render(
        <div className="app">
          <History native />
        </div>,
      );
      await screen.findByText("rotta");
      expect(screen.getByText("andata")).toBeTruthy();
      expect(screen.getByText("aperta")).toBeTruthy();
      // IL COSTO CALCOLATO E QUELLO DICHIARATO, AFFIANCATI. Se divergono, il
      // posto in cui accorgersene e' questo — ed e' il controllo che manca ai
      // tre strumenti di osservabilita' con bug pubblici sui numeri.
      // Due volte: il totale della corsa e la chiamata che lo compone.
      expect(screen.getAllByText("0,412 $").length).toBe(2);
      expect(screen.getByText("0,409 $")).toBeTruthy();
      // Una chiamata che non ha dichiarato token non ne ha consumati zero.
      expect(screen.getAllByText("non detto").length).toBeGreaterThan(0);
      expect(measure(30)).toEqual([]);
    } finally {
      stop();
    }
  });
});

describe("cosa è installato", () => {
  test("i tre stati di raggiungibilità restano leggibili, col motivo", async () => {
    const stop = pretendShell({
      machine_inventory: {
        entries: [
          { kind: "skill", name: "handoff", description: "La staffetta fra sessioni.", origin: "casa", path: "/a", reach: { state: "active" }, by_model: true },
          { kind: "agent", name: "verificatore", description: "Chi crea non giudica.", origin: "repo sailor", path: "/b", reach: { state: "inactive", reason: "il plugin che la contiene è spento" }, by_model: true },
          { kind: "rule", name: "R05", description: "I permessi.", origin: "un altro repo", path: "/c", reach: { state: "unknown", reason: "dipende da dove si apre la sessione" }, by_model: false },
        ],
        roots: ["/work", "/work/sailor"],
        stale_plugin_copies: 2,
      },
    });
    try {
      render(
        <div className="app">
          <Installed native />
        </div>,
      );
      await screen.findByText("attiva");
      expect(screen.getByText("spenta")).toBeTruthy();
      expect(screen.getByText("non lo so")).toBeTruthy();
      // Il motivo è tutto il valore della terza voce: se sparisse, «spenta»
      // resterebbe una parola che non si può correggere.
      expect(screen.getByText("il plugin che la contiene è spento")).toBeTruthy();
      expect(measure(30)).toEqual([]);
    } finally {
      stop();
    }
  });
});

describe("i comandi, dichiarati dal binario", () => {
  test("il letterale e il buco da riempire si distinguono senza sbiadire", async () => {
    // **UNA PAGINA CHE NESSUNA SCENA VISITA NON È VERIFICATA.** Lo stile dei
    // comandi è nato il 01/09/2026 e la batteria è rimasta verde senza averlo
    // mai disegnato: il divieto 6 vale su ogni parola che qualcuno legge, e
    // qui la tentazione più forte è proprio quella vietata — distinguere
    // `<nome>` dal testo letterale sbiadendolo, che è il divieto 7.
    const stop = pretendShell({
      manual: [
        {
          name: "flow",
          description: "elenca, controlla, esegue o riprende i flussi dichiarati in flows/",
          usage: ["sailor flow list", "sailor flow run <nome> [mandato]"],
        },
        {
          name: "version",
          description: "la versione di questo binario",
          usage: ["sailor version"],
        },
      ],
    });
    try {
      render(
        <div className="app">
          <Manual native />
        </div>,
      );
      await screen.findByText("sailor flow");
      // Aprire un comando è ciò che mostra le forme: a pagina chiusa lo stile
      // delle righe d'uso non verrebbe mai disegnato, e la misura sarebbe fatta
      // su ciò che non si vede.
      fireEvent.click(screen.getByText("sailor flow"));
      expect(screen.getByText("<nome>")).toBeTruthy();
      expect(screen.getByText("[mandato]")).toBeTruthy();
      expect(measure(20)).toEqual([]);
    } finally {
      stop();
    }
  });
});

describe("il riepilogo di oggi", () => {
  test("CIÒ CHE NON È STATO MISURATO SI LEGGE, e non è sbiadito", async () => {
    // La riga più importante di tutta la schermata, e quella che negli altri
    // strumenti manca: `--warn` su `--bg` deve stare sopra 4,5:1 come tutto il
    // resto, altrimenti l'avvertimento c'è e non si legge.
    const stop = pretendShell({
      open_runs: [],
      day_summary: {
        ledger_present: true, runs: 13, went: 11, broke: 2, still_open: 0,
        input_tokens: 400000, output_tokens: 20000, cached_tokens: 900000, cache_write_tokens: 5000,
        cost_micros: 1840000, unmeasured: 3, unpriced: 1, tokens_by_model: {},
      },
    });
    try {
      render(
        <div className="app">
          <Now native onOpen={() => {}} />
        </div>,
      );
      await screen.findByText(/chiamate senza token/);
      expect(screen.getByText("Non sta girando niente, e niente aspetta te.")).toBeTruthy();
      // 13 accoppiate: e' la scena piu' scarna che la finestra sappia mostrare
      // — il riepilogo e una frase — e va bene cosi', perche' e' anche quella
      // che una macchina tranquilla mostra per ore.
      expect(measure(12)).toEqual([]);
    } finally {
      stop();
    }
  });
});

describe("un deposito che non c'e'", () => {
  test("«NON LO SO» NON DIVENTA «ZERO»", () => {
    // Questa riga esisteva nella plancia — `the_embedded_page_marks_missing_
    // ledger_counts_as_unknown` — ed e' l'unica delle sue quattordici prove che
    // difendeva una distinzione invece di un trasporto. Riscrivere la vista
    // senza riscrivere il controllo l'avrebbe persa in silenzio: e' esattamente
    // il modo in cui Airflow 3 si e' accorto otto mesi dopo di aver tolto la
    // vista calendario.
    //
    // Zero corse e' una macchina tranquilla. Un deposito che non esiste e' una
    // macchina di cui non sappiamo niente, e chi legge deve poterle distinguere
    // prima di concludere che oggi non e' successo nulla.
    const stop = pretendShell({
      open_runs: [],
      day_summary: {
        ledger_present: false, runs: 0, went: 0, broke: 0, still_open: 0,
        input_tokens: 0, output_tokens: 0, cached_tokens: 0, cache_write_tokens: 0,
        cost_micros: 0, unmeasured: 0, unpriced: 0, tokens_by_model: {},
      },
    });
    try {
      render(
        <div className="app">
          <Now native onOpen={() => {}} />
        </div>,
      );
      return screen.findByText(/non e' la stessa cosa|non è la stessa cosa/).then(() => {
        // E nessun numero: uno zero scritto accanto a «non lo so» si legge
        // comunque come zero.
        expect(screen.queryByText("0")).toBeNull();
        stop();
      });
    } catch (error) {
      stop();
      throw error;
    }
  });
});

describe("la finestra a riposo", () => {
  test("nessuna accoppiata testo/sfondo sotto 4,5:1", () => {
    render(<App />);
    goToFlows();
    // 79 accoppiate quando questa riga è stata scritta: la soglia lascia
    // margine a chi toglie un pezzo di finestra, non a chi ne perde metà.
    expect(measure(70)).toEqual([]);
  });
});

describe("la finestra con un flusso a fuoco", () => {
  test("SPEGNERE UNA CORSIA NON SPEGNE LE SUE PAROLE", () => {
    // La misura dichiarata dal commit — «54 misurate, 0 sotto soglia» — valeva
    // solo sulla vista d'apertura. Un clic su un flusso nella colonna metteva
    // `data-dimmed` sulle altre corsie, `opacity: 0.3`, e cinque accoppiate
    // sotto soglia: la descrizione a 1,47:1. Lo stesso meccanismo che il
    // divieto 5 condanna altrove, e più aggressivo di quello che ha tolto.
    const { container } = render(<App />);
    goToFlows();
    const rail = Array.from(container.querySelectorAll("button.rail__item")).find(
      (button) => button.querySelector(".rail__label")?.textContent === "relay",
    );
    fireEvent.click(rail as HTMLElement);
    expect(document.querySelectorAll("[data-dimmed]").length).toBeGreaterThan(0);
    expect(measure(70)).toEqual([]);
  });
});

const STEP: Step = {
  id: "working-tree-is-clean",
  action: "shell_check",
  deps: [],
  with: {},
  species: "repeatable",
  max_attempts: 3,
} as unknown as Step;

const EVERY_STATE: StepState[] = [
  "waiting",
  "running",
  "went",
  "broke",
  "capped",
  "handed_to_human",
];

function stepProps(data: StepNodeData): NodeProps {
  return {
    id: "n",
    type: "step",
    data,
    selected: false,
    zIndex: 0,
    isConnectable: false,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
  } as unknown as NodeProps;
}

describe("i sei stati di un passo, e le corsie", () => {
  test("ogni stato resta leggibile, a fuoco e spento", () => {
    // I dati d'esempio non producono `capped` né `handed_to_human`: senza
    // questa scena due dei sei finali non verrebbero misurati da nessuno, e
    // sono proprio quelli che la tela mostra di rado e nessuno guarda.
    const runs = new Map<string, StepRun>(
      EVERY_STATE.map((state) => [
        `flow-${state}::working-tree-is-clean`,
        { step_id: "working-tree-is-clean", state, attempt: 2 } as StepRun,
      ]),
    );
    const band: FlowBandData = {
      name: "prima-corsa",
      description: "Il flusso più piccolo che esista: un controllo solo.",
      stepCount: 1,
      color: "#2563eb",
      dimmed: false,
    };

    render(
      <ReactFlowProvider>
        <div className="app">
          {[false, true].map((dimmed) =>
            EVERY_STATE.map((state) => (
              <StepNode
                key={`${state}-${String(dimmed)}`}
                {...stepProps({
                  step: STEP,
                  kind: "engine",
                  run: runs.get(`flow-${state}::working-tree-is-clean`),
                  flowName: `flow-${state}`,
                  color: "#2563eb",
                  dimmed,
                })}
              />
            )),
          )}
          {[false, true].map((dimmed) => (
            <FlowBandNode
              key={`band-${String(dimmed)}`}
              {...(stepProps({ ...band, dimmed } as unknown as StepNodeData) as NodeProps)}
            />
          ))}
        </div>
      </ReactFlowProvider>,
    );

    expect(screen.getAllByText("fermo al tetto").length).toBeGreaterThan(0);
    expect(measure(80)).toEqual([]);
  });
});
