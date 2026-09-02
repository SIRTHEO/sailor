// What Sailor knows about itself and can do: the section that answers
// «what does it save, how does it manage it, what can it do, with what».

import { AbilitiesScreen } from "./AbilitiesScreen";
import { EnginesScreen } from "./EnginesScreen";
import { Installed } from "./Installed";
import { KeepsScreen } from "./KeepsScreen";
import { MachineScreen } from "./MachineScreen";
import { Manual } from "./Manual";
import { SubRail } from "./Memory";
import { ProfileList } from "./ProfileList";
import { QuotaScreen } from "./QuotaScreen";

export type SailorTab = "keeps" | "cando" | "engines" | "profiles" | "models" | "equipment" | "commands";

export const SAILOR_TABS: { id: SailorTab; name: string; about: string; group: string }[] = [
  { id: "keeps", name: "What it keeps", about: "every store, its path and its size", group: "itself" },
  { id: "cando", name: "What it can do", about: "the actions a flow may use", group: "itself" },
  { id: "engines", name: "Engines", about: "which command lines are here, signed in, and how full", group: "setup" },
  { id: "profiles", name: "Profiles", about: "which account each command line runs under", group: "setup" },
  { id: "models", name: "Models", about: "the catalogue, and which is in use", group: "setup" },
  { id: "equipment", name: "Equipment", about: "tools, skills and rules on this machine", group: "setup" },
  { id: "commands", name: "Commands", about: "every verb sailor answers to", group: "setup" },
];

export const SAILOR_GROUPS = ["itself", "setup"];

export function SailorScreen({
  native,
  now,
  tab,
  onTab,
  onTerminalOpened,
}: {
  native: boolean;
  now: number;
  tab: SailorTab;
  onTab: (tab: SailorTab) => void;
  /** An engine's gesture opened a terminal: whoever holds the places shows it. */
  onTerminalOpened?: () => void;
}) {
  return (
    <div className="section">
      <SubRail here={tab} onGo={onTab} tabs={SAILOR_TABS} groups={SAILOR_GROUPS} />
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
