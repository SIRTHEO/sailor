// The tools that run a node — an AI command line, an MCP server, any binary —
// and the fields a node uses them with.

// THERE IS NO LIST OF TOOLS IN HERE, AND THERE MUST NOT BE. Whoever installs
// the product has different things on disk from whoever writes it: the window
// asks the engine (`discover_tools`) and draws what it gets back. A closed type
// with three names in it would be a shopping list disguised as a type.

// The four fields (tool, model, options, prompt) live in the step's params.
// The engine does not read them today: `external_engine` wants `bin`, `args`,
// `env`, `stdin`, `timeout_secs`, and bridging an id to a binary belongs to
// whoever executes. What this side can do is not lie: if a step already
// declares its own binary, the panel says so.

// THE FAMILY TYPE IS OPEN for the same reason: a fourth family arriving
// tomorrow shows under its own name instead of hiding the tool.

import { useSyncExternalStore } from "react";
import type { ValueSchema } from "./flow";

/** The families known today; the engine may declare others and they are kept. */
export type ToolKind = "ai_cli" | "mcp" | "tool" | (string & {});

/**
 * A tool as the engine describes it. The six fields are the minimum contract of
 * `discover_tools`; anything else the engine wants to add arrives without
 * breaking anything.
 */
export interface Tool {
  id: string;
  name: string;
  kind: ToolKind;
  path: string;
  version: string;
  available: boolean;
  /**
   * Why it is the way it is: where it was found, or why it is missing, or why
   * we could not look. A MISSING TOOL IS NOT HIDDEN — it is shown disabled with
   * this beside it, or a node that will not start is a guessing game.
   */
  reason: string;
  /** Which descriptor recognised it: the address of a wrong line. */
  descriptor: string;
  /** Models the engine suggests for this tool, if it knows any. */
  models?: string[];
  /** The options this tool accepts, if the descriptor declares them. */
  options?: OptionSpec[];
  [extra: string]: unknown;
}

/**
 * An option as a descriptor will describe it. NO DESCRIPTOR DECLARES ONE YET:
 * the type exists so the panel can already draw the guided choice, and while
 * the list is empty the panel shows the free field and says so. Inventing
 * plausible flags here would look machine-detected while written from memory.
 */
export interface OptionSpec {
  /** The option's name as the tool spells it, e.g. `--model`. */
  key: string;
  /** What it is called for a reader; falls back to `key`. */
  label?: string;
  /** The value's shape. An unknown shape is treated as text. */
  kind: "text" | "number" | "flag" | "choice";
  /** The allowed values, when the shape is `choice`. */
  choices?: string[];
  /** One line of explanation for whoever is choosing. */
  help?: string;
}

/**
 * What the window knows about the installed tools. "Silent" is not "no tools":
 * the first is an engine that did not answer, the second a machine with
 * nothing installed, and the two must stay distinguishable.
 */
export type ToolDiscovery =
  | { state: "asking" }
  | { state: "ready"; tools: Tool[] }
  | { state: "mute"; why: string };

// `mcp_server` IS THE FAMILY THE SHIPPED DESCRIPTORS ACTUALLY WRITE, measured
// by running the detector (`cargo run --example scan -p toolbox`). Both spellings
// stay: somebody else's descriptor may use either, and neither should surface a
// raw token in the interface.
const TOOL_KIND_LABEL: Record<string, string> = {
  ai_cli: "AI command line",
  mcp: "MCP server",
  mcp_server: "MCP server",
  tool: "tool",
};

/** A family's label; an unknown family is shown under the name it has. */
export function toolKindLabel(kind: ToolKind): string {
  return TOOL_KIND_LABEL[kind] ?? kind;
}

/**
 * Reads the engine's answer without trusting its shape. If it ever answers with
 * a field missing, the window drops that entry and shows the others rather than
 * going dark. A dropped entry is invisible — the price of not having a blank
 * screen — but the caller still knows how many arrived.
 */
