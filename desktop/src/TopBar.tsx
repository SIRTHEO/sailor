// The bar of the program: where the person is, and the facts that hold from
// every place. **NOTHING WHOSE SUBJECT IS THE STAGE LIVES HERE**: six controls
// of one section out of six sat in the strip that never leaves the screen,
// against a spec that allows the bar three facts and nothing else.

import type { ReactNode } from "react";

interface TopBarProps {
  /** Where the person is: the section, and the entry inside it. */
  crumbs: string[];
  /** What runs, what it costs, who as: drawn from every place. */
  chips?: ReactNode;
}

export function TopBar({ crumbs, chips }: TopBarProps) {
  return (
    <header className="topbar">
      <span className="topbar__brand">
        <svg
          className="topbar__mark"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M3 17l9-13 9 13" />
          <path d="M3 17c2.5 2 5 2 7.5 0S16 15 18.5 17" />
        </svg>
        Sailor
      </span>
      <span className="topbar__rule" />

      <nav className="topbar__crumbs" aria-label="where you are">
        {crumbs.map((crumb, index) => (
          <span className="topbar__crumb" key={`${index}-${crumb}`}>
            {crumb}
          </span>
        ))}
      </nav>

      <span className="topbar__spacer" />

      {chips}
    </header>
  );
}
