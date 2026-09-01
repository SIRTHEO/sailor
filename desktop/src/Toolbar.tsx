import type { ReactElement } from "react";
import { Panel } from "@xyflow/react";

import { KIND_LABEL } from "./StepNode";
import { DEFAULT_ACTION_FOR_KIND, type StepKind } from "./flow";

/**
 * **THE TOOLBOX SITS INSIDE THE CANVAS, NOT IN THE RAIL BESIDE IT**, so that
 * composing a flow does not cross the boundary between what you watch and what
 * you command on every step. It is a React Flow `Panel` because the canvas is
 * infinite pan/zoom and a bar drawn IN it would scroll away on the first drag —
 * `Panel` draws outside `.react-flow__viewport`, the element carrying the
 * `transform`, and to the eye the two are identical until you drag.
 *
 * **`bottom-left`, not `bottom-center`.** The bottom band already has two
 * tenants, the zoom controls (41px from the edge) and the minimap (215px); a
 * centred bar can only be twice the narrower side wide, which below a 1418px
 * window drops under what the tools occupy and covers the minimap. Anchored to
 * the side it starts after the controls and ends before the minimap, along the
 * corridor `styles.css` declares (`--controls-reserve`, `--minimap-reserve`).
 * `Toolbar.test.tsx` redoes that sum without measuring a pixel.
 */

/**
 * The families, grouped by **who does the work** — the only distinction that
 * changes what happens when the step runs: `engine`, `human` and `subflow` hand
 * the work outside, `check`, `gesture` and `deposit` Sailor runs itself. Seven
 * buttons in a row are a wall; seven in three groups read. The names are not
 * drawn but live in `aria-label`, because three caption rows would cost a third
 * of the bar's height to label groups of one and of three, which the hairline
 * and the marks already say and a screen reader hears anyway. The list itself
 * is read from `DEFAULT_ACTION_FOR_KIND`, never written here — a tool with no
 * action makes a node that will not save — and a test keeps the two glued BOTH
 * WAYS, since a hand-written list would break the link in silence.
 */
interface ToolGroup {
  /** What the group has in common, for whoever reads with a screen reader. */
  label: string;
  kinds: StepKind[];
}

export const TOOL_GROUPS: ToolGroup[] = [
  { label: "Da dove parte", kinds: ["trigger"] },
  { label: "Chi fa il lavoro al posto di Sailor", kinds: ["engine", "human", "subflow"] },
  { label: "Cosa fa Sailor da sé", kinds: ["check", "gesture", "deposit"] },
];

/** The families the bar offers, in the order they are seen. */
export const TOOLBAR_KINDS: StepKind[] = TOOL_GROUPS.flatMap((group) => group.kinds);

/**
 * A family's mark: **the shape draws the gesture**, it does not decorate. The
 * label stays underneath — rule 5 applied to shape. There is no colour here,
 * `currentColor` only: tint is reserved for machine state, and a tool sitting
 * in a box has no state. Square caps: a technical-drawing hand.
 */
function KindMark({ kind }: { kind: StepKind }) {
  const shape = MARK[kind];
  return (
    <svg
      className="toolbar__mark"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="square"
      strokeLinejoin="miter"
    >
      {shape}
    </svg>
  );
}

/**
 * Seven marks, each of two or three primitives. The outgoing arrow means "the
 * work leaves here", and what it reaches says to whom: another program (the
 * box), a person (head and shoulders), another flow (its stacked steps).
 * Whatever lacks the arrow works on the spot.
 */
