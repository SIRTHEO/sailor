// ⌘K: search or run a command, from anywhere in the window.
//
// **ELEVEN SCREENS BEHIND DOORS MADE EVERY SWITCH SEVERAL CLICKS.** The
// palette lists what the window can open and do, and one line of typing
// reaches any of it. It computes nothing: every entry is handed in.

import { useEffect, useMemo, useRef, useState } from "react";

export interface Entry {
  /** «Go to», «Run», «Switch profile»: the verb, drawn as a heading. */
  group: string;
  label: string;
  /** What the entry is about, under the label: an origin, a place, a state. */
  hint?: string;
  run: () => void;
}

/** The entries whose words contain every word typed, in the order given. */
export function matching(entries: Entry[], typed: string): Entry[] {
  const words = typed.toLowerCase().split(/\s+/).filter((word) => word !== "");
  if (words.length === 0) return entries;
  return entries.filter((entry) => {
    const text = `${entry.group} ${entry.label} ${entry.hint ?? ""}`.toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

export function isPaletteKey(event: { key: string; metaKey: boolean; ctrlKey: boolean }): boolean {
  return (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k";
}

export function Palette({ entries, open, onClose }: { entries: Entry[]; open: boolean; onClose: () => void }) {
  const [typed, setTyped] = useState("");
  const [cursor, setCursor] = useState(0);
  const box = useRef<HTMLInputElement>(null);
  const found = useMemo(() => matching(entries, typed), [entries, typed]);

  useEffect(() => {
    if (open) {
      setTyped("");
      setCursor(0);
      box.current?.focus();
    }
  }, [open]);

  useEffect(() => {
    setCursor(0);
  }, [typed]);

  if (!open) return null;

  const pick = (entry: Entry | undefined) => {
    if (entry === undefined) return;
    onClose();
    entry.run();
  };

  return (
    <div className="palette__scrim" onMouseDown={onClose} role="presentation">
      <div
        className="palette"
        role="dialog"
        aria-label="search or run a command"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <input
          ref={box}
          className="palette__box"
          aria-label="search or run a command"
          placeholder="Search or run a command"
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") onClose();
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setCursor((at) => Math.min(found.length - 1, at + 1));
            }
            if (event.key === "ArrowUp") {
              event.preventDefault();
              setCursor((at) => Math.max(0, at - 1));
            }
            if (event.key === "Enter") pick(found[cursor]);
          }}
        />
        <ul className="palette__list" role="listbox">
          {found.length === 0 && <li className="palette__none">Nothing matches «{typed}»</li>}
          {found.map((entry, index) => {
            const heads = index === 0 || found[index - 1].group !== entry.group;
            return (
              <li key={`${entry.group}:${entry.label}:${entry.hint ?? ""}`}>
                {heads && <div className="palette__group">{entry.group}</div>}
                <button
                  type="button"
                  role="option"
                  aria-selected={index === cursor}
                  className="palette__entry"
                  data-cursor={index === cursor || undefined}
                  onMouseEnter={() => setCursor(index)}
                  onClick={() => pick(entry)}
                >
                  <span className="palette__label">{entry.label}</span>
                  {entry.hint !== undefined && <span className="palette__hint">{entry.hint}</span>}
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
