// Gli strumenti che eseguono un nodo — una riga di comando con un'IA dietro, un
// server MCP, un binario qualunque — e i tre campi con cui un nodo li usa.
//
// QUI DENTRO NON C'È NESSUN ELENCO DI STRUMENTI, E NON DEVE ESSERCI. Chi
// installa il prodotto ha altre cose sul disco rispetto a chi lo scrive: la
// finestra chiede al motore cosa c'è (`discover_tools`) e disegna quello che
// riceve. Un tipo chiuso con tre nomi dentro sarebbe una lista della spesa
// travestita da tipo, e il primo utente con un quarto strumento resterebbe
// fuori dal prodotto.
//
// I quattro campi (strumento, modello, opzioni, prompt) stanno nei parametri
// del passo. Il motore oggi non li legge: `external_engine` vuole `bin`,
// `args`, `env`, `stdin`, `timeout_secs`, e il ponte fra un identificativo e un
// binario è di chi esegue, non di chi disegna. Quello che si può fare da questa
// parte è non mentire: se un passo dichiara già un binario suo, o se il suo
// schema d'ingresso rifiuterebbe questi campi, il pannello lo dice.
//
// LA SPECIE È APERTA per lo stesso motivo: le tre note oggi hanno un'etichetta
// in italiano, una quarta che arrivasse domani si mostra col suo nome invece di
// far scomparire lo strumento o di dire «sconosciuto».

import { useSyncExternalStore } from "react";
import type { ValueSchema } from "./flow";

/** Le specie note oggi; il motore può dichiararne altre e la finestra le accetta. */
export type ToolKind = "ai_cli" | "mcp" | "tool" | (string & {});

/**
 * Uno strumento come il motore lo descrive. I sei campi sono il contratto
 * minimo di `discover_tools`; tutto il resto che il motore volesse aggiungere
 * arriva qui senza rompere niente — `models` è il primo caso previsto, e se
 * manca il campo del modello resta libero.
 */
export interface Tool {
  id: string;
  name: string;
  kind: ToolKind;
  path: string;
  version: string;
  available: boolean;
  /**
   * Perché è così: da dove è stato trovato, o perché non c'è, o perché non si è
   * potuto guardare. UNO STRUMENTO ASSENTE NON SI NASCONDE, si mostra spento
   * con questo accanto — senza, chi guarda un nodo che non parte può solo
   * aprire un terminale e indovinare.
   */
  reason: string;
  /** Da quale descrittore è stato riconosciuto: l'indirizzo di una riga sbagliata. */
  descriptor: string;
  /** Modelli che il motore suggerisce per questo strumento, se li conosce. */
  models?: string[];
  /** Le opzioni che questo strumento accetta, se il descrittore le dichiara. */
  options?: OptionSpec[];
  [extra: string]: unknown;
}

/**
 * Un'opzione come il descrittore la descriverà.
 *
 * OGGI NESSUN DESCRITTORE LA DICHIARA: `toolbox::Descriptor` ha `detect`,
 * `enumerate`, `version`, `config`, `note` — e basta. Questo tipo esiste perché
 * il pannello sappia già disegnare la scelta guidata il giorno che il campo
 * arriverà, non perché faccia finta che ci sia: finché l'elenco è vuoto il
 * pannello mostra il campo libero, e lo dice.
 *
 * Inventare qui un elenco di modelli o di flag plausibili sarebbe la cosa
 * peggiore possibile — sembrerebbe rilevato dalla macchina, e sarebbe scritto
 * a memoria da chi non ha guardato quella macchina.
 */
export interface OptionSpec {
  /** Il nome dell'opzione come la scrive lo strumento, es. `--model`. */
  key: string;
  /** Come si chiama per chi legge; in mancanza si mostra `key`. */
  label?: string;
  /** Che forma ha il valore. Una forma che non conosco si tratta come testo. */
  kind: "text" | "number" | "flag" | "choice";
  /** I valori ammessi, quando la forma è `choice`. */
  choices?: string[];
  /** Una riga di spiegazione per chi sceglie. */
  help?: string;
}

