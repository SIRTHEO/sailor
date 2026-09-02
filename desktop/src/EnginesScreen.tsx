/**
 * The engines: every command line Sailor can run a step on, whether it is
 * here, signed in, how full its window is, and the gestures that change that.
 * **EVERY CLAIM CARRIES ITS EVIDENCE**, and a gesture opens a terminal of
 * Sailor's: a sign-in runs there, an install line waits there for Enter.
 */
import { useCallback, useEffect, useState } from "react";
import { engines, type Engine, type Engines } from "./engines";
import { openTerminal, pressKeys } from "./terminal";
import { BORN_COLS, BORN_ROWS } from "./TerminalPane";

type Ask = { state: "asking" } | { state: "asked"; value: Engines } | { state: "mute"; why: string };

const PRESENCE_WORD: Record<Engine["presence"], string> = {
  present: "here",
  absent: "not on this machine",
  undetermined: "nobody could look",
};

const SIGNED_WORD: Record<Engine["signed_in"], string> = {
  yes: "signed in",
  no: "not signed in",
  "not known": "nobody could ask",
};

export function percent(fraction: number): string {
  return `${Math.round(fraction * 100)}% spent`;
}

interface EnginesScreenProps {
  native: boolean;
  /** A terminal was opened for a gesture: whoever holds the places shows it. */
  onTerminalOpened?: () => void;
}

export function EnginesScreen({ native, onTerminalOpened }: EnginesScreenProps) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [busy, setBusy] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  const read = useCallback(() => {
    engines().then(
      (value) => setAsk({ state: "asked", value }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, []);

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no machine to look at" });
      return;
    }
    read();
  }, [native, read]);

  const gesture = useCallback(
    (key: string, work: () => Promise<void>) => {
      setBusy(key);
      setFailed(null);
      work().then(
        () => {
          setBusy(null);
          onTerminalOpened?.();
        },
        (error) => {
          setBusy(null);
          setFailed(String(error));
        },
      );
    },
    [onTerminalOpened],
  );

  if (ask.state === "mute") {
    return <div className="now"><p className="now__mute">I cannot look at the engines: {ask.why}</p></div>;
  }
  if (ask.state === "asking") {
    return <div className="now"><p className="now__mute">Looking at each engine…</p></div>;
  }

  const root = ask.value.workspace_root;
  const signIn = (engine: Engine) => async () => {
    if (engine.sign_in === null) throw new Error(`${engine.label} declares no sign-in`);
    await openTerminal({
      workspaceRoot: root,
      program: engine.sign_in.program,
      args: engine.sign_in.args.length > 0 ? engine.sign_in.args : undefined,
      cols: BORN_COLS,
      rows: BORN_ROWS,
    });
  };
  const install = (engine: Engine) => async () => {
    if (engine.install === null) throw new Error(`${engine.label} declares no install line`);
    // TYPED, NOT RUN: the line waits for the person's Enter, so what changes
    // the machine is read by a person first.
    const born = await openTerminal({ workspaceRoot: root, cols: BORN_COLS, rows: BORN_ROWS });
    await pressKeys(born.id, new TextEncoder().encode(engine.install.line));
  };
  const openWith = (engine: Engine) => async () => {
    if (engine.executable === null) throw new Error(`${engine.label} is not here`);
    await openTerminal({ workspaceRoot: root, program: engine.executable, cols: BORN_COLS, rows: BORN_ROWS });
  };

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Engines</h2>
        <span className="now__count">{ask.value.engines.length}</span>
      </header>
      <p className="now__mute">
        Each line was asked of the machine and of the engine itself. A gesture opens a terminal of Sailor’s in {root}.
      </p>
      {failed !== null && <p className="now__mute" data-bad>That did not work: {failed}</p>}

      {ask.value.engines.map((engine) => (
        <section className="panel__block" key={engine.id} data-presence={engine.presence}>
          <div className="panel__title">{engine.label}</div>
          {engine.executable !== null && <div className="now__command">{engine.executable}</div>}
          <dl className="now__kv">
            <dt>on this machine</dt>
            <dd>
              {PRESENCE_WORD[engine.presence]}
              {engine.version !== null && <> · {engine.version}</>}
              <div className="now__why">{engine.reason}</div>
            </dd>
            <dt>signed in</dt>
            <dd data-access={engine.signed_in}>
              {SIGNED_WORD[engine.signed_in]}
              {engine.profile_in_force !== null && <> · as {engine.profile_in_force}</>}
              <div className="now__why">{engine.signed_in_said}</div>
            </dd>
            <dt>window</dt>
            <dd>
              {engine.quota.length === 0 ? (
                <span className="now__why">{engine.quota_why ?? "nothing read"}</span>
              ) : (
                engine.quota.map((window) => (
                  <div key={window.unit}>
                    {window.unit}: {percent(window.spent_fraction)}
                    {window.resets_at !== null && <span className="now__why"> · resets {window.resets_at}</span>}
                  </div>
                ))
              )}
            </dd>
          </dl>
          <div className="now__new">
            {engine.presence === "present" && engine.sign_in !== null && engine.signed_in !== "yes" && (
              <button
                type="button"
                className="rail__all"
                disabled={busy !== null}
                title={engine.sign_in.note}
                onClick={() => gesture(`sign-in/${engine.id}`, signIn(engine))}
              >
                {busy === `sign-in/${engine.id}` ? "opening…" : "sign in, in a terminal"}
              </button>
            )}
            {engine.presence === "present" && engine.executable !== null && (
              <button
                type="button"
                className="rail__all"
                disabled={busy !== null}
                onClick={() => gesture(`open/${engine.id}`, openWith(engine))}
              >
                {busy === `open/${engine.id}` ? "opening…" : "open a terminal with it"}
              </button>
            )}
            {engine.presence !== "present" && engine.install !== null && (
              <button
                type="button"
                className="rail__all"
                disabled={busy !== null}
                title={engine.install.note}
                onClick={() => gesture(`install/${engine.id}`, install(engine))}
              >
                {busy === `install/${engine.id}` ? "opening…" : `type «${engine.install.line}» in a terminal`}
              </button>
            )}
            {engine.presence !== "present" && engine.install === null && (
              <span className="now__why">nobody measured where it installs from</span>
            )}
          </div>
        </section>
      ))}
    </div>
  );
}
