import type { Refusal } from "./engine";
import { t, tryT } from "./i18n";

/**
 * A refusal as structure: which check, which rule, at which path, and what it
 * saw, each in its own element. The sentence a step also carries in `said`
 * stays prose; this is the part a person scans and a rule can be read off.
 */

/** The rule in a person's words, or its engine name when the window has none. */
export function ruleWords(rule: string): string {
  return tryT(`run.refusal.rule.${rule}`) ?? rule;
}

export function StepRefusal({ refusal }: { refusal: Refusal }) {
  return (
    <div className="step-refusal" data-check={refusal.check} data-rule={refusal.rule}>
      <span className="step-refusal__check">{t("window.refusal.check", { check: refusal.check })}</span>
      <span className="step-refusal__rule">{ruleWords(refusal.rule)}</span>
      <span className="step-refusal__path">
        {refusal.path === "" ? t("window.refusal.whole") : t("window.refusal.at_path", { path: refusal.path })}
      </span>
      <code className="step-refusal__seen">{refusal.seen}</code>
    </div>
  );
}
