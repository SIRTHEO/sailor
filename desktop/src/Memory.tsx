// What happened, and what it cost: the ledger as a place a person asks
// questions of, instead of a thing Sailor writes and never reads.

import { useState } from "react";
import { FaultsScreen } from "./FaultsScreen";
import { History } from "./History";
import { LedgerBrowser } from "./LedgerBrowser";
import { MEMORY_TABS, type MemoryTab } from "./memorytabs";
import { Now } from "./Now";
import { QuotaScreen } from "./QuotaScreen";

export function Memory({
  native,
  now,
  tab,
  onTab,
  onOpenRun,
  onTable,
  root,
}: {
  native: boolean;
  now: number;
  tab: MemoryTab;
  onTab: (tab: MemoryTab) => void;
  onOpenRun: (runId: string) => void;
  /** The table the browser has open, for whoever draws the breadcrumbs. */
  onTable: (table: string | null) => void;
  /** The tree the window stands in, or `null` outside every workspace. */
  root: string | null;
}) {
  return (
    <div className="section">
      <SubRail here={tab} onGo={onTab} tabs={MEMORY_TABS} />
      <div className="section__body">
        {tab === "runs" && (
          <>
            <Now native={native} onOpen={onOpenRun} />
            <History native={native} root={root} />
          </>
        )}
        {tab === "spend" && <QuotaScreen native={native} now={now} />}
        {tab === "faults" && <FaultsScreen native={native} />}
        {/* THE TABLES ARE A VIEW OF THE DATA, not a section beside the runs:
            the same question had two doors, and neither named the other. */}
        {tab === "ledger" && <LedgerBrowser native={native} onTable={onTable} />}
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
