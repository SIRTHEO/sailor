import { scopeName, scopesOffered } from "./scope";
import type { Scope } from "./scope";

/**
 * The count sits inside the side it belongs to: a number beside a switch,
 * without its side, answers a question nobody can tell was asked.
 */
export function ScopeSwitch({
  scope,
  inAWorkspace,
  counts,
  onScope,
}: {
  scope: Scope;
  inAWorkspace: boolean;
  counts: Record<Scope, string>;
  onScope: (scope: Scope) => void;
}) {
  const offered = scopesOffered(inAWorkspace);
  // One side is not a switch: it would ask a person to choose between one
  // thing and nothing.
  if (offered.length < 2) return null;
  return (
    <div className="window-scope">
      {offered.map((side) => (
        <button
          key={side}
          type="button"
          className="window-scope__button"
          aria-pressed={side === scope}
          onClick={() => { onScope(side); }}
        >
          {scopeName(side)}
          <span className="window-scope__count">{counts[side]}</span>
        </button>
      ))}
    </div>
  );
}