/**
 * Cosa sa la finestra degli strumenti installati. «Muto» non è «nessuno
 * strumento»: il primo è un motore che non ha risposto, il secondo una
 * macchina senza niente installato, e chi guarda deve poterli distinguere.
 */
export type ToolDiscovery =
  | { state: "asking" }
  | { state: "ready"; tools: Tool[] }
  | { state: "mute"; why: string };

// `mcp_server` È LA FAMIGLIA VERA, misurata eseguendo il rilevatore
// (`cargo run --example scan -p toolbox`): i descrittori spediti scrivono
// `"family": "mcp_server"`, e la voce `mcp` qui sotto — scritta a memoria — non
// avrebbe agganciato niente. Restano tutte e due, perché un descrittore altrui
// può usare l'una o l'altra e nessuna delle due deve far comparire una parola
// grezza in mezzo all'italiano.
const TOOL_KIND_LABEL: Record<string, string> = {
  ai_cli: "riga di comando IA",
  mcp: "server MCP",
  mcp_server: "server MCP",
  tool: "strumento",
};

/** L'etichetta di una specie; una specie nuova si mostra col nome che ha. */
export function toolKindLabel(kind: ToolKind): string {
  return TOOL_KIND_LABEL[kind] ?? kind;
}

/**
 * Legge la risposta del motore senza fidarsi della sua forma.
 *
 * `discover_tools` nasce in un altro cantiere mentre questo pannello si
 * scrive: se un giorno risponde con un campo in meno, la finestra deve
 * scartare quella voce e mostrare le altre, non spegnersi. Una voce scartata
 * qui è invisibile — è il prezzo di non avere una schermata bianca — e chi
 * chiama sa comunque quante ne ha ricevute.
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
      // Un campo mancante non promette che lo strumento c'è: si assume assente.
      available: record.available === true,
      reason: typeof record.reason === "string" ? record.reason : "",
      descriptor: typeof record.descriptor === "string" ? record.descriptor : "",
      models: parseStrings(record.models),
      options: parseOptionSpecs(record.options),
    });
  }
  return tools;
}

/** Le stringhe di un elenco, saltando quello che stringa non è. */
function parseStrings(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string" && item !== "");
}

/**
 * Le opzioni dichiarate, lette senza fidarsi della forma.
 *
 * Il campo non esiste ancora da nessuna parte: quando esisterà lo scriverà chi
 * aggiunge un descrittore — un utente, con un file JSON a mano — e una riga
 * sbagliata deve far sparire quella riga, non il pannello. Una forma che non
 * conosco diventa `text`, che è la sola scelta che non perde il valore scritto.
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

/** Gli strumenti in ordine di lettura: prima quelli utilizzabili, poi per nome. */
export function sortTools(tools: Tool[]): Tool[] {
  return [...tools].sort((left, right) => {
    if (left.available !== right.available) return left.available ? -1 : 1;
    return left.name.localeCompare(right.name);
  });
}

/** Gli strumenti raggruppati per specie, ciascun gruppo già ordinato. */
export function groupByKind(tools: Tool[]): Array<{ kind: ToolKind; tools: Tool[] }> {
  const groups = new Map<ToolKind, Tool[]>();
  for (const tool of sortTools(tools)) {
    const group = groups.get(tool.kind);
    if (group) group.push(tool);
    else groups.set(tool.kind, [tool]);
  }
  return Array.from(groups.entries()).map(([kind, list]) => ({ kind, tools: list }));
}

// ── i tre campi con cui un nodo usa uno strumento ───────────────────────
//
// Stanno nei parametri del passo (`with`), non in campi nuovi di `Step`: la
// forma del passo è ricalcata su `crates/flow` e aggiungerci chiavi dalla sola
// parte della finestra farebbe divergere le due verità. Nel file del flusso
// finisce l'IDENTIFICATIVO dello strumento, mai il suo percorso: `claude` è
// vero su qualunque macchina, `/Users/tizio/.local/bin/claude` solo su una.
// Chi esegue risolve l'identificativo con la stessa scoperta che riempie
// questo elenco.

