/**
 * WHICH FILE RUNS UNDER A NAME, AND WHERE THAT WAS DECIDED. The shell reads
 * the chain from `flow::system::chains` in the window's own working directory,
 * which is not the terminal's: the words carry it, so a person can see why the
 * two may pick different flows.
 */
import { invoker } from "./engine";
import { t } from "./i18n";

/** One place a flow name can come from: a file, or the shipped place. */
export interface FlowCandidate {
  origin: string;
  path: string;
}

/** Every candidate for one name, least specific first, and the one that runs. */
export interface FlowChain {
  name: string;
  resolved_in: string | null;
  replaced: FlowCandidate[];
  winner: FlowCandidate;
  /** Set when the winner replaces an older version of the shipped flow. */
  stale?: { shipped: number; copied: number | null };
}

/** The chains of every flow the shell sees, read in one call. */
export async function flowChains(): Promise<FlowChain[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no disk to look at");
  return invoke<FlowChain[]>("flow_chains");
}

/** What the window knows of the chains: read, or refused with the reason. */
export type ChainsRead =
  | { state: "read"; chains: Map<string, FlowChain> }
  | { state: "unread"; why: string };

/** The chains, or the refusal kept as a state: never an empty map in its place. */
export async function readChains(): Promise<ChainsRead> {
  try {
    const chains = await flowChains();
    return { state: "read", chains: new Map(chains.map((chain) => [chain.name, chain])) };
  } catch (error: unknown) {
    return { state: "unread", why: String(error) };
  }
}

/** The mark a flow wears and its title, or `null` when it replaces nothing. */
export interface ChainMark {
  text: string;
  title: string;
}

/**
 * **AN UNREAD CHAIN MARKS EVERY FLOW.** Drawn bare, a flow that replaces a
 * shipped one would look exactly like one that does not.
 */
export function chainMark(read: ChainsRead, name: string): ChainMark | null {
  if (read.state === "unread") return { text: t("window.flow.chains_unread"), title: read.why };
  const chain = read.chains.get(name);
  const text = replacesWords(chain);
  return chain && text ? { text, title: chainWords(chain) } : null;
}

/**
 * «replaces built in», and the versions when the shipped flow rose past the
 * copy; `null` when the flow that runs hides nothing.
 */
export function replacesWords(chain: FlowChain | undefined): string | null {
  if (!chain || chain.replaced.length === 0) return null;
  const replaced = [...new Set(chain.replaced.map((one) => one.origin))].join(", ");
  const stale = chain.stale;
  if (!stale) return t("window.flow.replaces", { replaced });
  const shipped = String(stale.shipped);
  return stale.copied === null
    ? t("window.flow.replaces_unsaid", { replaced, shipped })
    : t("window.flow.replaces_stale", { replaced, copied: String(stale.copied), shipped });
}

/** The whole chain, one candidate a line, headed by where it was resolved. */
export function chainWords(chain: FlowChain): string {
  const directory = chain.resolved_in ?? t("window.flow.chain_unknown_directory");
  return [
    t("window.flow.chain_head", { directory }),
    ...chain.replaced.map((one) => t("window.flow.chain_replaced", { origin: one.origin, path: one.path })),
    t("window.flow.chain_runs", { origin: chain.winner.origin, path: chain.winner.path }),
  ].join("\n");
}
