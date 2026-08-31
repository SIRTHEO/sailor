import type { CallView, RunUsage } from "./flow";
import { nodeId } from "./layout";

/**
 * Quanto è costato un singolo passo, e con quale motore l'ha fatto.
 *
 * **Perché esiste.** `RunUsage.calls` arriva già dal deposito a ogni corsa
 * guardata, porta per ogni chiamata il modello davvero usato, i token e il
 * costo — e finiva schiacciato in una sola riga di totale in fondo alla
 * console. Chi guardava un flusso non poteva sapere quale passo aveva speso,
 * né su quale modello: era esattamente l'opacità che il vincolo «chiarezza per
 * chi guarda» vieta.
 *
 * Sta in un file suo, senza React, perché il calcolo si possa provare senza
 * montare una tela — la stessa ragione per cui `runstate.ts` è stato staccato.
 */

export interface StepUsage {
  /**
   * I modelli **davvero** usati, in ordine di prima chiamata.
   *
   * È un elenco e non una stringa perché un passo che ritenta può cadere su un
   * modello diverso dal primo, e mostrarne uno solo direbbe il falso su cosa è
   * successo. Il modello dichiarato nel passo è un'altra cosa e sta altrove:
   * questo è quello che il motore ha risposto di aver usato.
   */
  models: string[];
  inputTokens: number;
  outputTokens: number;
  /**
   * `null` quando **nessuna** chiamata del passo ha dichiarato un costo.
   *
   * Non è zero: zero vorrebbe dire «è girato gratis», e la differenza è quella
   * già scritta fra le decisioni — Codex dichiara il totale dei token e non i
   * due lati, quindi la sua riga resta senza costo. Un passo che mostra `0,0000 $`
   * dove nessuno ha misurato niente è una misura inventata.
   */
  costMicros: number | null;
  calls: number;
  /** Quante chiamate sono rimaste fuori dal conto del costo. */
  callsWithoutCost: number;
}

/** Somma nulla, per un passo che non ha ancora chiamato nessuno. */
function empty(): StepUsage {
  return { models: [], inputTokens: 0, outputTokens: 0, costMicros: null, calls: 0, callsWithoutCost: 0 };
}

function fold(into: StepUsage, call: CallView): StepUsage {
  const models = into.models.includes(call.actual_model) || call.actual_model === ""
    ? into.models
    : [...into.models, call.actual_model];
  // Un costo assente lascia `null` finché non ne arriva uno vero: sommare
  // `null` come zero è il modo esatto in cui un totale parziale si traveste da
  // totale.
  const cost = call.cost_micros === null
    ? into.costMicros
    : (into.costMicros ?? 0) + call.cost_micros;
  return {
    models,
    inputTokens: into.inputTokens + (call.input_tokens ?? 0),
    outputTokens: into.outputTokens + (call.output_tokens ?? 0),
    costMicros: cost,
    calls: into.calls + 1,
    callsWithoutCost: into.callsWithoutCost + (call.cost_micros === null ? 1 : 0),
  };
}

/**
 * Le chiamate di una corsa, raccolte per passo e chiavate `flusso::passo`.
 *
 * La chiave è qualificata col flusso per la ragione già pagata sulla tela
 * unica: `verifica` e `verdetto` esistono in più flussi, e una chiave nuda
 * farebbe apparire su un nodo la spesa di un altro.
 *
 * Le chiamate senza `step_id` — quelle della corsa e non di un passo — restano
 * fuori: appartengono al totale, non a un nodo.
 */
export function stepUsageOfRun(usage: RunUsage | null, flowName: string): Map<string, StepUsage> {
  const perStep = new Map<string, StepUsage>();
  if (!usage) return perStep;
  for (const call of usage.calls) {
    if (call.step_id === null || call.step_id === "") continue;
    const key = nodeId(flowName, call.step_id);
    perStep.set(key, fold(perStep.get(key) ?? empty(), call));
  }
  return perStep;
}

/** Vero quando il costo mostrato è più basso di quello vero, e va detto. */
export function usageIsPartial(usage: StepUsage): boolean {
  return usage.callsWithoutCost > 0;
}

/**
 * Il costo, in euro-virgola-italiana e con l'unità attaccata.
 *
 * Quattro decimali perché una chiamata sola costa spesso meno di un centesimo, e
 * arrotondare a due la farebbe leggere `0,00 $` — che è lo stesso inganno del
 * costo assente mostrato come zero.
 */
export function formatCost(micros: number): string {
  return `${(micros / 1e6).toFixed(4).replace(".", ",")} $`;
}

/**
 * I token in forma corta: `840`, `12,4k`, `1,03M`.
 *
 * Su un nodo non c'è spazio per sette cifre, e il numero esatto non è la
 * domanda che si fa guardando una tela — l'ordine di grandezza sì.
 */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1e6) return `${(n / 1000).toFixed(1).replace(".", ",")}k`;
  return `${(n / 1e6).toFixed(2).replace(".", ",")}M`;
}
