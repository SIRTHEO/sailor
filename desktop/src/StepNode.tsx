import { createContext, useContext, type ReactNode } from "react";
import { Handle, Position, useStore, type NodeProps } from "@xyflow/react";
import { stepCountLabel, type Step, type StepKind, type StepRun, type StepState } from "./flow";
import { nodeId, type PortShape, type StepPort, type StepPorts } from "./layout";
import { MODEL_KEY, TOOL_KEY, useTool, useToolsAreKnown } from "./tools";
import { ToolMark } from "./ToolMark";
import { formatCost, formatTokens, usageIsPartial, type StepUsage } from "./stepusage";
import { group, t } from "./i18n";

/**
 * The real step states, keyed `flow::step`. **THROUGH A CONTEXT AND NOT THE
 * NODE'S `data`**: `data` is part of the node list, and a list rebuilt on every
 * incoming fact loops this canvas. Facts change the context value, not the
 * nodes' identity, so only what changed redraws.
 */
export const StepRunContext = createContext<Map<string, StepRun>>(new Map());

/**
 * The second the window is at, ticking only while something runs. A node needs
 * it to know when a step has gone quiet: silence is not a fact anybody sends.
 */
export const NowContext = createContext<number>(0);

/** How long after its last piece of output a step is still «speaking». */
export const STILL_SPEAKING_SECS = 2;

/**
 * What each step spent, and on which model, keyed `flow::step`. Through a
 * context for the same reason as the state: the spend refreshes every three
 * seconds while the run goes, and rebuilding the node list each time loops it.
 */
export const StepUsageContext = createContext<Map<string, StepUsage>>(new Map());

/**
 * How a node asks for the menu of what could follow it — the same menu the wire
 * opens, reachable by pointing. Dragging is slower and three times as
 * error-prone, and a gesture reachable only by dragging fails WCAG 2.2 SC 2.5.7.
 * Null when nobody listens: the node then draws no button at all.
 */
export const WireContext = createContext<((stepId: string, flowName: string, at: { x: number; y: number }) => void) | null>(
  null,
);

/**
 * Below this zoom a node stops pretending to be readable. Not a number of
 * taste: at 0.5 — the zoom the canvas picks by itself with two flows open — a
 * lane's name renders 6.5px tall and its description 5.5px. They stay drawn,
 * and nobody can read them.
 */
const FAR_ZOOM = 0.62;

export interface StepNodeData extends Record<string, unknown> {
  step: Step;
  kind: StepKind;
  run?: StepRun;
  /** The flow the step belongs to, and its lane colour on the single canvas. */
  flowName: string;
  color: string;
  /** What goes in and out, read from the graph. Absent only in test data. */
  ports?: StepPorts;
}

/**
 * Colour says how a step ended, and the endings are not interchangeable: capped
 * is not broken, waiting on a person is not a failure. Roles, not tints: as
 * copied hexes they stayed in the day scheme after the ground went dark, and
 * the minimap was the only thing on screen still lit from noon.
 */
export const STATE_COLOR: Record<StepState, string> = {
  idle: "var(--state-waiting)",
  waiting: "var(--state-waiting)",
  running: "var(--state-running)",
  went: "var(--state-went)",
  broke: "var(--state-broke)",
  capped: "var(--state-capped)",
  handed_to_human: "var(--state-human)",
  skipped: "var(--state-waiting)",
};

const STATE_LABEL = group("window.step.state.") as Record<StepState, string>;

/** What a running step says instead, while its output is arriving. */
const SPEAKING_LABEL = t("window.step.speaking");

/**
 * **TWO REGISTERS OF ATTENTION.** Live runs share one quiet breathing dot that
 * says "alive", not "look at me". Singling out is kept for the one thing a view
 * holds that will not unblock itself, since Von Restorff works only while very
 * few elements deviate: hence the two below. "Broken, retrying" retries alone.
 */
const GESTURE_GRAVITY: Partial<Record<StepState, number>> = {
  handed_to_human: 0,
  capped: 1,
};

