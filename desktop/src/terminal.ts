// La metà React del contratto del terminale: i sei comandi, i due eventi, e le
// tre decisioni che si possono provare senza il ponte.
//
// LA FONTE È `docs/2026-09-01-il-contratto-del-terminale.md`, non questo file.
// Il ponte Rust nasce in parallelo contro lo stesso documento: se le due metà
// divergono, a cambiare è il documento — e chi se ne accorge lo dice, invece di
// adeguare in silenzio la propria metà.
//
// PERCHÉ LE TRE DECISIONI STANNO QUI E NON NEI COMPONENTI. Dove va un tasto,
// come si legge un byte, e se un terminale è vivo sono le tre cose che questa
// metà può sbagliare da sola. Dentro un componente si proverebbero solo
// disegnando la finestra; qui sono funzioni pure, e restano provate anche
// mentre il ponte non esiste.

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type Unlisten = () => void;
type Listen = <T>(event: string, handler: (event: { payload: T }) => void) => Promise<Unlisten>;

interface TauriGlobal {
  core?: { invoke?: Invoke };
  event?: { listen?: Listen };
}

function tauri(): TauriGlobal | null {
  return (window as unknown as { __TAURI__?: TauriGlobal }).__TAURI__ ?? null;
}

/**
 * Come si chiama il guscio.
 *
 * **RICOPIATO DA `engine.ts` DI PROPOSITO, E VALE LA PENA DIRE PERCHÉ.** Il
 * contratto assegna a questo cantiere `Terminals.tsx`, `TerminalPane.tsx`,
 * `terminal.ts` e poche righe di `App.tsx`: `engine.ts` non è fra questi, e
 * aprire un accesso lì dentro per farlo importare qui vorrebbe dire toccare un
 * file di cui nessuno dei due cantieri ha dichiarato di rispondere, mentre
 * l'altra metà si scrive. Sono quattro righe senza decisioni dentro; il giorno
 * in cui `engine.ts` esporta la sua, questa sparisce.
 */
function invoker(): Invoke | null {
  return tauri()?.core?.invoke ?? null;
}

/**
 * Una riga dell'elenco dei terminali aperti.
 *
 * **RICALCA `terminal::Summary`, E LE DUE METÀ DICONO LA STESSA COSA.** Il
 * contratto dichiara camelCase e il tipo del crate lo porta
 * (`#[serde(rename_all = "camelCase")]` in `crates/terminal/src/session.rs`),
 * con la sua guardia e il suo mutante scritti dall'altro cantiere. Qui non si
 * legge nessuna seconda forma «per sicurezza»: un lettore indulgente farebbe
 * combaciare le due metà nascondendo il giorno in cui smettono di combaciare.
 * Se un campo arriva con un altro nome, questo tipo deve rompersi.
 *
 * L'attributo arriva in questo albero con la fusione: fino ad allora il
 * `session.rs` che si legge qui accanto è ancora quello di prima.
 */
export interface TerminalSummary {
  id: string;
  workspaceRoot: string;
  workspaceName: string;
  alive: boolean;
  processId: number;
}

/**
 * Dove è finita una riga confermata con Invio.
 *
 * Lo decide il motore (`terminal::Routed`), non la finestra: una seconda regola
 * di smistamento scritta qui divergerebbe dalla prima senza che nessuno se ne
 * accorga. `rule` è l'`id` della regola che ha riconosciuto la riga, e serve a
 * chi guarda per risalire alla riga di JSON che ha deciso.
 */
export type Submitted =
  | { kind: "command" }
  | { kind: "flow"; flow: string; text: string; rule: string };

/** Come si apre un terminale: dove, cosa avviare, quanto grande. */
export interface Opening {
  /** **La cartella si dichiara aprendo, e non dopo.** Vedi `crates/terminal/src/lib.rs`. */
  workspaceRoot: string;
  program?: string;
  args?: string[];
  cols: number;
  rows: number;
}

// ── i sei comandi ────────────────────────────────────────────────────────