export function parseTools(payload: unknown): Tool[] {
  if (!Array.isArray(payload)) return [];
  const tools: Tool[] = [];
  for (const item of payload) {
    if (typeof item !== "object" || item === null) continue;
    const record = item as Record<string, unknown>;
    const id = typeof record.id === "string" ? record.id : null;
    if (!id) continue;
    tools.push({
      ...record,
      id,
      name: typeof record.name === "string" && record.name !== "" ? record.name : id,
      kind: typeof record.kind === "string" ? record.kind : "tool",
      path: typeof record.path === "string" ? record.path : "",
      version: typeof record.version === "string" ? record.version : "",
      // A missing field does not promise the tool is there: assume absent.
      available: record.available === true,
      reason: typeof record.reason === "string" ? record.reason : "",
      descriptor: typeof record.descriptor === "string" ? record.descriptor : "",
      models: parseStrings(record.models),
      options: parseOptionSpecs(record.options),
    });
  }
  return tools;
}

/** The strings of a list, skipping whatever is not one. */
function parseStrings(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string" && item !== "");
}

/**
 * The declared options, read without trusting the shape. When the field exists
 * it will be written by whoever adds a descriptor — a user, with a hand-written
 * JSON file — so a wrong line must lose that line, not the panel. An unknown
 * shape becomes `text`, the only choice that keeps the value already written.
 */
export function parseOptionSpecs(value: unknown): OptionSpec[] {
  if (!Array.isArray(value)) return [];
  const specs: OptionSpec[] = [];
  for (const item of value) {
    if (typeof item !== "object" || item === null) continue;
    const record = item as Record<string, unknown>;
    const key = typeof record.key === "string" ? record.key : "";
    if (key === "") continue;
    const kind = record.kind;
    specs.push({
      key,
      label: typeof record.label === "string" && record.label !== "" ? record.label : undefined,
      kind:
        kind === "number" || kind === "flag" || kind === "choice" || kind === "text"
          ? kind
          : "text",
      choices: parseStrings(record.choices),
      help: typeof record.help === "string" && record.help !== "" ? record.help : undefined,
    });
  }
  return specs;
}

/** Tools in reading order: usable ones first, then by name. */
export function sortTools(tools: Tool[]): Tool[] {
  return [...tools].sort((left, right) => {
    if (left.available !== right.available) return left.available ? -1 : 1;
    return left.name.localeCompare(right.name);
  });
}

/** Tools grouped by family, each group already sorted. */
export function groupByKind(tools: Tool[]): Array<{ kind: ToolKind; tools: Tool[] }> {
  const groups = new Map<ToolKind, Tool[]>();
  for (const tool of sortTools(tools)) {
    const group = groups.get(tool.kind);
    if (group) group.push(tool);
    else groups.set(tool.kind, [tool]);
  }
  return Array.from(groups.entries()).map(([kind, list]) => ({ kind, tools: list }));
}

// ── the fields a node uses a tool with ──────────────────────────────────
//
// They live in the step's params (`with`), not in new `Step` fields: the step
// shape mirrors `crates/flow`, and adding keys from the window side alone would
// let the two truths drift. What lands in the flow file is the tool's ID, never
// its path — an id is true on any machine, an absolute path on exactly one.

export const TOOL_KEY = "tool";
export const MODEL_KEY = "model";
export const PROMPT_KEY = "prompt";
export const OPTIONS_KEY = "options";

const MANAGED_KEYS = [TOOL_KEY, MODEL_KEY, PROMPT_KEY, OPTIONS_KEY];

/**
 * An option's value. `true` is the valueless switch (`--verbose`); `false` is
 * not written at all — an option turned off is removed, and leaving it written
 * as `false` would read as somebody having disabled it on purpose.
 */
export type OptionValue = string | number | boolean;