export interface GestureCall {
  /** The `flow::step` key of the single singled-out node, or `null`. */
  key: string | null;
  /** How many things wait on a person in all, including the singled-out one. */
  waiting: number;
}

/**
 * Which of all the runs the window knows about gets singled out. It lives here
 * and not in the layout because it reads the REAL run state, which comes
 * through a context: computing it where the node list is built would make it
 * one fact out of date.
 */
export function stepThatCallsForAGesture(runs: Map<string, StepRun>): GestureCall {
  const calling = Array.from(runs.entries()).filter(
    ([, run]) => GESTURE_GRAVITY[run.state] !== undefined,
  );
  if (calling.length === 0) return { key: null, waiting: 0 };
  // Ties are broken by name, which does not change from one fact to the next:
  // a highlight that hops between nodes on every update is not a highlight.
  calling.sort(([leftKey, left], [rightKey, right]) => {
    const gravity =
      (GESTURE_GRAVITY[left.state] as number) - (GESTURE_GRAVITY[right.state] as number);
    return gravity !== 0 ? gravity : leftKey.localeCompare(rightKey);
  });
  return { key: calling[0][0], waiting: calling.length };
}

/** What the type carried by a shape is called, in words. */
const SHAPE_LABEL: Record<PortShape, string> = {
  text: "testo",
  structure: "struttura",
  value: "valore",
};

/**
 * A port: the shape says the type, the fill says whether it is wired. **COLOUR
 * CARRIES NOTHING ON ITS OWN** (rule 5) — circle, diamond and square survive in
 * black and white, and a required input nobody feeds adds THE WORD, since a
 * hollow shape says "not wired" but not "and it had to be".
 */
function Port({ port }: { port: StepPort }) {
  const missing = port.required && !port.wired;
  return (
    <span
      className="step-node__port"
      data-wired={port.wired || undefined}
      data-missing={missing || undefined}
      title={`${port.name} · ${SHAPE_LABEL[port.shape]} · ${port.wired ? "wired" : "not wired"}`}
    >
      <span className="step-node__port-mark" data-shape={port.shape} aria-hidden="true" />
      <span className="step-node__port-name">{missing ? `${port.name} missing` : port.name}</span>
    </span>
  );
}

/**
 * The node's ports: inputs on the left, output on the right, on the side the
 * wires really enter and leave from.
 */
function StepPortsRow({ ports }: { ports: StepPorts }) {
  return (
    <div className="step-node__ports">
      <div className="step-node__ports-column">
        {ports.inputs.map((port) => (
          <Port key={port.name} port={port} />
        ))}
      </div>
      <div className="step-node__ports-column" data-out>
        <Port port={ports.output} />
      </div>
    </div>
  );
}

/**
 * The glyph of each species, drawn with the stroke of whatever inherits it.
 *
 * A SHAPE NEXT TO THE WORD, NEVER INSTEAD OF IT (prohibition 5), and never a
 * tint of its own: a species is not a machine state, and prohibition 4 keeps
 * colour for those. Nine kinds are told apart in greyscale by these outlines.
 */
const KIND_ICON: Record<StepKind, ReactNode> = {
  trigger: <path d="M13 2L4.5 13.5H11l-1 8.5L19.5 10H13z" fill="currentColor" stroke="none" />,
  engine: (
    <>
      <rect x="4" y="7" width="16" height="12" rx="2.5" />
      <path d="M12 7V4" />
      <circle cx="9" cy="13" r="1.2" fill="currentColor" />
      <circle cx="15" cy="13" r="1.2" fill="currentColor" />
    </>
  ),
  check: <path d="M4 12.5l5 5L20 6.5" />,
  wait: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M12 8v4.5l3 1.8" />
    </>
  ),
  branch: (
    <>
      <path d="M6 4v7a6 6 0 0012 0V4" />
      <path d="M12 21v-4" />
    </>
  ),
  deposit: (
    <>
      <path d="M4 7h16" />
      <path d="M4 12h16" />
      <path d="M4 17h9" />
    </>
  ),
  gesture: (
    <>
      <path d="M5 12h14" />
      <path d="M13 6l6 6-6 6" />
    </>
  ),
  human: (
    <>
      <circle cx="12" cy="8" r="3.4" />
      <path d="M5.5 20a6.5 6.5 0 0113 0" />
    </>
  ),
  subflow: (
    <>
      <rect x="3" y="4" width="8" height="8" rx="2" />
      <rect x="13" y="12" width="8" height="8" rx="2" />
      <path d="M11 8h2a1 1 0 011 1v3" />
    </>
  ),
};

