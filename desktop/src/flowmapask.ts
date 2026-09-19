/**
 * **THE MAP READS THE FILES, NOT A LIST THE ENGINE HAS ALREADY SETTLED.**
 * `flows` hands over one flow per name, the clash resolved; the map needs both
 * copies in the order the engine reads them. The field names are the contract
 * in `src-tauri/src/flows.rs`, held to this file by `flowmapcontract.test.ts`.
 */
import { invoker } from "./engine";
import type { FlowFileOnDisk, FlowOrigin } from "./flowmapread";

/** One flow file as the shell hands it over. */
export interface FlowText {
  name: string;
  /** The source's own word for itself: «built in», «yours», «the project's». */
  origin: string;
  text: string;
  /** Present only when the file would not open at all. */
  unreadable?: string;
}

/** The one origin the shell gives the flows that travel inside the binary. */
const FROM_THE_BINARY = "built in";

/**
 * Where the map splits them: what ships with Sailor, and what a person wrote.
 * The shell names three sources and the map draws two, because the question it
 * answers is whose flow this is — not which of a person's folders holds it.
 */
export function origin(said: string): FlowOrigin {
  return said === FROM_THE_BINARY ? "shipped" : "own";
}

export function asTheMapReadsThem(said: FlowText[]): FlowFileOnDisk[] {
  return said.map((one) => ({
    name: one.name,
    origin: origin(one.origin),
    text: one.text,
    unreadable: one.unreadable,
  }));
}

/** Every flow file on this machine, in the order the engine looks at them. */
export function flowTexts(): Promise<FlowFileOnDisk[]> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no engine to ask"));
  return invoke<FlowText[]>("flow_texts").then(asTheMapReadsThem);
}