export interface ToolChoice {
  /** The tool's id, as the engine declares it. */
  tool: string;
  /** Free text: models change faster than any list. */
  model: string;
  prompt: string;
  /**
   * The chosen options, by name. Insertion order is preserved for string keys,
   * so a reader of the file finds the command line as it was composed.
   */
  options: Record<string, OptionValue>;
}

function textAt(params: Record<string, unknown> | null | undefined, key: string): string {
  const value = params?.[key];
  return typeof value === "string" ? value : "";
}

/**
 * True if the panel's field can **hold** this value without losing part of it.
 * The text fields hold a string; the options hold an object of scalars only.
 */
function panelCanHold(key: string, value: unknown): boolean {
  if (key !== OPTIONS_KEY) return typeof value === "string";
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  return Object.values(value as Record<string, unknown>).every(
    (item) => typeof item === "string" || typeof item === "number" || typeof item === "boolean",
  );
}

/**
 * Splits the panel-managed fields from the rest. **WHAT CANNOT BE READ MUST NOT
 * BE REWRITTEN**: a managed field the panel cannot hold stays among the rest,
 * returns to disk as it was and shows in the JSON box, since omitting a field
 * is a write too. `tool` paid for it: as a plain string a `Chain` saves as `""`.
 */
export function splitToolParams(params: Record<string, unknown> | null | undefined): {
  choice: ToolChoice;
  rest: Record<string, unknown>;
} {
  const rest: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(params ?? {})) {
    if (!MANAGED_KEYS.includes(key) || !panelCanHold(key, value)) rest[key] = value;
  }
  return {
    choice: {
      tool: textAt(params, TOOL_KEY),
      model: textAt(params, MODEL_KEY),
      prompt: textAt(params, PROMPT_KEY),
      options: panelCanHold(OPTIONS_KEY, params?.[OPTIONS_KEY])
        ? readOptions(params?.[OPTIONS_KEY])
        : {},
    },
    rest,
  };
}

/**
 * The options written in the step. A value that is neither text, nor number,
 * nor switch is dropped rather than converted: an object turned into
 * `"[object Object]"` would land on disk in place of what was there.
 */
function readOptions(value: unknown): Record<string, OptionValue> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const options: Record<string, OptionValue> = {};
  for (const [key, item] of Object.entries(value as Record<string, unknown>)) {
    if (typeof item === "string" || typeof item === "number" || typeof item === "boolean") {
      options[key] = item;
    }
  }
  return options;
}

/**
 * How the resulting command line would read. It shows what is being composed
 * and is NOT what ends up in the file: on disk the options stay by name, to be
 * re-read and recomposed. A preview, and to be read as one.
 */
export function optionsPreview(choice: ToolChoice): string {
  const words: string[] = [];
  for (const [key, value] of Object.entries(choice.options)) {
    if (value === false) continue;
    words.push(key);
    if (value !== true) words.push(String(value));
  }
  return words.join(" ");
}

/**
 * Puts the params back together. An empty field is not written as an empty
 * string, so on disk "I did not choose it" stays apart from "I chose nothing".
 * `rest` is copied **first**: a field `splitToolParams` could not hold — a
 * chain in `tool` — survives unless the panel made an explicit choice.
 */
export function joinToolParams(
  rest: Record<string, unknown>,
  choice: ToolChoice,
): Record<string, unknown> | null {
  const params: Record<string, unknown> = { ...rest };
  if (choice.tool !== "") params[TOOL_KEY] = choice.tool;
  if (choice.model !== "") params[MODEL_KEY] = choice.model;
  if (choice.prompt !== "") params[PROMPT_KEY] = choice.prompt;
  // An option with no name is not written: it is a row still being composed,
  // and saving it would give the engine an empty key.
  const options: Record<string, OptionValue> = {};
  for (const [key, value] of Object.entries(choice.options)) {
    if (key.trim() !== "") options[key] = value;
  }
  if (Object.keys(options).length > 0) params[OPTIONS_KEY] = options;
  return Object.keys(params).length === 0 ? null : params;
}

