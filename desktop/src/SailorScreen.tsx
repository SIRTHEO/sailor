// What this machine holds, one screen per row of the column's third ground.
// THE COLUMN IS THE ONLY NAVIGATION: this screen had a column of its own with
// the same seven names, so the window offered one list twice and the second
// copy was the one nobody could see from anywhere else.

import { AbilitiesScreen } from "./AbilitiesScreen";
import type { SailorTab } from "./sailortabs";
import { EnginesScreen } from "./EnginesScreen";
import { Installed } from "./Installed";
import { KeepsScreen } from "./KeepsScreen";
import { LookScreen } from "./LookScreen";
import { MachineScreen } from "./MachineScreen";
import { Manual } from "./Manual";
import { ModelsScreen } from "./ModelsScreen";
import { ProfileList } from "./ProfileList";
import { RunningScreen } from "./RunningScreen";


export function SailorScreen({
  native,
  tab,
  onTerminalOpened,
  onQuota,
}: {
  native: boolean;
  tab: SailorTab;
  /** An engine's gesture opened a terminal: whoever holds the places shows it. */
  onTerminalOpened?: () => void;
  /** The quota is another page's question: this is the way to it. */
  onQuota?: () => void;
}) {
  return (
    // Exactly one place, so it says which: a guard asks whether THAT place drew.
    <div className="section" data-place="sailor">
      <div className="section__body">
        {tab === "running" && <RunningScreen native={native} />}
        {tab === "keeps" && <KeepsScreen native={native} />}
        {tab === "cando" && <AbilitiesScreen native={native} />}
        {tab === "engines" && <EnginesScreen native={native} onTerminalOpened={onTerminalOpened} />}
        {tab === "profiles" && <ProfileList native={native} />}
        {tab === "models" && <ModelsScreen native={native} onQuota={onQuota} />}
        {tab === "equipment" && (
          <>
            <MachineScreen native={native} />
            <Installed native={native} />
          </>
        )}
        {tab === "commands" && <Manual native={native} />}
        {tab === "look" && <LookScreen />}
      </div>
    </div>
  );
}
