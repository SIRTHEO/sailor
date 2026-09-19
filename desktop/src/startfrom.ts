// The catalogue, asked of the shell: the entries a flow is started from, and
// the gesture that makes one. The shell takes the command line's own road.

import { invoker } from "./engine";
import { t } from "./i18n";

export interface CatalogueInput {
  name: string;
  means: string;
  required: boolean;
}

export interface CatalogueStep {
  id: string;
  action: string;
  deps: string[];
}

export interface CatalogueEntry {
  name: string;
  /** Absent on an entry the catalogue's judge refused. */
  kind?: "template" | "example";
  purpose: string;
  inputs: CatalogueInput[];
  teaches?: { capability: string; result: string };
  steps: CatalogueStep[];
  refused?: string;
}

export interface CatalogueReading {
  entries: CatalogueEntry[];
  /** Where each destination writes, or `null` when none is in sight. */
  workspace: string | null;
  home: string | null;
}

export type Destination = "workspace" | "home";

export interface MadeFlow {
  flow: string;
  origin: string;
  directory: string;
}

export async function readCatalogue(): Promise<CatalogueReading> {
  const invoke = invoker();
  if (!invoke) throw new Error(t("window.catalogue.outside"));
  return invoke<CatalogueReading>("flow_catalogue");
}

export async function makeFromCatalogue(
  entry: string,
  name: string,
  inputs: Record<string, string>,
  place: Destination,
): Promise<MadeFlow> {
  const invoke = invoker();
  if (!invoke) throw new Error(t("window.catalogue.outside"));
  return invoke<MadeFlow>("flow_from_catalogue", { entry, name, inputs, place });
}

/** The required inputs still blank: the gesture waits for them. */
export function inputsStillMissing(entry: CatalogueEntry, values: Record<string, string>): string[] {
  return entry.inputs
    .filter((input) => input.required && (values[input.name] ?? "").trim() === "")
    .map((input) => input.name);
}

/** The place a new flow goes unless the person picks the other. */
export function firstDestination(reading: CatalogueReading): Destination {
  return reading.workspace !== null ? "workspace" : "home";
}