export const TOOL_KEY = "tool";
export const MODEL_KEY = "model";
export const PROMPT_KEY = "prompt";
export const OPTIONS_KEY = "options";

const MANAGED_KEYS = [TOOL_KEY, MODEL_KEY, PROMPT_KEY, OPTIONS_KEY];

/**
 * Il valore di un'opzione. `true` è l'interruttore senza valore (`--verbose`);
 * `false` non si scrive affatto — un'opzione spenta si toglie, e lasciarla
 * scritta a `false` farebbe credere a chi legge il file che qualcuno l'abbia
 * disattivata di proposito.
 */
export type OptionValue = string | number | boolean;

export interface ToolChoice {
  /** L'identificativo dello strumento, come lo dichiara il motore. */
  tool: string;
  /** Testo libero: i modelli cambiano più in fretta di qualunque elenco. */
  model: string;
  prompt: string;
  /**
   * Le opzioni scelte, per nome. L'ordine di scrittura si conserva — in JS le
   * chiavi non numeriche escono nell'ordine in cui sono entrate — e chi legge
   * il file ritrova la riga di comando com'è stata composta.
   */
  options: Record<string, OptionValue>;
}

function textAt(params: Record<string, unknown> | null | undefined, key: string): string {
  const value = params?.[key];
  return typeof value === "string" ? value : "";
}

/** Separa i tre campi gestiti dal pannello dagli altri parametri del passo. */
export function splitToolParams(params: Record<string, unknown> | null | undefined): {
  choice: ToolChoice;
  rest: Record<string, unknown>;
} {
  const rest: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(params ?? {})) {
    if (!MANAGED_KEYS.includes(key)) rest[key] = value;
  }
  return {
    choice: {
      tool: textAt(params, TOOL_KEY),
      model: textAt(params, MODEL_KEY),
      prompt: textAt(params, PROMPT_KEY),
      options: readOptions(params?.[OPTIONS_KEY]),
    },
    rest,
  };
}

/**
 * Le opzioni scritte nel passo. Un valore che non è né testo, né numero, né
 * interruttore viene scartato invece che convertito: un oggetto trasformato in
 * `"[object Object]"` finirebbe sul disco al posto di quello che c'era.
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
 * Come si leggerebbe la riga di comando che ne esce. Serve al pannello per
 * mostrare a chi sceglie *cosa sta componendo*, e non è quello che finisce nel
 * file: sul disco restano le opzioni per nome, che si rileggono e si
 * ricompongono. È un'anteprima, e va letta come tale.
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
 * Rimette insieme i parametri. Un campo lasciato vuoto non finisce nel file
 * come stringa vuota: sul disco resta la differenza fra «non l'ho scelto» e
 * «l'ho scelto vuoto», e la prima è la verità.
 */
export function joinToolParams(
  rest: Record<string, unknown>,
  choice: ToolChoice,
): Record<string, unknown> | null {
  const params: Record<string, unknown> = { ...rest };
  if (choice.tool !== "") params[TOOL_KEY] = choice.tool;
  if (choice.model !== "") params[MODEL_KEY] = choice.model;
  if (choice.prompt !== "") params[PROMPT_KEY] = choice.prompt;
  // Un'opzione senza nome non si scrive: è una riga che chi sceglie sta ancora
  // componendo, e salvarla darebbe al motore una chiave vuota.
  const options: Record<string, OptionValue> = {};
  for (const [key, value] of Object.entries(choice.options)) {
    if (key.trim() !== "") options[key] = value;
  }
  if (Object.keys(options).length > 0) params[OPTIONS_KEY] = options;
  return Object.keys(params).length === 0 ? null : params;
}

