/**
 * Everything Sailor knows about itself, behind one door. **ELEVEN PLACES IN A
 * ROW IS A LIST OF WHAT EXISTS, NOT A NAVIGATION**: the bar grew one entry per
 * feature. Four are where work happens; the rest answer one question, grouped
 * by what they are about.
 */
import { useState } from "react";
import { AbilitiesScreen } from "./AbilitiesScreen";
import { FaultsScreen } from "./FaultsScreen";
import { Installed } from "./Installed";
import { LedgerScreen } from "./LedgerScreen";
import { MachineScreen } from "./MachineScreen";
import { Manual } from "./Manual";
import { ProfileList } from "./ProfileList";
import { Projects } from "./Projects";
import { QuotaScreen } from "./QuotaScreen";
import { Worktrees } from "./Worktrees";

export type Corner =
  | "projects"
  | "worktrees"
  | "profiles"
  | "quota"
  | "machine"
  | "installed"
  | "commands"
  | "ledger"
  | "abilities"
  | "faults";

/**
 * The groups, and what each one is for. The heading is the question, not the
 * category: «what am I working on» is answerable, «workspaces» is a word.
 */
const GROUPS: { heading: string; corners: { id: Corner; name: string; about: string }[] }[] = [
  {
    heading: "what you are working on",
    corners: [
      { id: "projects", name: "Projects", about: "the ones Sailor has been opened in" },
      { id: "worktrees", name: "Worktrees", about: "copies of a repository, side by side" },
    ],
  },
  {
    heading: "what you are working as",
    corners: [
      { id: "profiles", name: "Profiles", about: "which account each command line runs under" },
      { id: "quota", name: "Quota and models", about: "what is spent, what runs, what it costs" },
    ],
  },
  {
    heading: "what this machine has",
    corners: [
      { id: "machine", name: "Tools", about: "what is installed, where, in which version" },
      { id: "installed", name: "Skills and rules", about: "what a session is given to work with" },
      { id: "commands", name: "Commands", about: "every verb Sailor answers to" },
      { id: "abilities", name: "What Sailor can do", about: "the actions a flow may use" },
    ],
  },
  {
    heading: "what Sailor has written down",
    corners: [
      { id: "ledger", name: "The ledger", about: "runs, processes, and what flows kept" },
      { id: "faults", name: "Faults", about: "and what would have prevented each" },
    ],
  },
];

export function Everything({ native, now }: { native: boolean; now: number }) {
  const [corner, setCorner] = useState<Corner>("projects");

  return (
    <div className="everything">
      {/* THE COLUMN SAYS WHAT EACH ONE ANSWERS, not only its name. A row of
          eight nouns makes you open them to find out; the line underneath is
          what turns the list into a choice. */}
      <aside className="everything__rail">
        {GROUPS.map((group) => (
          <div className="everything__group" key={group.heading}>
            <div className="everything__heading">{group.heading}</div>
            {group.corners.map((one) => (
              <button
                type="button"
                key={one.id}
                className="everything__item"
                data-here={corner === one.id || undefined}
                onClick={() => setCorner(one.id)}
              >
                <span className="everything__name">{one.name}</span>
                <span className="everything__about">{one.about}</span>
              </button>
            ))}
          </div>
        ))}
      </aside>

      <div className="everything__body">
        {corner === "projects" && <Projects native={native} now={now} />}
        {corner === "worktrees" && <Worktrees native={native} />}
        {corner === "profiles" && <ProfileList native={native} />}
        {corner === "quota" && <QuotaScreen native={native} now={now} />}
        {corner === "machine" && <MachineScreen native={native} />}
        {corner === "installed" && <Installed native={native} />}
        {corner === "commands" && <Manual native={native} />}
        {corner === "abilities" && <AbilitiesScreen native={native} />}
        {corner === "ledger" && <LedgerScreen native={native} now={now} />}
        {corner === "faults" && <FaultsScreen native={native} />}
      </div>
    </div>
  );
}
