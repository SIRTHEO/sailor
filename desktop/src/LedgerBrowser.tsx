// The ledger as a database: the tables with their counts, a statement, the
// rows, and the row picked. **THE SELECTED ROW IS THE POINT**: a wide table
// hides half its columns, and one row laid out as key and value hides none.

import { useCallback, useEffect, useState } from "react";
import { cellText, ledgerQuery, ledgerTables, openingStatement, type Answer, type Tables } from "./ledger";

type Listed = { state: "asking" } | { state: "listed"; tables: Tables } | { state: "mute"; why: string };
type Asked = { state: "idle" } | { state: "asking" } | { state: "answered"; answer: Answer } | { state: "refused"; why: string };

export function LedgerBrowser({ native }: { native: boolean }) {
  const [listed, setListed] = useState<Listed>({ state: "asking" });
  const [table, setTable] = useState<string | null>(null);
  const [sql, setSql] = useState("");
  const [asked, setAsked] = useState<Asked>({ state: "idle" });
  const [picked, setPicked] = useState<number | null>(null);

  useEffect(() => {
    if (!native) {
      setListed({ state: "mute", why: "outside the native shell: no ledger to list" });
      return;
    }
    ledgerTables().then(
      (tables) => setListed({ state: "listed", tables }),
      (error: unknown) => setListed({ state: "mute", why: String(error) }),
    );
  }, [native]);

  const run = useCallback((statement: string) => {
    setAsked({ state: "asking" });
    setPicked(null);
    ledgerQuery(statement).then(
      (answer) => setAsked({ state: "answered", answer }),
      (error: unknown) => setAsked({ state: "refused", why: String(error) }),
    );
  }, []);

  const open = useCallback(
    (name: string) => {
      setTable(name);
      const statement = openingStatement(name);
      setSql(statement);
      run(statement);
    },
    [run],
  );

  return (
    <div className="browser">
      <aside className="browser__tables">
        <div className="places__heading">tables</div>
        {listed.state === "asking" && <p className="browser__note">Listing…</p>}
        {listed.state === "mute" && <p className="browser__note">{listed.why}</p>}
        {listed.state === "listed" && !listed.tables.exists && (
          <p className="browser__note">
            No ledger at {listed.tables.directory}: nothing has been written yet. Saying so is the job.
          </p>
        )}
        {listed.state === "listed" &&
          listed.tables.tables.map((one) => (
            <button
              type="button"
              key={one.name}
              className="browser__table"
              data-here={table === one.name || undefined}
              onClick={() => open(one.name)}
            >
              <span className="browser__table-name">{one.name}</span>
              <span className="browser__table-rows">{one.rows}</span>
            </button>
          ))}
        {listed.state === "listed" && listed.tables.exists && (
          <>
            <div className="places__heading browser__file-heading">the file</div>
            <div className="browser__file">{listed.tables.directory}</div>
          </>
        )}
      </aside>

      <div className="browser__stage">
        <form
          className="browser__ask"
          onSubmit={(event) => {
            event.preventDefault();
            if (sql.trim() !== "") run(sql);
          }}
        >
          <input
            className="browser__sql"
            aria-label="a statement for the ledger"
            value={sql}
            onChange={(event) => setSql(event.target.value)}
            placeholder="select * from runs order by started_at desc limit 50"
            spellCheck={false}
          />
          <button type="submit" className="browser__run">
            Run
          </button>
        </form>

        {asked.state === "idle" && <p className="browser__note">Pick a table, or type a statement.</p>}
        {asked.state === "asking" && <p className="browser__note">Asking…</p>}
        {asked.state === "refused" && (
          <p className="browser__note" data-gravity="danger">
            {asked.why}
          </p>
        )}
        {asked.state === "answered" && asked.answer.rows.length === 0 && (
          <p className="browser__note">
            No rows. Columns: {asked.answer.columns.length === 0 ? "none" : asked.answer.columns.join(", ")}
          </p>
        )}
        {asked.state === "answered" && asked.answer.rows.length > 0 && (
          <div className="browser__grid">
            <table className="browser__rows">
              <thead>
                <tr>
                  {asked.answer.columns.map((column) => (
                    <th key={column}>{column}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {asked.answer.rows.map((row, index) => (
                  <tr
                    key={index}
                    data-picked={picked === index || undefined}
                    onClick={() => setPicked(index)}
                  >
                    {row.map((cell, column) => (
                      <td key={column} data-null={cell === null || undefined}>
                        {cellText(cell)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
            {asked.answer.truncated && (
              <p className="browser__note">Cut at the limit: narrow the statement to see the rest.</p>
            )}
          </div>
        )}

        {asked.state === "answered" && picked !== null && asked.answer.rows[picked] !== undefined && (
          <section className="browser__picked">
            <div className="places__heading">selected row</div>
            <dl className="browser__pairs">
              {asked.answer.columns.map((column, index) => (
                <div className="browser__pair" key={column}>
                  <dt>{column}</dt>
                  <dd data-null={asked.answer.rows[picked][index] === null || undefined}>
                    {cellText(asked.answer.rows[picked][index])}
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        )}
      </div>
    </div>
  );
}
