/**
 * **ONE LIST SERVES THE WORKSPACE AND THE MACHINE.** Without the switch every
 * screen exists twice — «the flows here» and «the flows everywhere» — and the
 * two drift.
 */
import { t } from "../i18n";

export type Scope = "here" | "all";

/**
 * **A WORKSPACE NOBODY IS IN OFFERS NO «HERE».** A switch with a side that
 * counts nothing is a control that lies about having two answers; the same
 * reason the bar drops the branch where git is absent.
 */
export function scopesOffered(inAWorkspace: boolean): Scope[] {
  return inAWorkspace ? ["here", "all"] : ["all"];
}

export function scopeName(scope: Scope): string {
  return scope === "here" ? t("window.scope.here") : t("window.scope.all");
}

/** The scope a list falls back to when the one asked for is not on offer. */
export function scopeInForce(asked: Scope, inAWorkspace: boolean): Scope {
  return scopesOffered(inAWorkspace).includes(asked) ? asked : "all";
}
