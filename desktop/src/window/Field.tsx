import type { ReactNode } from "react";

/**
 * **THE FIELD HOLDS OPEN THINGS**, whatever kind they are: a terminal, a flow,
 * a page, a workspace and its settings. The head names the one in front and
 * carries what acts on it.
 */
export function Field({
  name,
  note,
  actions,
  children,
}: {
  name: string;
  /** What the head says under the name: never a number nobody asked for. */
  note?: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="window-field" aria-label={name}>
      <header className="window-field__head">
        <span className="window-field__name">{name}</span>
        {note === undefined ? null : <span className="window-field__note">{note}</span>}
        {actions === undefined ? null : <span className="window-field__actions">{actions}</span>}
      </header>
      <div className="window-field__body">{children}</div>
    </section>
  );
}
