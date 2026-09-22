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

/**
 * **A THING THAT CARRIES ITS OWN HEAD IS NOT GIVEN A SECOND ONE.** A terminal
 * names its tree, its tty and its state, and holds what acts on it: a head
 * above that would say the same three words again.
 */
export function FieldHeld({ name, children }: { name: string; children: ReactNode }) {
  return (
    <section className="window-field" aria-label={name}>
      <div className="window-field__body">{children}</div>
    </section>
  );
}

/**
 * **NOTHING OPEN IS NOT A THING CALLED «NOTHING OPEN».** A head over an empty
 * field names something that is not there, and then the body repeats it: the
 * field says once what fills it, and carries no head until it holds one.
 */
export function FieldEmpty({ say }: { say: string }) {
  return (
    <section className="window-field" aria-label={say}>
      <p className="window-field__empty">{say}</p>
    </section>
  );
}
