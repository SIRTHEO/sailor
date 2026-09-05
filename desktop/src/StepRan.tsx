import type { Ran } from "./engine";

/**
 * The line a step started, on one line: the program and then its arguments, in
 * the order the process received them. What a person would have to type to
 * reach the same outcome by hand, instead of inferring it from the outcome.
 */

// A word with a space in it, or nothing in it, sits between «» so the reader
// can tell where each one begins and ends: the shape the ledger writes, so a
// line reads alike in both places.
function asOneWord(text: string): string {
  return text === "" || /\s/.test(text) ? `«${text}»` : text;
}

export function renderRan(ran: Ran): string {
  return [ran.program, ...ran.args].map(asOneWord).join(" ");
}

export function StepRan({ ran }: { ran: Ran }) {
  return <code className="step-ran">{renderRan(ran)}</code>;
}