/**
 * Apre un terminale dentro uno spazio di lavoro.
 *
 * **NON ESISTE UN TERMINALE GENERICO A CUI POI SI DICE DOVE ANDARE.** La
 * cartella è parte di cosa il terminale *è*: è la condizione perché lo
 * smistamento sappia di quale progetto si parla, e un terminale che scopre la
 * propria cartella dopo essere nato appartiene per un istante a un posto
 * sbagliato — l'istante in cui l'utente scrive la prima riga.
 */
export async function openTerminal(opening: Opening): Promise<TerminalSummary> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun motore che apra un terminale");
  return invoke<TerminalSummary>("terminal_open", { ...opening });
}

/**
 * La riga che l'utente ha confermato con Invio, guardata **prima** di essere
 * eseguita: può andare a un flusso invece che alla shell.
 *
 * Quando torna `{ kind: "command" }` il motore l'ha già scritta dentro lo
 * pseudo-terminale col ritorno a capo, come se fosse stata digitata. Quando
 * torna `{ kind: "flow" }` **non ha scritto niente**, e chi chiama decide cosa
 * farne.
 */
export async function submitLine(id: string, line: string): Promise<Submitted> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessuno a cui consegnare la riga");
  return invoke<Submitted>("terminal_submit", { id, line });
}

/**
 * Byte grezzi sull'ingresso, senza passare dallo smistamento.
 *
 * Riceve i byte e non una stringa perché è la finestra a sapere in che
 * codifica sta ciò che l'utente ha premuto: farli diventare testo qui e
 * ricodificarli là dentro è il punto in cui un accento si perde.
 */
export async function pressKeys(id: string, bytes: Uint8Array): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessuno a cui mandare i tasti");
  await invoke<null>("terminal_press", { id, bytes: encodeBytes(bytes) });
}

export async function resizeTerminal(id: string, cols: number, rows: number): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun terminale da ridimensionare");
  await invoke<null>("terminal_resize", { id, cols, rows });
}

export async function closeTerminal(id: string): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun terminale da chiudere");
  await invoke<null>("terminal_close", { id });
}

/**
 * I terminali aperti.
 *
 * **È L'UNICO ELENCO, E NON NE ESISTE UNA COPIA NELLA FINESTRA.** Un terminale
 * sopravvive a chi lo guarda: chiudere la finestra non uccide la sessione
 * dentro, e al riavvio è questo elenco a ritrovarla. Una lista tenuta nel
 * frontend direbbe «nessun terminale aperto» a una macchina che ne ha tre, e lo
 * direbbe con la stessa faccia di una macchina che non ne ha nessuno.
 */
export async function listTerminals(): Promise<TerminalSummary[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: l'elenco dei terminali lo tiene il motore");
  return invoke<TerminalSummary[]>("terminal_list");
}

// ── i byte, che non sono testo ───────────────────────────────────────────

/**
 * Byte → base64.
 *
 * **UN BYTE ALLA VOLTA, MAI ATTRAVERSO UNA STRINGA DI TESTO.** `btoa("à")`
 * risponde `4A==` — il byte 0xE0 di latin-1 — mentre la stessa lettera in UTF-8
 * è `0xC3 0xA0`, cioè `w6A=`. Un terminale che manda la prima forma scrive una
 * lettera sbagliata nella shell, e lo fa solo sulle parole accentate: la prova
 * che ci si accorgerebbe leggendo l'inglese è già stata smentita.
 */
export function encodeBytes(bytes: Uint8Array): string {
  let latin = "";
  for (const byte of bytes) latin += String.fromCharCode(byte);
  return btoa(latin);
}

/**
 * base64 → byte.
 *
 * **TORNA BYTE E NON TESTO, ED È TUTTA LA RAGIONE DEL BASE64 NEL CONTRATTO.**
 * Ciò che esce da uno pseudo-terminale può spezzarsi a metà di un carattere
 * multibyte: `è` arriva come `0xC3` in un evento e `0xA8` nel successivo.
 * Decodificarli in due stringhe darebbe due caratteri di rimpiazzo; consegnarli
 * come byte lascia rimettere insieme la lettera a chi sa farlo — l'emulatore.
 */