export function KindIcon({
  kind,
  className = "step-node__icon",
}: {
  kind: StepKind;
  className?: string;
}) {
  return (
    <span className={className} aria-hidden="true">
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {KIND_ICON[kind]}
      </svg>
    </span>
  );
}

/**
 * **THE ONE STATE THAT WILL NOT UNBLOCK ITSELF GETS A SHAPE OF ITS OWN.** Every
 * other ending is a tint and a word: this one is a request, and a request has
 * to be findable across a screen without reading. A silhouette and not strokes,
 * because at eleven pixels line art becomes a smudge.
 */
export function HandMark({ className = "step-node__hand" }: { className?: string }) {
  return (
    <span className={className} aria-hidden="true">
      <svg viewBox="0 0 24 24">
        <path
          fill="currentColor"
          d="M12 2a1.5 1.5 0 00-1.5 1.5V11h-1V5.2a1.5 1.5 0 10-3 0V13l-.8-1.4a1.45 1.45 0 10-2.5 1.4l2.8 4.9A6.2 6.2 0 0011.3 21h1.4a6 6 0 006-6V7.2a1.5 1.5 0 10-3 0V11h-1V3.5A1.5 1.5 0 0012 2z"
        />
      </svg>
    </span>
  );
}

/**
 * How long a step took, at the precision that answers the question: under ten
 * seconds a tenth separates «instant» from «slow», and over a minute nobody
 * compares tenths. Comma for the decimal, like every other number here.
 */