const MARK: Record<StepKind, ReactElement> = {
  // The bar everything starts from, and the signal leaving it.
  trigger: (
    <>
      <path d="M2.7 2.5v11" />
      <path d="M5.5 4l8 4-8 4z" fill="currentColor" stroke="none" />
    </>
  ),
  // The work leaves and enters another program.
  engine: (
    <>
      <path d="M1.5 8h4.5" />
      <path d="M4 5.5L6.5 8 4 10.5" />
      <rect x="9" y="3.2" width="5.5" height="9.6" />
    </>
  ),
  // The work leaves and a person takes it.
  human: (
    <>
      <path d="M1.5 8h4.5" />
      <path d="M4 5.5L6.5 8 4 10.5" />
      <circle cx="11.6" cy="5.4" r="2.1" />
      <path d="M8.4 13.2c0-2 1.5-3.4 3.2-3.4s3.2 1.4 3.2 3.4" />
    </>
  ),
  // The work leaves and another flow takes it, with its own steps.
  subflow: (
    <>
      <path d="M1.5 8h4.5" />
      <path d="M4 5.5L6.5 8 4 10.5" />
      <path d="M9.2 4h5.3M9.2 8h5.3M9.2 12h5.3" />
    </>
  ),
  // The echo of a command line: a shell's prompt sign.
  check: (
    <>
      <path d="M2.2 3.6L6.4 8l-4.2 4.4" />
      <path d="M7.6 12.4h6.2" />
    </>
  ),
  // Two ends touching: the question put to a connected service.
  gesture: (
    <>
      <circle cx="3.9" cy="8" r="2.1" />
      <circle cx="12.1" cy="8" r="2.1" />
      <path d="M6 8h4" />
    </>
  ),
  // The store's drum: what stays written.
  deposit: (
    <>
      <path d="M2.6 4.2v7.6c0 1.1 2.4 2 5.4 2s5.4-.9 5.4-2V4.2" />
      <ellipse cx="8" cy="4.2" rx="5.4" ry="2" />
    </>
  ),
  // The two families with no action: nothing in the engine resolves to them, so
  // the bar does not offer them and these marks are never drawn. They are here
  // to keep the map total, so the compiler notices if an action ever arrives.
  wait: <path d="M8 2.5v5.5l3.5 2.5" />,
  branch: (
    <>
      <path d="M2.5 8h4" />
      <path d="M6.5 8L12 3.5M6.5 8L12 12.5" />
    </>
  ),
};

interface ToolbarProps {
  /** The flow that receives the step, or `null` if none has focus. */
  flowName: string | null;
  onAdd: (kind: StepKind) => void;
  onNewFlow: () => void;
}

/**
 * **WITH NO FOCUSED FLOW THE BAR CHANGES JOB, IT DOES NOT GREY OUT.** Seven
 * disabled buttons take the space of seven gestures while offering none, and a
 * `title` only appears after a second of hovering. Instead the bar shrinks to
 * one row that says what is missing and carries the gesture that fixes it.
 */
export function Toolbar({ flowName, onAdd, onNewFlow }: ToolbarProps) {
  if (flowName === null) {
    return (
      <Panel position="bottom-left" className="toolbar">
        <p className="toolbar__prompt">
          Scegli un flusso nella colonna per aggiungere passi.
          <button type="button" className="toolbar__new" onClick={onNewFlow}>
            + Nuovo flusso
          </button>
        </p>
      </Panel>
    );
  }

  return (
    <Panel position="bottom-left" className="toolbar">
      {/* DOVE VA A FINIRE IL PASSO, scritto prima di premere. La tela mostra
          tutti i flussi insieme: senza questa riga il passo nuovo comparirebbe
          in una corsia qualunque delle tante, e capire quale è un indovinello
          che si risolve dopo. */}
      <div className="toolbar__target">
        Aggiungi a <span className="toolbar__target-name">«{flowName}»</span>
      </div>
      <div className="toolbar__row">
        {TOOL_GROUPS.map((group) => (
          <div className="toolbar__group" key={group.label} role="group" aria-label={group.label}>
            {group.kinds.map((kind) => (
              <button
                key={kind}
                type="button"
                className="toolbar__tool"
                data-kind={kind}
                onClick={() => onAdd(kind)}
              >
                <KindMark kind={kind} />
                <span className="toolbar__label">{KIND_LABEL[kind]}</span>
              </button>
            ))}
          </div>
        ))}
      </div>
    </Panel>
  );
}

/**
 * The families the default-action map knows, for the test that compares the
 * groups against it. Exported from here and not copied into the test, because
 * this is the same read the bar does: a test that copied the list would only be
 * testing its own copy.
 */
export const KINDS_WITH_ACTION = Object.keys(DEFAULT_ACTION_FOR_KIND) as StepKind[];
