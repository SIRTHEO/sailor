// What an agent changed, read inside Sailor.
//
// **A DIFF, NOT AN EDITOR.** This shows git's answer about a working tree and
// hands a file to whoever opens files here, naming them before the press.
// Nothing here computes a difference: the text is `git diff`'s.

import { useCallback, useEffect, useState } from "react";
import {
  openInEditor,
  openerNote,
  openerWord,
  statusWord,
  whoOpensFiles,
  workspaceChanges,
  type Changes as Seen,
  type Opener,
} from "./changes";

type Ask = { state: "asking" } | { state: "asked"; seen: Seen } | { state: "mute"; why: string };

export function ChangesScreen({ root, name }: { root: string; name: string }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [trouble, setTrouble] = useState<string | null>(null);
  const [opener, setOpener] = useState<Opener | null>(null);

  const again = useCallback(() => {
    setAsk({ state: "asking" });
    workspaceChanges(root).then(
      (seen) => setAsk({ state: "asked", seen }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, [root]);

  useEffect(again, [again]);

  // Asked before a file is pressed: the word on the button is what will
  // happen, and an unanswered ask promises nothing.
  useEffect(() => {
    let listening = true;
    whoOpensFiles().then(
      (answer) => listening && setOpener(answer),
      () => listening && setOpener(null),
    );
    return () => {
      listening = false;
    };
  }, []);

  const open = useCallback((path: string) => {
    setTrouble(null);
    openInEditor(path).catch((error: unknown) => setTrouble(String(error)));
  }, []);

  return (
    <section className="changes">
      <header className="changes__head">
        <span className="label">What changed in {name}</span>
        <span className="changes__root">{root}</span>
        <button type="button" className="changes__again" onClick={again}>
          read again
        </button>
      </header>

      {openerNote(opener) !== null && <p className="changes__note">{openerNote(opener)}</p>}
      {ask.state === "asking" && <p className="changes__note">Asking git…</p>}
      {ask.state === "mute" && <p className="changes__note">I cannot read the working tree: {ask.why}</p>}
      {trouble !== null && (
        <p className="terminals__trouble" data-gravity="danger">
          {trouble}
        </p>
      )}

      {ask.state === "asked" && ask.seen.files.length === 0 && (
        <p className="changes__note">Nothing changed since the last commit.</p>
      )}

      {ask.state === "asked" && ask.seen.files.length > 0 && (
        <>
          <ul className="changes__files">
            {ask.seen.files.map((file) => (
              <li className="changes__file" key={file.path}>
                <span className="changes__status">{statusWord(file.status)}</span>
                <span className="changes__path">{file.path}</span>
                <button
                  type="button"
                  className="changes__open"
                  onClick={() => open(`${ask.seen.root}/${file.path}`)}
                >
                  {openerWord(opener)}
                </button>
              </li>
            ))}
          </ul>
          {/* GIT'S TEXT, VERBATIM: a person can compare it line for line with
              the terminal, which is the only way to trust a diff. */}
          <pre className="changes__diff">{ask.seen.diff}</pre>
        </>
      )}
    </section>
  );
}
