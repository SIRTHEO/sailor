// The Sailor commands, declared by the binary and not copied out here.
//
// **THIS FILE CONTAINS THE NAME OF NO COMMAND.** That is its one rule, and it matters
// more than how it is laid out. Sailor has ten commands and some thirty forms: writing
// them in TypeScript would have been half an hour of work and a page that diverges from
// the binary at the first option added. It is fault 10 — the same truth in more than one
// place, with nothing comparing them — which in this repo has already come back five
// times, the last on the vocabulary of the actions, where the window offered six names
// the engine did not know and refused five it could execute.
//
// So `crates/sailor` became lib+bin, `manual` translates only the shape, and here it
// is laid out. If a command is born tomorrow, this page shows it without anybody
// opening the page; if one disappears, it disappears from here too.
//
// OUTSIDE THE SHELL NO LIST IS INVENTED. In the browser the engine does not answer,
// and the page says so instead of showing a plausible example: a fake manual is worse
// than an absent one, because it reads the same.

import { useMemo, useState } from "react";
import { useAsk } from "./ask";
import { manual, type CommandDoc } from "./engine";

/** The manual is compiled into the binary: it is asked once. */
const ONCE = null;

const EMPTY: CommandDoc[] = [];

/**
 * The words of a usage form, split into what you type and what you replace. It
 * gives the two different weight without anybody rewriting the lines by hand:
 * `<nome>` and `[opzioni]` are holes to fill, all the rest is literal text.
 */
export function pieces(line: string): { text: string; hole: boolean }[] {
  return line
    .split(/(\s+)/)
    .filter((piece) => piece.trim() !== "")
    .map((word) => ({
      text: word,
      hole: word.startsWith("<") || word.startsWith("[") || word.includes("|"),
    }));
}

/** How many forms in all: the number that says whether the manual arrived whole. */
export function shapeCount(commands: CommandDoc[]): number {
  return commands.reduce((total, command) => total + command.usage.length, 0);
}

export function Manual({ native }: { native: boolean }) {
  const { asked } = useAsk<CommandDoc[]>(
    native,
    manual,
    ONCE,
    "outside the shell: the binary declares the commands",
  );
  const [open, setOpen] = useState<string | null>(null);

  const commands = asked.state === "answered" ? asked.value : EMPTY;
  const shapes = useMemo(() => shapeCount(commands), [commands]);

  if (asked.state === "mute") {
    return (
      <div className="now">
        <p className="now__mute">Cannot list the commands: {asked.why}</p>
      </div>
    );
  }
  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Asking the binary which commands it has…</p>
      </div>
    );
  }

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Commands</h2>
        <span className="now__count">{commands.length}</span>
        <span className="now__note">
          {shapes} forms, declared by the binary that is running
        </span>
      </header>

      <ul className="manual">
        {commands.map((command) => {
          const here = open === command.name;
          return (
            <li className="manual__row" key={command.name}>
              <button
                type="button"
                className="manual__head"
                aria-expanded={here}
                onClick={() => setOpen(here ? null : command.name)}
              >
                <code className="manual__name">sailor {command.name}</code>
                <span className="manual__what">{command.description}</span>
                <span className="manual__shapes">
                  {command.usage.length}
                  <span className="manual__shapes-word">
                    {command.usage.length === 1 ? " forma" : " forme"}
                  </span>
                </span>
              </button>
              {here && (
                <ul className="manual__shapes-list">
                  {command.usage.map((shape) => (
                    <li className="manual__shape" key={shape.form}>
                      <code>
                        {pieces(shape.form).map((piece, index) => (
                          <span
                            key={`${piece.text}-${index}`}
                            className={piece.hole ? "manual__hole" : "manual__word"}
                          >
                            {piece.text}{" "}
                          </span>
                        ))}
                      </code>
                      {shape.says !== "" && (
                        <span className="manual__says">{shape.says}</span>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