export function decodeBytes(base64: string): Uint8Array {
  const latin = atob(base64);
  const bytes = new Uint8Array(latin.length);
  for (let index = 0; index < latin.length; index += 1) bytes[index] = latin.charCodeAt(index);
  return bytes;
}

/** Il testo che l'utente ha premuto, nei byte che la shell si aspetta. */
export function keyBytes(data: string): Uint8Array {
  return new TextEncoder().encode(data);
}

// ── dove va un tasto ─────────────────────────────────────────────────────

/**
 * Che cosa la tastiera sta facendo, adesso.
 *
 * `compose` è il terminale a un prompt: la finestra tiene la riga, la disegna
 * lei, e a Invio la consegna allo smistamento. `raw` è il terminale dentro un
 * programma: ogni tasto passa così com'è, Invio compreso.
 */
export type KeyMode = "compose" | "raw";

/** Che cosa fare di un tasto. Una decisione, non un effetto: gli effetti li fa chi disegna. */
export type KeyAction =
  /** Alla shell, byte grezzi. */
  | { kind: "press"; bytes: Uint8Array }
  /** Allo smistamento, come riga intera. */
  | { kind: "submit"; line: string }
  /** Sullo schermo, disegnato dalla finestra: è l'eco della riga che sta componendo. */
  | { kind: "echo"; text: string }
  /** Da nessuna parte, e con scritto perché. */
  | { kind: "ignored"; why: string };

export interface Stroke {
  /** La riga in composizione dopo questo tasto. */
  draft: string;
  actions: KeyAction[];
}

/** Invio, in tutte e due le forme in cui un emulatore lo consegna. */
function isEnter(data: string): boolean {
  return data === "\r" || data === "\n" || data === "\r\n";
}

/** Cancella l'ultimo carattere: indietro, spazio, indietro. */
function eraseCells(count: number): string {
  return "\b \b".repeat(count);
}

/**
 * Tutto ciò che si può stampare, compreso un incollaggio di più caratteri. Un
 * carattere di controllo in mezzo lo esclude: si compone una riga, non si
 * pilota un programma.
 */
function isPrintable(data: string): boolean {
  if (data.length === 0) return false;
  for (const character of data) {
    const code = character.codePointAt(0) ?? 0;
    if (code < 0x20 || code === 0x7f) return false;
  }
  return true;
}

/**
 * **INVIO E TASTI SONO DUE STRADE DIVERSE, E QUESTA FUNZIONE È IL BIVIO.**
 *
 * Solo la riga che l'utente conferma con Invio esce come `submit`; ogni altra
 * cosa esce come `press`. Se ogni tasto passasse dallo smistamento, un editor
 * dentro il terminale diventerebbe inservibile: una freccia e un Ctrl-C
 * verrebbero esaminati da un elenco di regole che non li riguarda, e la
 * risposta arriverebbe dopo un giro di ponte.
 *
 * **PERCHÉ COMPORRE LA RIGA E NON SPECCHIARLA.** `terminal_submit` scrive lui
 * la riga dentro lo pseudo-terminale quando è un comando. Se la finestra avesse
 * già mandato quei caratteri alla shell, la riga finirebbe scritta due volte —
 * `lsls`. Quindi mentre si compone **non parte niente**: l'eco lo disegna la
 * finestra, e a Invio lo cancella prima di consegnare, così a scrivere la riga
 * sullo schermo è la shell, una volta sola.
 *
 * **IL PREZZO, DICHIARATO — ED È PERCHÉ `raw` È IL PREDEFINITO.** Mentre la
 * finestra tiene la riga, la `readline` della shell non ce l'ha: saltano la
 * cronologia, le frecce e soprattutto **il Tab**, che dopo le lettere è il
 * tasto più premuto di tutti. E non c'è niente che accorga la finestra che lì
 * dentro è appena partito un programma a schermo intero — `claude`, `vim`, un
 * `less` — cioè esattamente il caso per cui questo cantiere esiste. Un
 * terminale quindi **nasce terminale**: `compose` è la scelta esplicita di chi
 * vuole lo smistamento su quella riga, non lo stato in cui si apre.
 *
 * Dentro `compose`, un tasto che non sia un carattere, la cancellazione o Invio
 * **non parte** e dice perché, invece di partire e sfasare lo schermo. Con la
 * riga vuota non c'è niente da sfasare e il terminale torna a essere un
 * passaggio diretto. Ctrl-C è l'eccezione a tutto e passa sempre: un modo di
 * fermare ciò che gira non si toglie mai a nessuno.
 *
 * **LA STRADA CHE TOGLIE IL COMPROMESSO INVECE DI SCEGLIERLO** sono i
 * marcatori di prompt OSC 133, come li usano VS Code e Warp: la shell segna
 * dove comincia la riga, la finestra non tiene niente e non disegna nessun eco,
 * e all'Invio legge la riga dal buffer dell'emulatore. È a piano in
 * `docs/da-fare.md`, con la motivazione per esteso.
 */
