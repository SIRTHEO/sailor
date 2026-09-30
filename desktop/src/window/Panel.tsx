import type { ReactNode } from "react";

/**
 * **THE PANEL PICKS WHICH THING.** It never holds one: what is chosen opens in
 * the field, so a list and the thing it named are never the same surface.
 */
export function Panel({
  title,
  action,
  children,
}: {
  title: string;
  /** One button at most, and it is what this list is for: a new one. */
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="window-panel">
      <div className="window-panel__head">
        <div className="window-panel__title">
          <h1 className="window-panel__name">{title}</h1>
          {action}
        </div>
      </div>
      <div className="window-panel__list">{children}</div>
    </div>
  );
}
