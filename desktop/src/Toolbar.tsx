import { Panel } from "@xyflow/react";

/* THE SAME GLYPH AS ON THE CANVAS, AND NOT A SECOND DRAWING OF IT. The bar
   carried its own set of nine marks in its own visual language: a species read
   as two pictures, so a mark learned in the toolbox was not on the board. */
import { KIND_LABEL, KindIcon } from "./StepNode";
import { DEFAULT_ACTION_FOR_KIND, type StepKind } from "./flow";

/**
 * **THE TOOLBOX SITS INSIDE THE CANVAS, NOT IN THE RAIL BESIDE IT.** It is a
 * React Flow `Panel`, which draws outside the element carrying the pan/zoom
 * `transform`: a bar drawn in the viewport scrolls away on the first drag.
 * `bottom-left` and not `bottom-center`, because the bottom band already holds
 * the zoom controls and the minimap; anchored to the side the bar runs the
 * corridor `styles.css` declares, and `Toolbar.test.tsx` redoes that sum.
 */

/**
 * The families, grouped by **who does the work** — the only distinction that
 * changes what happens when the step runs: `engine`, `human` and `subflow` hand
 * it outside, `check`, `gesture` and `deposit` Sailor runs itself. The names
 * live in `aria-label`, not drawn: caption rows would cost a third of the bar
 * to say what the marks say. The list is read from `DEFAULT_ACTION_FOR_KIND`,
 * and a test keeps the two glued both ways.
 */
interface ToolGroup {
  /** What the group has in common, for whoever reads with a screen reader. */
  label: string;
  kinds: StepKind[];
}

export const TOOL_GROUPS: ToolGroup[] = [
  { label: "Where it starts", kinds: ["trigger"] },
  { label: "Who does the work instead of Sailor", kinds: ["engine", "human", "subflow"] },
  { label: "What Sailor does itself", kinds: ["check", "gesture", "deposit"] },
];

/** The families the bar offers, in the order they are seen. */
export const TOOLBAR_KINDS: StepKind[] = TOOL_GROUPS.flatMap((group) => group.kinds);



interface ToolbarProps {
  /** The flow that receives the step: the one the board draws. */
  flowName: string;
  onAdd: (kind: StepKind) => void;
}

/**
 * **THE BAR HAS ONE JOB BECAUSE THE BOARD HAS ONE FLOW.** A second face saying
 * «pick a flow in the column», and a line naming which lane the step fell into,
 * both answered a question the board no longer asks.
 */
export function Toolbar({ flowName, onAdd }: ToolbarProps) {
  return (
    /* WHERE THE STEP LANDS IS STILL SAID, WHERE IT COSTS NOTHING: the drawn row
       repeated the name at the top of the paper, and said it to no reader. */
    <Panel position="bottom-left" className="toolbar" aria-label={`Add a step to ${flowName}`}>
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
                <KindIcon kind={kind} className="toolbar__mark" />
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
