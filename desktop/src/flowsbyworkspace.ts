/**
 * WHAT RUNS IN EVERY CHECKOUT, AS THE SHELL RESOLVED IT. The field names are
 * the contract written in `src-tauri/src/flows_by_workspace.rs`; the grouping
 * here reads winners, it never decides one.
 */
import { invoker } from "./engine";
import type { FlowChain } from "./flowchain";

/** One flow name as it resolves in one context. */
export interface FlowRow extends FlowChain {
  /** `null` when the winning file will not load. */
  steps: number | null;
  broken?: string;
}

/** Why a checkout carries no rows: facts the window puts into words. */
export type Refusal =
  | { kind: "unreadable"; why: string }
  | { kind: "timed_out"; seconds: number }
  | { kind: "stopped" };

/** A source folder that is there and refused the reading. */
export interface SourceTrouble {
  origin: string;
  dir: string;
  why: string;
}

/** A source outside every checkout that did not answer, and was left out. */
export interface Unanswered {
  origin: string;
  dir: string;
  refused: Refusal;
}

/** One place flows are resolved from: a checkout, or outside every workspace. */
export interface FlowContext {
  workspace: string | null;
  root: string | null;
  branch: string | null;
  /** Git did not answer in time: no branch is known, which is not «no branch». */
  branch_unread?: boolean;
  current: boolean;
  /** Only the names whose winner differs from outside every workspace. */
  flows: FlowRow[];
  troubles?: SourceTrouble[];
  unanswered?: Unanswered[];
  refused?: Refusal;
}

export interface FlowsReading {
  outside: FlowContext;
  contexts: FlowContext[];
}

/** What the place knows: still reading, read, or refused with the reason. */
export type FlowsAsk =
  | { state: "reading" }
  | { state: "read"; reading: FlowsReading }
  | { state: "unreadable"; why: string };

function ask<T>(command: string): Promise<T> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the native shell: no disk to look at"));
  return invoke<T>(command);
}

/** Where the window stands, and outside every workspace: no other checkout. */
export function flowsHere(): Promise<FlowsReading> {
  return ask<FlowsReading>("flows_here");
}

/** Every known checkout, each within the shell's limit. */
export function flowsByWorkspace(): Promise<FlowsReading> {
  return ask<FlowsReading>("flows_by_workspace");
}

/** The origin a project's own flows carry, either way it was found. */
export function isProjectOrigin(origin: string): boolean {
  return origin.startsWith("this project");
}

/**
 * Every name as it resolves in one context: the context's own rows, then the
 * rows outside every workspace that it does not decide itself.
 */
export function resolvedIn(reading: FlowsReading, context: FlowContext): FlowRow[] {
  if (context.refused !== undefined) return [];
  if (context === reading.outside) return [...context.flows].sort(byName);
  const own = new Set(context.flows.map((row) => row.name));
  const rest = reading.outside.flows.filter((row) => !own.has(row.name));
  return [...context.flows, ...rest].sort(byName);
}

function byName(one: { name: string }, other: { name: string }): number {
  return one.name.localeCompare(other.name);
}

/** The context the window stands in: a checkout, or outside every workspace. */
export function standingContext(reading: FlowsReading): FlowContext {
  return reading.contexts.find((context) => context.current) ?? reading.outside;
}

/** The key of the checkout's own group; every other group is keyed by origin. */
export const OF_THE_WORKSPACE = "workspace";

export interface FlowGroupOfPlace {
  key: string;
  rows: FlowRow[];
}

/** «of this workspace», then every other origin, most specific first. */
export function groupsHere(reading: FlowsReading): FlowGroupOfPlace[] {
  const order = [OF_THE_WORKSPACE, "declared", "yours", "built in"];
  const byKey = new Map<string, FlowRow[]>();
  for (const row of resolvedIn(reading, standingContext(reading))) {
    const key = isProjectOrigin(row.winner.origin) ? OF_THE_WORKSPACE : row.winner.origin;
    byKey.set(key, [...(byKey.get(key) ?? []), row]);
  }
  const rank = (key: string) => (order.includes(key) ? order.indexOf(key) : order.length);
  return [...byKey]
    .sort(([one], [other]) => rank(one) - rank(other))
    .map(([key, rows]) => ({ key, rows }));
}

/** One context's winner for a name, in the global view. */
export interface WinnerIn {
  context: FlowContext;
  row: FlowRow;
}

/** One name across every context it resolves in. */
export interface GlobalRow {
  name: string;
  winners: WinnerIn[];
  /** True when two contexts run two different files under this name. */
  differs: boolean;
}

/** Every name, with the contexts it resolves in and whether they disagree. */
export function globalRows(reading: FlowsReading): GlobalRow[] {
  const contexts = [reading.outside, ...reading.contexts];
  const byName = new Map<string, WinnerIn[]>();
  for (const context of contexts) {
    for (const row of resolvedIn(reading, context)) {
      byName.set(row.name, [...(byName.get(row.name) ?? []), { context, row }]);
    }
  }
  return [...byName]
    .sort(([one], [other]) => one.localeCompare(other))
    .map(([name, winners]) => ({
      name,
      winners,
      differs: new Set(winners.map((one) => `${one.row.winner.origin}\n${one.row.winner.path}`)).size > 1,
    }));
}

/** Whether anything resolves anywhere. */
export function holdsAnyFlow(reading: FlowsReading): boolean {
  return reading.outside.flows.length > 0 || reading.contexts.some((one) => one.flows.length > 0);
}

/** Every folder that refused the reading, outside and in each checkout. */
export function troublesOf(reading: FlowsReading): SourceTrouble[] {
  return [reading.outside, ...reading.contexts].flatMap((context) => context.troubles ?? []);
}

/** Every source outside the checkouts that did not answer in time. */
export function unansweredOf(reading: FlowsReading): Unanswered[] {
  return [reading.outside, ...reading.contexts].flatMap((context) => context.unanswered ?? []);
}

/** Whether the chosen row is the file that runs where the window stands. */
export function runsWhereTheWindowStands(reading: FlowsReading, row: FlowRow): boolean {
  const here = resolvedIn(reading, standingContext(reading)).find((one) => one.name === row.name);
  return here !== undefined && here.winner.path === row.winner.path && here.winner.origin === row.winner.origin;
}
