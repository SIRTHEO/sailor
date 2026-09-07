import type { Why } from "./engine";

/**
 * Why the engine did with a step what it did: the place read, the demand,
 * what was there, the verdict. A skipped step looks like nothing happened,
 * and three of those four facts were nowhere. Nothing there is not empty.
 */

export function renderWhy(why: Why): string {
  const asked = [why.looked_at, why.wanted].filter((one) => one !== null).join(" ");
  const found = why.found_was_there ? `it was ${JSON.stringify(why.found)}` : "nothing was there";
  return `${why.held ? "ran" : "skipped"}: ${asked} — ${found}`;
}

export function StepWhy({ why }: { why: Why }) {
  return (
    <div className="step-why" data-held={why.held ? "" : undefined}>
      {renderWhy(why)}
    </div>
  );
}