/**
 * True if the step's input schema would reject the panel's fields. A step may
 * declare a closed object (`allow_extra: false`), and real flows do: writing a
 * key the schema does not list produces a file the engine rejects at load. The
 * panel says so while it is typed, not at save time.
 */
export function schemaRejectsToolKeys(schema: ValueSchema, choice: ToolChoice): boolean {
  if (schema.type !== "object" || schema.allow_extra) return false;
  const written = joinToolParams({}, choice);
  if (written === null) return false;
  return Object.keys(written).some((key) => !(key in schema.properties));
}

/**
 * The binary the step declares on its own, if there is one. It coexists with
 * the tool field only by mistake: they answer the same question.
 */
export function rivalBinary(rest: Record<string, unknown>): string {
  const bin = rest.bin;
  return typeof bin === "string" ? bin : "";
}

/** The id of the tool a step chose, if it chose one. */
export function toolOf(params: Record<string, unknown> | null | undefined): string {
  return textAt(params, TOOL_KEY);
}

/**
 * The engine chain `splitToolParams` left among the other params, if any. The
 * panel uses it to **say so**: a step with a chain and a selector reading "none"
 * would be the same lie the node used to tell, moved one window along. The
 * panel cannot compose one yet; it can say there is one and that it stays.
 */
export function chainIn(rest: Record<string, unknown>): string[] {
  const declared = rest[TOOL_KEY];
  if (!Array.isArray(declared)) return [];
  return declared.filter((id): id is string => typeof id === "string" && id !== "");
}

/**
 * The models to suggest: what the engine declares for the chosen tool, plus
 * what is already written in the other steps. The field stays free — these help
 * with typing, they are not a list of what is allowed.
 */
export function modelSuggestions(tool: Tool | undefined, used: Iterable<string>): string[] {
  const seen = new Set<string>();
  const declared = Array.isArray(tool?.models) ? (tool?.models as unknown[]) : [];
  for (const model of declared) {
    if (typeof model === "string" && model !== "") seen.add(model);
  }
  for (const model of used) {
    if (model !== "") seen.add(model);
  }
  return Array.from(seen);
}

// ── the shared register: whoever asked already tells everyone ────────────
//
// WHY IT EXISTS. A node on the canvas must show the mark and the name of the
// tool that runs it, and whether that tool is on this machine: those are facts
// of the discovery, not of the step. Threading them down the node chain would
// mean rewriting both the layout builder and its mounter.

// There is only one discovery: whoever runs it (`engine.discoverTools`) leaves
// the outcome here and anyone reads it. No second disk query, no mount order to
// respect — a node mounted before the answer shows what the step declares, and
// updates itself when the answer arrives.

let registry: ReadonlyMap<string, Tool> = new Map();
const listeners = new Set<() => void>();

/** Leaves a discovery's outcome here and wakes whoever was watching. */
export function publishTools(tools: Tool[]): void {
  registry = new Map(tools.map((tool) => [tool.id, tool]));
  for (const listener of listeners) listener();
}

/**
 * The known tools, by id. **THE MAP'S IDENTITY CHANGES ONLY ON A NEW
 * DISCOVERY**, and that is not a detail: `useSyncExternalStore` compares
 * references, so returning a fresh map on every read would redraw the canvas
 * forever.
 */
export function knownTools(): ReadonlyMap<string, Tool> {
  return registry;
}

function subscribeTools(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * The tool with this id, if discovery found it. `undefined` answers two
 * different questions — discovery has not answered yet, or it answered and this
 * tool is absent — and the caller must tell them apart: `toolsAreKnown()` says
 * whether an answer arrived.
 */
export function useTool(id: string): Tool | undefined {
  return useSyncExternalStore(
    subscribeTools,
    () => (id === "" ? undefined : registry.get(id)),
    () => undefined,
  );
}

/** True once a discovery has answered something. */
export function useToolsAreKnown(): boolean {
  return useSyncExternalStore(
    subscribeTools,
    () => registry.size > 0,
    () => false,
  );
}
