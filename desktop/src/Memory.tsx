// What happened, and what it cost: the ledger as a place a person asks
// questions of, instead of a thing Sailor writes and never reads.

import { useState } from "react";
import { FaultsScreen } from "./FaultsScreen";
import { History } from "./History";
import { LedgerBrowser } from "./LedgerBrowser";
import { Now } from "./Now";
import { QuotaScreen } from "./QuotaScreen";

export type MemoryTab = "runs" | "ledger" | "spend" | "faults";

export const MEMORY_TABS: { id: MemoryTab; name: string; about: string }[] = [
  { id: "runs", name: "Runs", about: "open now, and every one before" },
  { id: "ledger", name: "Ledger", about: "the tables, as they are" },
  { id: "spend", name: "Spend and quota", about: "what it cost, what is left" },
  { id: "faults", name: "Faults", about: "and what would have prevented each" },
];

export function Memory({
  native,
  now,
  tab,
  onTab,
  onOpenRun,
  onLedgerTable,
}: {
  native: boolean;
  now: number;
  tab: MemoryTab;
  onTab: (tab: MemoryTab) => void;
  onOpenRun: (runId: string) => void;
  /** The table open in the ledger browser, for the breadcrumbs; `null` when none. */
  onLedgerTable?: (table: string | null) => void;
}) {
  return (
    <div className="section">
      <SubRail here={tab} onGo={onTab} tabs={MEMORY_TABS} />
      <div className="section__body">
        {tab === "runs" && (
          <>
            <Now native={native} onOpen={onOpenRun} />
            <History native={native} />
          </>
        )}
        {tab === "ledger" && <LedgerBrowser native={native} onTable={onLedgerTable} />}
        {tab === "spend" && <QuotaScreen native={native} now={now} />}
        {tab === "faults" && <FaultsScreen native={native} />}
      </div>
    </div>
  );
}

/** The column inside a section: one entry per question, with what it answers. */
export function SubRail<T extends string>({
  here,
  onGo,
  tabs,
  groups,
}: {
  here: T;
  onGo: (tab: T) => void;
  tabs: { id: T; name: string; about: string; group?: string }[];
  groups?: string[];
}) {
  const grouped = groups ?? [""];
  return (
    <aside className="subrail">
      {grouped.map((group) => (
        <div className="subrail__group" key={group}>
          {group !== "" && <div className="places__heading">{group}</div>}
          {tabs
            .filter((one) => (one.group ?? "") === group)
            .map((one) => (
              <button
                type="button"
                key={one.id}
                className="subrail__item"
                data-here={here === one.id || undefined}
                onClick={() => onGo(one.id)}
              >
                <span className="subrail__name">{one.name}</span>
                <span className="subrail__about">{one.about}</span>
              </button>
            ))}
        </div>
      ))}
    </aside>
  );
}

export function useTab<T extends string>(first: T): [T, (tab: T) => void] {
  return useState<T>(first);
}