export function keyStroke(mode: KeyMode, draft: string, data: string): Stroke {
  if (mode === "raw") {
    return { draft: "", actions: [{ kind: "press", bytes: keyBytes(data) }] };
  }

  // Ctrl-C prima di tutto: annulla la riga in composizione e arriva comunque a
  // chi sta girando.
  if (data === "\x03") {
    return {
      draft: "",
      actions: [
        { kind: "echo", text: eraseCells(draft.length) },
        { kind: "press", bytes: keyBytes(data) },
      ],
    };
  }

  if (isEnter(data)) {
    // Un Invio a vuoto è un a capo, non una riga da smistare: mandarlo allo
    // smistamento chiederebbe a un elenco di regole cosa fare del nulla.
    if (draft === "") return { draft: "", actions: [{ kind: "press", bytes: keyBytes("\r") }] };
    return {
      draft: "",
      actions: [
        { kind: "echo", text: eraseCells(draft.length) },
        { kind: "submit", line: draft },
      ],
    };
  }

  if (data === "\x7f" || data === "\b") {
    if (draft === "") return { draft, actions: [{ kind: "ignored", why: "non c'è niente da cancellare" }] };
    return { draft: draft.slice(0, -1), actions: [{ kind: "echo", text: eraseCells(1) }] };
  }

  // Ctrl-U: la riga si butta via, e nessuno la esegue.
  if (data === "\x15") {
    return { draft: "", actions: [{ kind: "echo", text: eraseCells(draft.length) }] };
  }

  if (isPrintable(data)) {
    return { draft: draft + data, actions: [{ kind: "echo", text: data }] };
  }

  if (draft === "") {
    return { draft, actions: [{ kind: "press", bytes: keyBytes(data) }] };
  }

  return {
    draft,
    actions: [
      {
        kind: "ignored",
        why: "mentre la finestra tiene la riga passano solo i caratteri, la cancellazione e Invio: svuotala per usare i tasti della shell",
      },
    ],
  };
}

// ── vivo, morto, o non lo so più ─────────────────────────────────────────

/**
 * Com'è messo un terminale, per chi guarda.
 *
 * **TRE STATI, NON DUE.** «Vivo» e «non lo so più» non sono la stessa cosa, ed
 * è il guasto 12 rifatto nella finestra: là un comando zittito dal perimetro
 * rispondeva «vuoto» senza errore, e la sorveglianza ha detto «nessun flusso in
 * esecuzione» mentre due giravano. Qui la forma è la stessa — se il canale
 * degli eventi non si è attaccato, una morte non arriverebbe mai, e un pannello
 * che continua a dire «vivo» sta affermando una cosa che non può sapere.
 */
export type Liveness =
  | { state: "alive" }
  | { state: "closed"; status: string | null }
  | { state: "unknown"; why: string };

/**
 * Lo stato da mostrare, dai due soli fatti che la finestra ha.
 *
 * L'ordine conta. **Un evento vince sull'elenco**: `terminal_closed` è arrivato
 * quando il processo è finito, mentre l'elenco è vecchio di quanto dista il
 * prossimo giro di domande — leggere prima l'elenco farebbe lampeggiare «vivo»
 * su un terminale già morto, per qualche secondo, ogni volta.
 */