export function formatElapsed(seconds: number): string {
  if (seconds < 10) return `${seconds.toFixed(1)} s`;
  if (seconds < 60) return `${Math.round(seconds)} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ${Math.round(seconds - minutes * 60)} s`;
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

/**
 * **ONE LIST, NOT TWO.** The inspector kept its own copy of these words, and
 * the two had drifted: a step was «agente» on the board and «engine» in the
 * panel, «a una persona» here and «person» there. Same nine keys, two files,
 * nothing that could tell either of them it was wrong.
 */
export const KIND_LABEL = group("window.step.kind.") as Record<StepKind, string>;

/** The model the step chose, if it chose one. */
function modelOf(step: Step): string {
  const model = step.with?.[MODEL_KEY];
  return typeof model === "string" ? model : "";
}

/**
 * The engines the step names, **in the order it names them**. `tool` IS NOT
 * JUST A STRING: in `crates/actions/src/lib.rs` it is a `ToolChoice`, `One` or
 * `Chain`, "who to run, best first". Read as a plain string it answers nothing
 * on a chain. The first is shown as the one tried, the rest as its spares.
 */
export function enginesOf(params: Record<string, unknown> | null | undefined): string[] {
  const declared = params?.[TOOL_KEY];
  if (typeof declared === "string") return declared === "" ? [] : [declared];
  if (!Array.isArray(declared)) return [];
  return declared.filter((id): id is string => typeof id === "string" && id !== "");
}

/**
 * What runs this step, on the canvas: the tool's mark, its name, and the model.
 * A MISSING TOOL DOES NOT VANISH, it greys out with the reason detection holds
 * — otherwise a node that cannot run here looks fine and fails at start. Until
 * discovery answers, nobody is accused: the id written in the step is shown.
 */
function StepTool({
  id,
  model,
  actual,
  fallbacks,
}: {
  id: string;
  model: string;
  actual: string[];
  /** The spares declared after the first, in the order written in the step. */
  fallbacks: string[];
}) {
  const tool = useTool(id);
  const known = useToolsAreKnown();
  // Three states, not two: found and present, found and absent, and "not known
  // yet". The third is not the second, and greying it out would be a lie that
  // lasts as long as detection does.
  const off = known && !tool?.available;
  const why = tool?.reason ?? (known ? "not among the tools found on this machine" : "");
  // The real model shows only when it says something different from the one
  // written in the step: repeating the same word twice informs nobody.
  const surprising = actual.filter((name) => name !== model);

  return (
    <div className="step-node__tool" data-off={off || undefined}>
      <ToolMark id={id} size={16} off={off} />
      <div className="step-node__tool-text">
        {/* NOME IN PROSA, IDENTIFICATIVO IN MONOSPAZIO — e questo slot mostra
            l'uno o l'altro. Finché la scoperta non ha risposto qui ci finisce
            `id`, che è un dato: senza distinguerli, `claude-code` compariva in
            due caratteri diversi sulle due righe dello stesso riquadro — in
            prosa sopra, in monospazio nella catena sotto. La regola non cambia,
            cambia il fatto che si legge prima di applicarla. */}
        <span className="step-node__tool-name" data-raw={tool === undefined || undefined} title={id}>
          {tool?.name ?? id}
        </span>
        {model !== "" && <span className="step-node__tool-model">{model}</span>}
        {/* UNA CATENA SI DICE, e prima non si diceva affatto: il nodo mostrava
            «nessun motore» proprio dove il passo ne nominava tre. Chi guarda
            deve sapere che se il primo non c'è ne parte un altro. */}
        {fallbacks.length > 0 && (
          <span
            className="step-node__tool-chain"
            title={`in order of preference: ${[id, ...fallbacks].join(", ")}`}
          >
            if missing: {fallbacks.join(", ")}
          </span>
        )}
        {surprising.length > 0 && (
          <span
            className="step-node__tool-actual"
            title={`the engine answered that it used: ${surprising.join(", ")}`}
          >
            ha girato su {surprising.join(", ")}
          </span>
        )}
        {/* Il motivo si legge sulla tela, non solo passandoci sopra: chi guarda
            un nodo spento non deve andare a cercare dove sta scritto perché. */}
        {off && why !== "" && (
          <span className="step-node__tool-why" title={why}>
            {why}
          </span>
        )}
      </div>
    </div>
  );
}

/**
 * The box that says the engine is undeclared. Absence is a fact and has to be
 * drawn: with no box, a step that runs fine here is indistinguishable from one
 * whose engine nobody looked at. The wording says the FIELD is missing, not the
 * engine, so it does not contradict the action name written under it.
 */
function NoTool({ action }: { action: string }) {
  return (
    <div className="step-node__tool" data-none>
      <div className="step-node__tool-text">
        <span className="step-node__tool-name">engine not declared</span>
        <span className="step-node__tool-model" title={action}>
          {action}
        </span>
      </div>
    </div>
  );
}

/**
 * The tokens in and out of this step, and only once it has called somebody:
 * before that a row of zeros would read as a measurement. What it cost is not
 * here — it sits in the bench row below, once, because two places for one
 * number are two places to read it wrong.
 */
function StepMeter({ usage }: { usage: StepUsage }) {
  return (
    <div className="step-node__meter">
      <span title={`${usage.inputTokens} tokens in`}>↑ {formatTokens(usage.inputTokens)}</span>
      <span title={`${usage.outputTokens} tokens out`}>↓ {formatTokens(usage.outputTokens)}</span>
    </div>
  );
}

export function StepNode({ data, selected }: NodeProps) {
  const { step, kind, run: fromData, flowName, color: flowColor, ports } =
    data as StepNodeData;
  // REAL FACTS BEAT THE SAMPLE. `run` in `data` still exists because that is
  // how the sample data colours the canvas outside the native shell; when a
  // real run exists, its state is the one that counts.
  const key = nodeId(flowName, step.id);
  const runs = useContext(StepRunContext);
  const run = runs.get(key) ?? fromData;
  const usage = useContext(StepUsageContext).get(key);
  const wire = useContext(WireContext);
  // No run is «not run yet», never «waiting»: a node that waits is waiting
  // on something, and a flow nobody ran waits on nothing. Fault 30.
  const state: StepState = run?.state ?? "idle";
  // SPEAKING IS SOMETHING A RUNNING STEP DOES, and it is the only sign that
  // separates an agent at work from one stuck: the breathing dot says «alive»
  // for both. The last piece of output has a second on it; silence has nothing,
  // so it is read off the clock.
  const now = useContext(NowContext);
  const speaking =
    state === "running" &&
    run?.spoke_at !== undefined &&
    now - run.spoke_at <= STILL_SPEAKING_SECS;
  const isAgent = kind === "engine";
  // Which tool runs this node reads off the canvas, not only from the panel:
  // it is the first question asked of a flow somebody else wrote.
  const engines = enginesOf(step.with);
  // How far away the eye is. Below the threshold the node drops the detail
  // instead of drawing it six pixels tall.
  const zoom = useStore((s) => s.transform[2]);
  const far = zoom < FAR_ZOOM;
  // The view's single singled-out node, and the count waiting elsewhere.
  const call = stepThatCallsForAGesture(runs);
  const isolated = call.key === key;

  return (
    <div
      className="step-node"
      data-agent={isAgent || undefined}
      data-state={state}
      data-selected={selected || undefined}
      data-far={far || undefined}
      data-calls={isolated || undefined}
    >
      <div className="step-node__flow" style={{ background: flowColor }} title={flowName} />
      {/* WHAT COULD FOLLOW THIS, without a drag. Hidden from the far zoom with
          everything else that cannot be read there. */}
      {!far && wire && (
        <button
          type="button"
          className="step-node__more"
          aria-label={`what follows «${step.id}»`}
          onClick={(event) => {
            event.stopPropagation();
            const box = event.currentTarget.getBoundingClientRect();
            wire(step.id, flowName, { x: box.right + 4, y: box.top });
          }}
        >
          +
        </button>
      )}
      {/* «This is the part being dimensioned»: two elements, four corners. */}
      {selected && (
        <>
          <span className="step-node__marks" aria-hidden="true" />
          <span className="step-node__marks" data-second aria-hidden="true" />
        </>
      )}
      <Handle type="target" position={Position.Left} />

      {/* THE HEAD SAYS WHAT YOU ARE: the glyph of the species, its name, and
          the identifier of the step. It survives the far zoom, where it is the
          only thing still legible. */}
      {/* WHAT YOU ARE AND HOW YOU ARE, on one band — the two questions asked
          first, answered side by side. Dot plus word, never the colour alone.
          The name gets the full width below: crowded onto this row it pushes
          the state to a second line and the band stops being a band. */}
      <div className="step-node__head">
        <KindIcon kind={kind} />
        <span className="step-node__kind">{KIND_LABEL[kind]}</span>
        {step.when && <span className="step-node__when">condizionato</span>}
        <span className="step-node__state">
          {speaking ? (
            <span className="speaks" aria-hidden="true" />
          ) : state === "handed_to_human" ? (
            <HandMark />
          ) : (
            <span className="step-node__state-dot" aria-hidden="true" />
          )}
          {speaking ? SPEAKING_LABEL : STATE_LABEL[state]}
        </span>
      </div>

      {/* The name, at the size that makes it the figure. It outlives the far
          zoom with the band above it: those two are the plate. */}
      <div className="step-node__id" title={step.id}>
        {step.id}
      </div>

      {!far && (
        <div className="step-node__body">
          {/* DA VICINO IL NODO DICE SEMPRE CON COSA GIRA — anche quando la
              risposta è «con niente». Da lontano no: a quello zoom non si
              leggerebbe. */}
          {engines.length > 0 ? (
            <StepTool
              id={engines[0]}
              model={modelOf(step)}
              actual={usage?.models ?? []}
              fallbacks={engines.slice(1)}
            />
          ) : (
            <NoTool action={step.action} />
          )}

          {usage && usage.calls > 0 && <StepMeter usage={usage} />}

          {ports && <StepPortsRow ports={ports} />}
        </div>
      )}

      {/* THE FOOT SAYS HOW IT WENT, and the duration belongs beside the word,
          not a row apart: they are one answer. The foot is drawn at every zoom
          — from far, a node that says only what it is says half of it. */}
      <div className="step-node__foot">
        {/* Chi tiene il passo è un fatto, e sta bene in fondo: non chiede
            niente a nessuno. */}
        {!far && isAgent && state === "running" && (
          <span className="step-node__pid">pid {run?.held_by_pid ?? "?"}</span>
        )}
        {!far && run && run.attempt > 1 && (
          <span className="step-node__attempt">
            {run.attempt}ª di {step.max_attempts}
          </span>
        )}
        {/* Le altre che aspettano non prendono ciascuna un'evidenza: si
            contano qui, sull'unico nodo isolato. */}
        {!far && isolated && call.waiting > 1 && (
          <span className="step-node__elsewhere">{call.waiting - 1} more waiting</span>
        )}

      </div>

      {/* THE BENCH ROW: three cells parted by a rule, not three cards. Where
          space costs, a rule does a container's work without paying its
          padding twice. A cell nobody measured says so with a dash — never a
          zero, which reads as a measurement. */}
      {!far && (
        <div className="step-node__bench">
          <span className="step-node__cell">
            <span className="step-node__cell-label">attempt</span>
            <span className="step-node__cell-value">
              {run ? `${run.attempt} of ${step.max_attempts}` : `1 of ${step.max_attempts}`}
            </span>
          </span>
          <span className="step-node__cell">
            <span className="step-node__cell-label">took</span>
            <span className="step-node__cell-value">
              {run?.elapsed_secs !== undefined ? formatElapsed(run.elapsed_secs) : "—"}
            </span>
          </span>
          <span className="step-node__cell">
            <span className="step-node__cell-label">cost</span>
            {/* A cost nobody declared is a dash, never a zero: zero would read
                as «it ran for free». A partial one says so with a plus. */}
            <span
              className="step-node__cell-value"
              title={
                usage == null
                  ? undefined
                  : usage.costMicros === null
                    ? "none of this step's calls declares a cost"
                    : usageIsPartial(usage)
                      ? `${usage.callsWithoutCost} calls out of ${usage.calls} declare no cost: the figure is lower than the truth`
                      : `${usage.calls} calls`
              }
            >
              {usage?.costMicros != null
                ? `${formatCost(usage.costMicros)}${usageIsPartial(usage) ? " +" : ""}`
                : "—"}
            </span>
          </span>
        </div>
      )}

      <Handle type="source" position={Position.Right} />
    </div>
  );
}

export interface FlowBandData extends Record<string, unknown> {
  name: string;
  description: string;
  stepCount: number;
  color: string;
}

/**
 * A flow's lane: only the ground and the label, behind its steps. No handles
 * and no selection — it is the frame that makes "one system, branches
 * connected" readable instead of forty scattered nodes.
 */
export function FlowBandNode({ data }: NodeProps) {
  const { name, description, stepCount, color } = data as FlowBandData;
  // As for a step: from far away the label grows instead of staying drawn six
  // pixels tall.
  const far = useStore((s) => s.transform[2]) < FAR_ZOOM;
  return (
    <div className="flow-band" data-far={far || undefined} style={{ borderColor: color }}>
      <div className="flow-band__head">
        {/* IL COLORE DELLA CORSIA È UN SEGNO, NON UN COLORE DI TESTO.
            Scriverci sopra il nome del flusso è ciò che si faceva prima, e la
            tavolozza delle corsie non era mai stata misurata: su quattordici
            tinte solo tre reggono 4,5:1 sul fondo della banda, e cinque stanno
            sotto 3:1. `prima-corsa` veniva 2,77:1. Il nome adesso è inchiostro
            e si legge sempre; la tinta identifica la corsia da qui. */}
        <span className="flow-band__mark" style={{ background: color }} aria-hidden="true" />
        <span className="flow-band__name">{name}</span>
        <span className="flow-band__count">{stepCountLabel(stepCount)}</span>
      </div>
      {/* Troncata a due righe dallo stile, ma non persa: per intero si legge
          passandoci sopra. */}
      <div className="flow-band__desc" title={description}>
        {description}
      </div>
    </div>
  );
}
