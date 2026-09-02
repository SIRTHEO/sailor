// Cosa è installato su questa macchina.
//
// **È LA DOMANDA CHE HAI FATTO: «che mostri tutto all'utente finale, mcp,
// skill, flussi, regole, workspace, profili».** Competenze, agenti, comandi,
// regole e ganci esistono da mesi sul disco e nessuna finestra li nominava: si
// vedevano solo su `127.0.0.1:47831`, che vuol dire ricordarsi una porta.
//
// TRE STATI DI RAGGIUNGIBILITÀ, NON DUE. Il censimento distingue «attiva»,
// «spenta col motivo» e «non lo so col motivo», e la terza voce non è un
// ripiego: una competenza dentro un plugin spento è dimostrabilmente
// irraggiungibile, una regola in un repo dipende da chi apre la sessione e da
// dove, e dire «attiva» sarebbe una bugia comoda.
//
// DOVE HA GUARDATO, SEMPRE IN CHIARO. Un elenco che non dice dove ha cercato
// non si può smentire — e chi non trova una cosa che sa di avere non ha modo
// di capire se manca lei o la cartella.

import { useMemo, useState } from "react";
import { useAsk } from "./ask";
import { machineInventory, type Installed as Census, type InstalledEntry } from "./engine";

/** Il censimento cammina sul disco: si chiede una volta, non a battito. */
const ONCE = null;

const FAMILY_WORD: Record<InstalledEntry["kind"], string> = {
  skill: "skills",
  agent: "agents",
  command: "commands",
  rule: "rules",
  hook: "hooks",
};

const FAMILIES = ["skill", "agent", "command", "rule", "hook"] as const;

/** Quante ce ne sono per famiglia, comprese quelle a zero. */
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
        // Non sono voci del censimento — nessuno le carica — ma sono spazio, e
        // finché nessuno le conta nessuno le toglie.
        <p className="now__note">
          {asked.value.stale_plugin_copies} copie di plugin restano in cache senza essere quella installata.
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
              {/* IL MOTIVO STA ACCANTO ALLO STATO. «Spenta» senza il perché
                  non si può correggere: è tutto il valore della terza voce. */}
              <td className="now__state" data-reach={entry.reach.state}>
                {entry.reach.state === "active" ? "active" : entry.reach.state === "inactive" ? "switched off" : "not known"}
                {entry.reach.state !== "active" && <span className="now__why">{entry.reach.reason}</span>}
              </td>
              <td className="now__when">{entry.by_model ? "the model too" : "only you"}</td>
            </tr>
          ))}
        </tbody>
      </table>

      {/* Dove ha guardato. In fondo perché è la risposta a una domanda che
          nasce solo quando manca qualcosa — ma deve esserci. */}
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

/** Un elenco vuoto stabile: un `[]` nuovo a ogni render rifarebbe i conti. */
const EMPTY: InstalledEntry[] = [];
