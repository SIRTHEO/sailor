import { createContext, useContext } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";
import type { FlowTrigger, RunSnapshot } from "./engine";
import { KindIcon } from "./StepNode";

/**
 * The trigger node: where a flow starts, and the gesture that starts it.
 *
 * **IT IS A NODE AND NOT A BUTTON IN THE BAR.** The gesture sits where the
 * graph begins, and the mandate written into it is that flow's.
 *
 * **THIS NODE IS THE GESTURE, NOT THE STEP.** The step is in the flow file — no
 * deps, `"action": "trigger"`, taking its mandate in `inputs.<step>.text` — and
 * draws like any other. This one sends it the signal, is never saved, never
 * wired by hand and never in the `.flow.json`: the line between what the engine
 * knows and what the window adds has to stay visible.
 *
 * **THE STATE COMES FROM A CONTEXT, NOT FROM THE NODE'S `data`.** Putting the
 * run in `data` means rebuilding the node list on every fact from the shell,
 * and a canvas that rebuilt its nodes inside an effect never settled long
 * enough to measure itself: invisible nodes, a full minimap.
 */

/** How the query to the shell about what triggers a flow stands. */
export type TriggerState =
  | { state: "asking" }
  | { state: "ready"; trigger: FlowTrigger }
  | { state: "mute"; why: string };

export interface RunControls {
  /** True inside the shell: outside there is no engine, and the button says so. */
  native: boolean;
  triggerOf: (flowName: string) => TriggerState;
  /** The most recent run of that flow, if this window knows of one. */
  runOf: (flowName: string) => RunSnapshot | undefined;
  /**
   * The mandate being written. It lives outside the node because the node
   * remounts every time the canvas redraws, and a long hand-written text must
   * not vanish because somebody renamed a step elsewhere.
   */
  mandateOf: (flowName: string) => string;
  onMandate: (flowName: string, text: string) => void;
  onRun: (flowName: string) => void;
  starting: (flowName: string) => boolean;
  errorOf: (flowName: string) => string | undefined;
  /** Opens the run view on that flow. */
  onWatch: (flowName: string) => void;
}

const MUTE: RunControls = {
  native: false,
  triggerOf: () => ({ state: "mute", why: "outside the shell: no engine to trigger" }),
  runOf: () => undefined,
  mandateOf: () => "",
  onMandate: () => {},
  onRun: () => {},
  starting: () => false,
  errorOf: () => undefined,
  onWatch: () => {},
};

export const RunContext = createContext<RunControls>(MUTE);

export interface TriggerNodeData extends Record<string, unknown> {
  flowName: string;
  color: string;
}

/** The id of a flow's trigger node, kept distinct from the steps. */
export function triggerNodeId(flowName: string): string {
  return `trigger::${flowName}`;
}

const RUNNING_LABEL: Record<string, string> = {
  running: "running",
  complete: "complete",
  failed: "failed",
  waiting: "waiting",
  stopped: "stopped",
  // Not «failed»: the run respected a limit somebody set on it. Whoever reads
  // the two as the same thing stops looking at both.
  cap_reached: "stopped by the spend cap",
  incomplete: "incomplete",
};

export function TriggerNode({ data }: NodeProps) {
  const { flowName, color } = data as TriggerNodeData;
  const controls = useContext(RunContext);
  const trigger = controls.triggerOf(flowName);
  const run = controls.runOf(flowName);
  const busy = controls.starting(flowName) || run?.status === "running";
  const error = controls.errorOf(flowName);

  const mandate = trigger.state === "ready" ? trigger.trigger.mandate : null;
  const canWriteMandate = mandate?.kind === "field";

  return (
    <div className="trigger-node" style={{ borderColor: color }}>
      <div className="trigger-node__head">
        {/* The same glyph the toolbox and the step nodes use for this species:
            one shape, wherever a trigger appears. */}
        <KindIcon kind="trigger" className="trigger-node__icon" />
        <span className="trigger-node__mark" style={{ background: color }} />
        <span className="trigger-node__kind">trigger · by hand</span>
      </div>

      <div className="trigger-node__flow">{flowName}</div>

      {/* The schedule is not a detail to hide: if the flow also starts on its
          own, whoever presses must know they are not the only one starting it. */}
      {trigger.state === "ready" && trigger.trigger.scheduled && (
        <div className="trigger-node__note">this flow also has a schedule of its own</div>
      )}

      {trigger.state === "asking" && <div className="trigger-node__note">asking the engine…</div>}

      {trigger.state === "mute" && <div className="trigger-node__why">{trigger.why}</div>}

      {trigger.state === "ready" && canWriteMandate && (
        // `nodrag` and `nowheel`: without them, dragging to select the text
        // would move the node, and the wheel would zoom the canvas instead of
        // scrolling the text.
        <textarea
          className="trigger-node__mandate nodrag nowheel"
          placeholder={
            mandate?.kind === "field"
              ? `the mandate: it enters «${mandate.step}» as «${mandate.field}»`
              : "the mandate: what it has to do, this time"
          }
          aria-label={`mandate for the flow ${flowName}`}
          value={controls.mandateOf(flowName)}
          disabled={busy}
          onChange={(event) => controls.onMandate(flowName, event.target.value)}
        />
      )}

      {trigger.state === "ready" && mandate?.kind === "none" && (
        // Why no mandate can be written is said **before** the press. After, it
        // would be the discovery that the flow started on somebody else's text.
        <div className="trigger-node__why">{mandate.why}</div>
      )}

      <div className="trigger-node__foot">
        <button
          type="button"
          className="trigger-node__go nodrag"
          disabled={!controls.native || trigger.state !== "ready" || busy}
          onClick={() => controls.onRun(flowName)}
          title={
            controls.native
              ? `start «${flowName}»`
              : "outside the native shell there is no engine to run it"
          }
        >
          {busy ? "running…" : "▶ Run"}
        </button>

        {run && (
          <button
            type="button"
            className="trigger-node__watch nodrag"
            onClick={() => controls.onWatch(flowName)}
            data-status={run.status}
          >
            {RUNNING_LABEL[run.status] ?? run.status}
          </button>
        )}
      </div>

      {error && <div className="trigger-node__error">{error}</div>}

      <Handle type="source" position={Position.Right} />
    </div>
  );
}
