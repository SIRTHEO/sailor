// What is running: the live terminals, and the trees they run in.
//
// The terminals stay mounted whichever entry is open, like behind the other
// places: unmounting them would destroy every emulator while the processes
// inside are alive and talking.

import { SubRail } from "./Memory";
import { Projects } from "./Projects";
import type { TerminalSummary } from "./terminal";
import { Terminals } from "./Terminals";
import type { Bench } from "./Workbench";
import { Worktrees } from "./Worktrees";

export type TerminalsTab = "live" | "projects" | "worktrees";

export const TERMINALS_TABS: { id: TerminalsTab; name: string; about: string }[] = [
  { id: "live", name: "Live", about: "the terminals open now" },
  { id: "projects", name: "Projects", about: "the ones sailor has been opened in" },
  { id: "worktrees", name: "Worktrees", about: "copies of a repository, side by side" },
];

export function TerminalsSection({
  native,
  now,
  shown,
  tab,
  onTab,
  ceiling,
  onProjectChanged,
  onCount,
  onList,
  bench,
  onBenchClosed,
}: {
  native: boolean;
  now: number;
  shown: boolean;
  tab: TerminalsTab;
  onTab: (tab: TerminalsTab) => void;
  /** The ceiling the relay hands on at, or `null` when no loaded flow declares one. */
  ceiling: number | null;
  /** The window moved into another project: whoever holds the flows reads them again. */
  onProjectChanged: () => void;
  /** How many terminals are open, for the column's count. */
  onCount: (count: number) => void;
  /** The terminals themselves, for the column that nests them under a tree. */
  onList: (all: TerminalSummary[]) => void;
  /** The terminal opened to work on a handed step, when one is. */
  bench?: Bench | null;
  onBenchClosed?: (answer: string) => void;
}) {
  return (
    <div className="section" hidden={!shown}>
      <SubRail here={tab} onGo={onTab} tabs={TERMINALS_TABS} />
      <div className="section__body section__body--terminals">
        <Terminals
          native={native}
          shown={shown && tab === "live"}
          ceiling={ceiling}
          onCount={onCount}
          onList={onList}
          bench={bench}
          onBenchClosed={onBenchClosed}
        />
        {shown && tab === "projects" && <Projects native={native} now={now} onMoved={onProjectChanged} />}
        {shown && tab === "worktrees" && <Worktrees native={native} />}
      </div>
    </div>
  );
}
