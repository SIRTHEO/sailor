// What is installed on this machine.
//
// **THE REQUIREMENT: «show the end user everything — mcp, skills, flows, rules,
// workspace, profiles».** Skills, agents, commands, rules and hooks have been on the
// disk for months and no window named them: they showed only on `127.0.0.1:47831`,
// which means remembering a port.
//
// THREE STATES OF REACHABILITY, NOT TWO. The census tells «active», «switched off,
// with the reason» and «not known, with the reason» apart, and the third is no
// fallback: a skill inside a switched-off plugin is demonstrably unreachable, a rule
// in a repo depends on who opens the session and from where, so «active» would lie.
//
// WHERE IT LOOKED, ALWAYS IN THE CLEAR. A list that does not say where it searched
// cannot be contradicted — and whoever misses a thing they know they have cannot
// tell whether it or the folder is missing.

import { useMemo, useState } from "react";
import { useAsk } from "./ask";
import { machineInventory, type Installed as Census, type InstalledEntry } from "./engine";

/** The census walks the disk: it is asked once, not on a beat. */
const ONCE = null;

const FAMILY_WORD: Record<InstalledEntry["kind"], string> = {
  skill: "skills",
  agent: "agents",
  command: "commands",
  rule: "rules",
  hook: "hooks",
};

const FAMILIES = ["skill", "agent", "command", "rule", "hook"] as const;

/** How many there are per family, the ones at zero included. */
export function countByFamily(entries: InstalledEntry[]): Record<InstalledEntry["kind"], number> {
  const counts = { skill: 0, agent: 0, command: 0, rule: 0, hook: 0 };
  for (const entry of entries) counts[entry.kind] += 1;
  return counts;
}

export function Installed({ native }: { native: boolean }) {
  const { asked } = useAsk<Census>(native, machineInventory, ONCE, "outside the shell: the engine takes the census");
  const [family, setFamily] = useState<InstalledEntry["kind"] | null>(null);

  const entries = asked.state === "answered" ? asked.value.entries : EMPTY;
  const counts = useMemo(() => countByFamily(entries), [entries]);
  const shown = useMemo(
    () => (family === null ? entries : entries.filter((entry) => entry.kind === family)),
    [entries, family],
  );

  if (asked.state === "mute") {
    return (
      <div className="now">
        <p className="now__mute">Cannot take stock of this machine: {asked.why}</p>
      </div>
    );
  }
  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Walking the disk…</p>
      </div>
    );
  }

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Installato</h2>
        <span className="now__count">{entries.length}</span>
        <span className="now__note">su {asked.value.roots.length} radici</span>
      </header>

      <div className="families">
        <button
          type="button"
          className="families__item"
          data-here={family === null || undefined}
          onClick={() => setFamily(null)}
        >
          tutto
          <span className="families__count">{entries.length}</span>
        </button>
        {FAMILIES.map((kind) => (
          <button
            type="button"
            key={kind}
            className="families__item"
            data-here={family === kind || undefined}
            onClick={() => setFamily((current) => (current === kind ? null : kind))}
          >
            {FAMILY_WORD[kind]}
            <span className="families__count">{counts[kind]}</span>
          </button>
        ))}
      </div>

      {asked.value.stale_plugin_copies > 0 && (
        // Not census entries — nobody loads them — but they are space, and
        // while nobody counts them nobody takes them away.
        <p className="now__note">
          {asked.value.stale_plugin_copies} plugin copies sit in the cache without being the installed one.
        </p>
      )}

      <table className="now__table">
        <thead>
          <tr>
            <th>name</th>
            <th>family</th>
            <th>from where</th>
            <th>reachable</th>
            <th>who invokes it</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((entry) => (
            <tr key={`${entry.kind}::${entry.origin}::${entry.name}`}>
              <td className="now__entity">
                {entry.name}
                {entry.description !== "" && <span className="now__why">{entry.description}</span>}
              </td>
              <td className="now__when">{FAMILY_WORD[entry.kind]}</td>
              <td className="now__when">{entry.origin}</td>
              {/* THE REASON SITS BESIDE THE STATE. «Switched off» without the
                  why cannot be fixed: that is the third state's whole value. */}
              <td className="now__state" data-reach={entry.reach.state}>
                {entry.reach.state === "active" ? "active" : entry.reach.state === "inactive" ? "switched off" : "not known"}
                {entry.reach.state !== "active" && <span className="now__why">{entry.reach.reason}</span>}
              </td>
              <td className="now__when">{entry.by_model ? "the model too" : "only you"}</td>
            </tr>
          ))}
        </tbody>
      </table>

      {/* Where it looked. At the foot because it answers a question that arises
          only when something is missing — but it has to be there. */}
      <details className="roots">
        <summary className="roots__head">Where it looked</summary>
        <ul className="roots__list">
          {asked.value.roots.map((root) => (
            <li key={root}>{root}</li>
          ))}
        </ul>
      </details>
    </div>
  );
}

/** A stable empty list: a fresh `[]` on every render would redo the counts. */
const EMPTY: InstalledEntry[] = [];