export function livenessOf(
  summary: TerminalSummary,
  closed: ReadonlyMap<string, string>,
  watching: boolean,
): Liveness {
  const status = closed.get(summary.id);
  if (status !== undefined) return { state: "closed", status };
  // L'elenco lo dà per chiuso senza dire con quale esito: è un fatto più povero
  // dell'evento, e si scrive per quello che è invece di inventare uno stato.
  if (!summary.alive) return { state: "closed", status: null };
  if (!watching) {
    return {
      state: "unknown",
      why: "il canale degli eventi non c'è: se il processo finisse, questo pannello non lo saprebbe",
    };
  }
  return { state: "alive" };
}

/** La parola che accompagna la tinta: il divieto 5 non ammette che il colore stia da solo. */
export function livenessWord(liveness: Liveness): string {
  switch (liveness.state) {
    case "alive":
      return "vivo";
    case "closed":
      return "finito";
    case "unknown":
      return "non lo so più";
  }
}

// ── l'uscita, mentre esce ────────────────────────────────────────────────

/**
 * Chi disegna un terminale si iscrive qui; chi ascolta l'evento versa.
 *
 * **PERCHÉ NON UNO STATO DI REACT.** Ciò che esce da uno pseudo-terminale
 * arriva a pezzi e di continuo: un `setState` per pezzo ridisegnerebbe la
 * finestra a ogni riga di un `cargo build`. I byte vanno all'emulatore, che è
 * l'unico che sa cosa farne, e la finestra si ridisegna solo quando cambia
 * qualcosa che una persona legge.
 */
export class OutputBus {
  private readonly readers = new Map<string, (bytes: Uint8Array) => void>();

  /** Torna come si disdice. */
  subscribe(id: string, reader: (bytes: Uint8Array) => void): () => void {
    this.readers.set(id, reader);
    return () => {
      if (this.readers.get(id) === reader) this.readers.delete(id);
    };
  }

  /**
   * Consegna. **Torna falso se nessuno stava guardando**, e non è un dettaglio:
   * byte arrivati per un terminale che non ha un pannello sono byte persi, e
   * chi li perde in silenzio mostra uno schermo vuoto dove c'era un'uscita.
   */
  deliver(id: string, bytes: Uint8Array): boolean {
    const reader = this.readers.get(id);
    if (!reader) return false;
    reader(bytes);
    return true;
  }
}

interface Watchers {
  onOutput: (id: string, bytes: Uint8Array) => void;
  onClosed: (id: string, status: string) => void;
}

/**
 * Si mette in ascolto dei due eventi del contratto.
 *
 * **UN ASCOLTO CHE NON SI ATTACCA DEVE DIRLO**, come `listenToRuns`: tornare
 * `null` in silenzio è ciò che il 28/08/2026 è costato una vista che mostrava
 * «in corso» su una corsa finita. Qui costerebbe di più — un terminale morto
 * disegnato vivo — e per questo il motivo torna a chi chiama, che lo mette
 * dentro `livenessOf`.
 *
 * **O TUTTI E DUE O NESSUNO.** Se il secondo ascolto non si attacca, il primo
 * si disdice: metà canale vuol dire uscita che scorre su un pannello che non
 * saprà mai di essere morto, che è peggio di nessun canale — perché sembra che
 * funzioni.
 */
export async function watchTerminals(
  watchers: Watchers,
): Promise<{ stop: () => void } | { why: string }> {
  const shell = tauri();
  if (!shell) return { why: "fuori dal guscio nativo: nessun canale di eventi" };
  const listen = shell.event?.listen;
  if (!listen) {
    return { why: "il guscio non espone «event.listen»: un terminale morto resterebbe disegnato vivo" };
  }
  try {
    const stopOutput = await listen<{ id: string; bytes: string }>("terminal_output", (event) => {
      watchers.onOutput(event.payload.id, decodeBytes(event.payload.bytes));
    });
    try {
      const stopClosed = await listen<{ id: string; status: string }>("terminal_closed", (event) => {
        watchers.onClosed(event.payload.id, event.payload.status);
      });
      return {
        stop: () => {
          stopOutput();
          stopClosed();
        },
      };
    } catch (error) {
      stopOutput();
      return { why: String(error) };
    }
  } catch (error) {
    return { why: String(error) };
  }
}
