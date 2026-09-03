// What this machine holds, one screen per row of the column's third ground.
// THE COLUMN IS THE ONLY NAVIGATION: this screen had a column of its own with
// the same seven names, so the window offered one list twice and the second
// copy was the one nobody could see from anywhere else.

import { AbilitiesScreen } from "./AbilitiesScreen";
import type { SailorTab } from "./sailortabs";
import { EnginesScreen } from "./EnginesScreen";
import { Installed } from "./Installed";
import { KeepsScreen } from "./KeepsScreen";
import { MachineScreen } from "./MachineScreen";
import { Manual } from "./Manual";
import { ProfileList } from "./ProfileList";
import { QuotaScreen } from "./QuotaScreen";


export function SailorScreen({
  native,
  now,
  tab,
  onTerminalOpened,
}: {
  native: boolean;
  now: number;
  tab: SailorTab;
  /** An engine's gesture opened a terminal: whoever holds the places shows it. */
  onTerminalOpened?: () => void;
}) {
  return (
    <div className="section">
      <div className="section__body">
        {tab === "keeps" && <KeepsScreen native={native} />}
        {tab === "cando" && <AbilitiesScreen native={native} />}
        {tab === "engines" && <EnginesScreen native={native} onTerminalOpened={onTerminalOpened} />}
        {tab === "profiles" && <ProfileList native={native} />}
        {tab === "models" && <QuotaScreen native={native} now={now} />}
        {tab === "equipment" && (
          <>
            <MachineScreen native={native} />
            <Installed native={native} />
          </>
        )}
        {tab === "commands" && <Manual native={native} />}
      </div>
    </div>
  );
}
