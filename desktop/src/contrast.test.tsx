// @vitest-environment jsdom
import stylesheetSource from "./styles.css?raw";
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { FlowBandNode, StepNode, type FlowBandData, type StepNodeData } from "./StepNode";
import { t } from "./i18n";
import { Now, RunGroup } from "./Now";
import { History } from "./History";
import { Installed } from "./Installed";
import { Manual } from "./Manual";
import type { OpenRun } from "./engine";
import type { Step, StepRun, StepState } from "./flow";
import { belowThreshold, contrastPairs, inDark, parseStylesheet, type Stylesheet } from "./contrast";

/**
 * **PROHIBITION 6, MEASURED ON THE PAINTED DOM, INSIDE `npm test`:** no
 * text/background pair below 4.5:1, as declared at the top of `styles.css`.
 * **THREE SCENES, NOT ONE:** at rest, with a flow focused, and one node per
 * each of the six states — including those the sample data never produces.
 */

afterEach(cleanup);

// React Flow measures its own box on mount: outside a real browser nobody does
// it, and without these two the canvas never mounts at all.
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
 * React Flow keeps a node `visibility: hidden` until a `ResizeObserver` has
 * measured it, and there is none here: without this the whole canvas would stay
 * outside the measurement and the check would say "zero below threshold" having
 * looked at nothing. In a real browser those nodes are visible.
 */
function revealCanvasNodes(): void {
  for (const node of Array.from(document.querySelectorAll<HTMLElement>(".react-flow__node"))) {
    if (node.style.visibility === "hidden") node.style.visibility = "visible";
  }
}

/**
 * The DOM to measure is the whole document: `:root` carries the roles.
 *
 * **WHOEVER MEASURES MUST BE MEASURED.** Each scene declares how many pairs it
 * expects to find: a check that looks at nothing passes anyway.
 */
function measure(atLeast: number): string[] {
  revealCanvasNodes();
  const light = contrastPairs(document.documentElement, sheet);
  expect(light.length).toBeGreaterThanOrEqual(atLeast);
  // EVERY SCENE TWICE: the dark scheme is the same sheet with the roles
  // swapped, so it is measured on the same DOM and must find the same pairs.
  const dark = contrastPairs(document.documentElement, inDark(sheet));
  expect(dark.length).toBe(light.length);
  return [...belowThreshold(light), ...belowThreshold(dark).map((pair) => `dark: ${pair}`)];
}

describe("the stylesheet, read as rules", () => {
  test("NO COLOR INSIDE AN @-RULE, which this engine does not look at", () => {
    // If someone writes one, the measurement below would go blind without going
    // red: exactly the flaw this test closes.
    expect(sheet.colorsInsideAtRules).toBe(0);
  });

  test("THE DARK SCHEME IS READ, so every scene below is measured twice", () => {
    // Without this the dark measurement would silently be the light one again.
    expect(sheet.darkRoot).not.toBeNull();
    expect(new Map(sheet.darkRoot ?? []).get("--bg")).not.toBe(new Map(sheet.rules.find((r) => r.selector === ":root")?.declarations ?? []).get("--bg"));
  });

  test("as many rules are read as the sheet has", () => {
    // A parser that stops at the first oddity would say "zero below threshold"
    // for the wrong reason. The exact number does not matter; the order does.
    expect(sheet.rules.length).toBeGreaterThan(200);
  });
});

/**
 * Takes the window to the flows canvas. **IT DOES NOT OPEN THERE:** the window
 * opens on «Now» and the canvas sits behind a place that has to be chosen,
 * so a scene that skips this measures a screen it did not mean to.
 */
function goToFlows(): void {
  const place = screen.getByRole("button", { name: /^Board/ });
  fireEvent.click(place);
}

describe("the first screen: what is happening right now", () => {
  test("open runs stay legible, in both states", () => {
    // We render `RunGroup` and not `Now`: outside the native shell `Now` has no
    // store to ask and would show a single sentence. Measuring that sentence and
    // calling it "the first screen" is exactly how this check would go back to
    // being decoration.
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
    expect(screen.getByText("waits for you")).toBeTruthy();
    expect(screen.getByText("unnamed")).toBeTruthy();
    // An attempt that is not the first: a step open on the second try means the
    // first round fell over, and that is what a green line hides.
    expect(screen.getByText("attempt 2")).toBeTruthy();
    expect(measure(20)).toEqual([]);
  });
});