/**
 * Vero se lo schema d'ingresso del passo rifiuterebbe i campi del pannello.
 *
 * Un passo può dichiarare un oggetto chiuso (`allow_extra: false`), e i flussi
 * veri sul disco lo fanno: scrivergli dentro una chiave che lo schema non
 * elenca produce un file che il motore rifiuta al caricamento. Il pannello lo
 * dice mentre lo si scrive, invece di lasciarlo scoprire a chi salva.
 */
export function schemaRejectsToolKeys(schema: ValueSchema, choice: ToolChoice): boolean {
  if (schema.type !== "object" || schema.allow_extra) return false;
  const written = joinToolParams({}, choice);
  if (written === null) return false;
  return Object.keys(written).some((key) => !(key in schema.properties));
}

/**
 * Il binario che il passo dichiara per conto suo, se ce n'è uno. Convive col
 * campo «strumento» solo per sbaglio: sono due risposte alla stessa domanda.
 */
export function rivalBinary(rest: Record<string, unknown>): string {
  const bin = rest.bin;
  return typeof bin === "string" ? bin : "";
}

/** L'identificativo dello strumento scelto da un passo, se ne ha uno. */
export function toolOf(params: Record<string, unknown> | null | undefined): string {
  return textAt(params, TOOL_KEY);
}

/**
 * I modelli da suggerire: quelli che il motore dichiara per lo strumento
 * scelto, più quelli già scritti negli altri passi. Il campo resta libero —
 * questi sono un aiuto a scrivere, non un elenco di ciò che è permesso.
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

// ── il registro condiviso: chi ha già chiesto, lo dice a tutti ────────────
//
// PERCHÉ ESISTE. Un nodo sulla tela deve mostrare il segno e il nome dello
// strumento che esegue, e sapere se su questa macchina c'è: sono dati della
// scoperta, non del passo. Passarli lungo la catena dei nodi vorrebbe dire
// riscrivere chi costruisce la disposizione e chi la monta — due file
// condivisi, uno dei quali in mano a un altro cantiere in questo momento.
//
// La scoperta resta una sola: chi la esegue (`engine.discoverTools`) deposita
// qui l'esito, e chiunque lo voglia lo legge. Nessuna seconda interrogazione
// del disco, nessun ordine di montaggio da rispettare — un nodo montato prima
// della risposta mostra quello che il passo dichiara, e si aggiorna da sé
// quando la risposta arriva.

let registry: ReadonlyMap<string, Tool> = new Map();
const listeners = new Set<() => void>();

/** Deposita l'esito di una scoperta e sveglia chi lo stava guardando. */
export function publishTools(tools: Tool[]): void {
  registry = new Map(tools.map((tool) => [tool.id, tool]));
  for (const listener of listeners) listener();
}

/**
 * Gli strumenti conosciuti, per identificativo.
 *
 * L'IDENTITÀ DELLA MAPPA CAMBIA SOLO CON UNA SCOPERTA NUOVA, e non è un
 * dettaglio: `useSyncExternalStore` confronta i riferimenti, e restituire una
 * mappa nuova a ogni lettura farebbe ridisegnare la tela all'infinito. È già
 * successo su questa tela il 28/08 per un `new Map()` scritto dentro un render.
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
 * Lo strumento con questo identificativo, se la scoperta lo ha trovato.
 *
 * `undefined` risponde a due domande diverse — la scoperta non ha ancora
 * risposto, oppure ha risposto e questo strumento qui non c'è — e chi chiama
 * deve distinguerle: `toolsAreKnown()` dice se una risposta è arrivata.
 */
export function useTool(id: string): Tool | undefined {
  return useSyncExternalStore(
    subscribeTools,
    () => (id === "" ? undefined : registry.get(id)),
    () => undefined,
  );
}

/** Vero quando una scoperta ha già risposto qualcosa. */
export function useToolsAreKnown(): boolean {
  return useSyncExternalStore(
    subscribeTools,
    () => registry.size > 0,
    () => false,
  );
}
