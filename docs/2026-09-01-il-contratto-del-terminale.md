# Il contratto del terminale nella finestra

**01/09/2026.** Scritto prima del lavoro, non dopo, perché due cantieri lo
costruiscono in parallelo — il ponte (Rust, dentro `desktop/src-tauri`) e la
finestra (React, dentro `desktop/src`) — e senza un contratto scritto si
incontrerebbero solo in fusione. Il guasto 31 è nato esattamente così: nove
agenti sullo stesso repo, quattro conflitti che `git` non vedeva perché erano
d'intenzione, non di righe.

**Questo file è la fonte per tutti e due.** Chi cambia il contratto lo cambia
qui e lo dice; chi lo scopre diverso dal codice apre un guasto invece di
adeguare in silenzio la propria metà.

## Perché esiste il cantiere

`crates/terminal` è finito e provato — 2.295 righe: pseudo-terminale vero,
scrittura, uscita mentre esce, ridimensionamento, chiusura, elenco degli aperti,
e lo smistamento di ciò che l'utente scrive come dati (`routing`). **Nessun
binario lo spedisce**: l'elenco dei comandi che la finestra espone
(`desktop/src-tauri/src/main.rs`) non ne ha nemmeno uno che riguardi un
terminale.

Senza terminale nella finestra, dentro Sailor non si tiene aperta una sessione
di un motore: è il pezzo che decide se Sailor può sostituire l'ambiente in cui
Theo lavora oggi.

## I nomi, e perché sono in inglese

Sono identificatori — li legge il compilatore e li scrive `invoke(...)` — quindi
inglese, come dice `AGENTS.md`. Ciò che legge una persona resta italiano.

## I comandi che il ponte espone

Sei, e nessuno di più senza aggiornare questo file.

| comando | argomenti | risposta |
|---|---|---|
| `terminal_open` | `{ workspaceRoot: string, program?: string, args?: string[], cols: number, rows: number }` | `{ id, workspaceRoot, workspaceName, alive, processId }` |
| `terminal_submit` | `{ id: string, line: string }` | `{ kind: "command" } \| { kind: "flow", flow: string, text: string, rule: string }` |
| `terminal_press` | `{ id: string, bytes: string }` (base64) | `null` |
| `terminal_resize` | `{ id: string, cols: number, rows: number }` | `null` |
| `terminal_close` | `{ id: string }` | `null` |
| `terminal_list` | — | `[{ id, workspaceRoot, workspaceName, alive, processId }]` |

La riga dell'elenco è `terminal::Summary`, che è già `Serialize`: **non si
ricopia in TypeScript un tipo che il Rust già dichiara** — è il guasto 10, che
in questo repo si è già ripresentato cinque volte.

Ogni comando torna `Result<_, String>`: l'errore è il testo che `PtyError`
produce, e va mostrato a chi guarda invece di finire in un `console.error`.

### Perché `submit` e `press` sono due comandi e non uno

Il motore li distingue già, e la distinzione è il senso di tutto il crate:
`submit` guarda una riga intera **prima** di eseguirla e può mandarla a un
flusso invece che alla shell; `press` passa byte grezzi — un Ctrl-C, una
freccia, la risposta a una domanda interattiva — senza farli esaminare da un
elenco di regole che non li riguarda.

La finestra manda a `submit` solo ciò che l'utente conferma con Invio a inizio
riga; tutto il resto è `press`. Un emulatore che mandasse tutto a `submit`
farebbe passare ogni tasto per lo smistamento, e un editor dentro il terminale
diventerebbe inservibile.

## L'evento dell'uscita

Nome: **`terminal_output`**. Payload: `{ id: string, bytes: string }`, dove
`bytes` è base64.

**Base64 e non una stringa** perché ciò che esce da uno pseudo-terminale è una
sequenza di byte che può spezzarsi a metà di un carattere multibyte: consegnarla
come stringa la corromperebbe, e l'accento sparito si vedrebbe solo su una
parola italiana in mezzo a un'uscita lunga.

Un evento **`terminal_closed`** con `{ id, status }` dice che il processo dentro
è finito: senza, la finestra mostrerebbe come vivo un terminale morto — che è la
forma in cui il guasto 12 si ripresenta ogni volta.

## Chi possiede quali file

Perché due cantieri paralleli non si tocchino:

- **il ponte**: `desktop/src-tauri/src/terminal.rs` (nuovo), `crates/terminal/**`,
  `crates/supervisor/**`, e **una sola riga** in
  `desktop/src-tauri/src/main.rs` — le sei voci dentro `generate_handler!`;
- **la finestra**: `desktop/src/Terminals.tsx`, `desktop/src/TerminalPane.tsx`,
  `desktop/src/terminal.ts` e le loro prove (nuovi), `desktop/src/styles.css`, e
  **poche righe** in `desktop/src/App.tsx` per la voce di navigazione.

Chi ha bisogno di toccare un file dell'altro si ferma e lo dice a chi coordina.

## Cosa manca nel crate, e chi lo aggiunge

`Terminal` non espone il proprio lettore: `Pty::reader()` esiste ma è
raggiungibile solo da dentro. **Il ponte aggiunge il modo di leggere l'uscita**
— nel crate, non nella finestra, perché un motore che si può provare solo
aprendo la finestra è un motore che nessuno prova (`crates/terminal/src/lib.rs`
lo dice come vincolo del crate).

## Le due proprietà che il cantiere deve avere, e come si vedono rosse

1. **Un terminale sopravvive alla finestra.** Chi chiude la finestra non uccide
   la sessione dentro: al riavvio, `terminal_list` la ritrova e la finestra si
   riaggancia. Il registro dei processi esiste già ed è dove va scritta
   (`supervisor::child::Process::start` è l'unica strada che registra: il guasto
   4 è chiuso proprio perché nessuno la aggira).
2. **Ciò che esce arriva mentre esce, non alla fine.** La prova che lo dice
   rossa: un comando che stampa, aspetta, stampa ancora — e l'asserzione che il
   primo pezzo è arrivato **prima** che il secondo esista.