/**
 * Fakes the native shell, with hand-written answers. **MEASURE THE REAL
 * COMPONENTS, NOT A FRAGMENT:** a table extracted so it can stand alone is not
 * what the window has — a color on the container around it would keep this
 * green while the screen stayed unreadable.
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

describe("the run history", () => {
  test("a broken, a finished and an open run all stay legible", async () => {
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
              // The call that said nothing: «non detto», not zero.
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
      await screen.findByText("broke");
      expect(screen.getByText("went")).toBeTruthy();
      expect(screen.getByText("open")).toBeTruthy();
      // COMPUTED COST AND DECLARED COST, SIDE BY SIDE. If they diverge, this is
      // where you notice — the check the three observability tools with public
      // bugs on their numbers are missing. Twice: the run total and the call
      // that makes it up.
      expect(screen.getAllByText("$0.412").length).toBe(2);
      expect(screen.getByText("$0.409")).toBeTruthy();
      // A call that declared no tokens did not consume zero of them.
      expect(screen.getAllByText("not said").length).toBeGreaterThan(0);
      expect(measure(30)).toEqual([]);
    } finally {
      stop();
    }
  });
});

describe("what is installed", () => {
  test("the three reachability states stay legible, with the reason", async () => {
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
      await screen.findByText("active");
      expect(screen.getByText("switched off")).toBeTruthy();
      expect(screen.getByText("not known")).toBeTruthy();
      // The reason is the whole value of the third entry: without it, «spenta»
      // stays a word nobody can act on.
      expect(screen.getByText("il plugin che la contiene è spento")).toBeTruthy();
      expect(measure(30)).toEqual([]);
    } finally {
      stop();
    }
  });
});

describe("the commands, as the binary declares them", () => {
  test("literal text and the blank to fill differ without fading", async () => {
    // **A PAGE NO SCENE VISITS IS NOT VERIFIED.** Prohibition 6 holds on every
    // word anybody reads, and here the strongest temptation is the forbidden
    // one — telling `<nome>` apart from literal text by fading it, which is
    // prohibition 7.
    const stop = pretendShell({
      manual: [
        {
          name: "flow",
          description: "elenca, controlla, esegue o riprende i flussi dichiarati in flows/",
          usage: [
            { form: "sailor flow list", says: "" },
            { form: "sailor flow run <nome> [mandato]", says: "esegue un flusso" },
          ],
        },
        {
          name: "version",
          description: "la versione di questo binario",
          usage: [{ form: "sailor version", says: "" }],
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
      // Opening a command is what shows the shapes: with the page closed the
      // usage lines would never be painted, and the measurement would be made on
      // what nobody sees.
      fireEvent.click(screen.getByText("sailor flow"));
      expect(screen.getByText("<nome>")).toBeTruthy();
      expect(screen.getByText("[mandato]")).toBeTruthy();
      expect(measure(20)).toEqual([]);
    } finally {
      stop();
    }
  });
});

describe("today's summary", () => {
  test("WHAT WAS NOT MEASURED IS READABLE, and not faded", async () => {
    // The most important line on the whole screen, and the one the other tools
    // lack: `--warn` on `--bg` must stay above 4.5:1 like everything else, or
    // the warning is there and cannot be read.
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
      await screen.findByText(/calls without tokens/);
      expect(screen.getByText("Nothing is running, and nothing waits for you.")).toBeTruthy();
      // The barest scene the window can show — the summary and one sentence —
      // and that is fine, because it is also the one a quiet machine shows for
      // hours on end.
      expect(measure(12)).toEqual([]);
    } finally {
      stop();
    }
  });
});

describe("a ledger that is not there", () => {
  test("«NON LO SO» NEVER BECOMES «ZERO»", () => {
    // Zero runs is a quiet machine. A ledger that does not exist is a machine we
    // know nothing about, and whoever reads must be able to tell the two apart
    // before concluding that nothing happened today.
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
      return screen.findByText(/not the same as/).then(() => {
        // And no number: a zero written next to «non lo so» still reads as zero.
        expect(screen.queryByText("0")).toBeNull();
        stop();
      });
    } catch (error) {
      stop();
      throw error;
    }
  });
});

describe("the window at rest", () => {
  test("no text/background pair below 4.5:1", () => {
    render(<App />);
    goToFlows();
    // The threshold leaves room for someone removing a piece of the window, not
    // for someone losing half of it.
    expect(measure(70)).toEqual([]);
  });
});

describe("the window with a flow focused", () => {
  test("DIMMING A LANE MUST NOT DIM ITS WORDS", () => {
    // Clicking a flow in the column puts `data-dimmed` on the other lanes with
    // `opacity: 0.3`, which sank their description to 1.47:1. It is the same
    // mechanism prohibition 5 condemns elsewhere.
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

describe("the six states of a step, and the lanes", () => {
  test("every state stays legible, focused and dimmed", () => {
    // The sample data produces neither `capped` nor `handed_to_human`: without
    // this scene two of the six endings would be measured by nobody, and they
    // are exactly the ones the canvas rarely shows.
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

    expect(screen.getAllByText(t("window.step.state.capped")).length).toBeGreaterThan(0);
    expect(measure(80)).toEqual([]);
  });
});
