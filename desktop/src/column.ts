/**
 * ONE READING ANSWERS THE WHOLE COLUMN. Four calls answered its four
 * questions at four different moments, so the column could show a tree as
 * current while its flows still belonged to the last one. `theColumn` is the
 * seam: fed from a stub until the backend reading lands.
 */
import { invoker } from "./engine";
import type { TerminalSummary } from "./terminal";
import type { Project } from "./workspaces";

/** A flow as the column draws it: a name, what it is, and what it is doing. */
export interface ColumnFlow {
  name: string;
  note: string;
  /** The lane's tint on the board, so the column and the paper agree. */
  color?: string;
  dirty: boolean;
  /** `null` when nothing is happening to it, absent when nobody asked. */
  live?: ColumnFlowLive | null;
}

/** What is happening to a flow right now, in the words the column prints. */
export interface ColumnFlowLive {
  state: "running" | "stopping" | "handed_to_human" | "went" | "broke";
  done: number;
  steps: number;
  waiting: number;
  speaking: boolean;
}

/** A flow that will not open, kept in the list with the reason it will not. */
export interface ColumnBrokenFlow {
  name: string;
  reason: string;
}

/** One checkout of a workspace, with everything Sailor has open in it. */
export interface ColumnTree {
  root: string;
  /** The last segment of the path: what a person calls this tree. */
  name: string;
  standing: Project["standing"];
  current: boolean;
  lastSeen: number;
  flows: ColumnFlow[];
  brokenFlows: ColumnBrokenFlow[];
  terminals: TerminalSummary[];
  /** What hangs off the tree's board, when the count is known. */
  boardCount: number | null;
}

/** THE NAME GROUPS AND THE PATH IDENTIFIES: several trees, one declared name. */
export interface ColumnWorkspace {
  name: string;
  trees: ColumnTree[];
}

/** A source of flows that is the same wherever you stand. */
export interface ColumnFlowSource {
  /** «yours», «built in»: where the engine says these came from. */
  origin: string;
  flows: ColumnFlow[];
  brokenFlows: ColumnBrokenFlow[];
}

/** OUTSIDE IS A PLACE. What is open where no workspace claims it. */
export interface ColumnOutside {
  terminals: TerminalSummary[];
}

/** Everything the column draws, read at one instant. */
export interface ColumnReading {
  workspaces: ColumnWorkspace[];
  flowsEverywhere: ColumnFlowSource[];
  outside: ColumnOutside;
  /** The tree the window stands in, or `null` when it stands in none. */
  standingIn: string | null;
  /** When the engine took this reading, in seconds. */
  readAt: number;
}

/**
 * NOTHING READ YET IS A SHAPE, NOT AN ABSENCE. A column fed `undefined`
 * before the first answer draws nothing and says nothing about why.
 */
export const EMPTY_COLUMN: ColumnReading = {
  workspaces: [],
  flowsEverywhere: [],
  outside: { terminals: [] },
  standingIn: null,
  readAt: 0,
};

function terminal(id: string, root: string, name: string, device: string): TerminalSummary {
  return {
    id,
    workspaceRoot: root,
    workspaceName: name,
    alive: true,
    processId: 0,
    device,
    moved: 0,
    estimatedTokens: 0,
    program: "zsh",
    profile: null,
  };
}

/**
 * What the column draws until the engine answers. A STUB SAYS IT IS ONE:
 * every name here is invented, and the column declares its source so nobody
 * reads «a-project» as something on their disk.
 */
export function columnStub(): ColumnReading {
  const flow = (name: string, note: string): ColumnFlow => ({ name, note, dirty: false, live: null });
  return {
    workspaces: [
      {
        name: "a-project",
        trees: [
          {
            root: "/a-project/a-tree",
            name: "a-tree",
            standing: "declared",
            current: true,
            lastSeen: 0,
            flows: [flow("a-flow", "7 steps")],
            brokenFlows: [],
            terminals: [terminal("t1", "/a-project/a-tree", "a-project", "ttys001")],
            boardCount: 31,
          },
          {
            root: "/a-project/another-tree",
            name: "another-tree",
            standing: "declared",
            current: false,
            lastSeen: 0,
            flows: [],
            brokenFlows: [],
            terminals: [],
            boardCount: null,
          },
        ],
      },
    ],
    flowsEverywhere: [
      { origin: "yours", flows: [flow("a-flow-of-yours", "4 steps")], brokenFlows: [] },
      { origin: "built in", flows: [flow("a-shipped-flow", "9 steps")], brokenFlows: [] },
    ],
    outside: { terminals: [terminal("t2", "/somewhere", "", "ttys002")] },
    standingIn: "/a-project/a-tree",
    readAt: 0,
  };
}

/**
 * The one call the column is fed from: `left_column`, which answers all four
 * of its questions at once. Outside the desktop shell there is no engine to
 * ask, so the stub stands in — a drawn column beats an empty one in a browser.
 */
export function theColumn(): Promise<ColumnReading> {
  const invoke = invoker();
  if (!invoke) return Promise.resolve(columnStub());
  return invoke<ColumnReading>("left_column");
}
