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
}

/** The chains of every flow the shell sees, read in one call. */
export async function flowChains(): Promise<FlowChain[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no disk to look at");
  return invoke<FlowChain[]>("flow_chains");
}

/** «replaces built in», or `null` when the flow that runs hides nothing. */
export function replacesWords(chain: FlowChain | undefined): string | null {
  if (!chain || chain.replaced.length === 0) return null;
  const origins = [...new Set(chain.replaced.map((one) => one.origin))];
  return t("window.flow.replaces", { replaced: origins.join(", ") });
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
