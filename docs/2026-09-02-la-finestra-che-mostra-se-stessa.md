# La finestra che mostra sé stessa

**02/09/2026.** Nasce da una frase di Theo — *«l'utente deve poter vedere tutto
quello che è Sailor, dalle cose che salva a come le gestisce»* — e da una
seconda che ne è il metodo: *«non tutto dovrebbe esistere in primo piano»*.

Non è un disegno nuovo. È il disegno che discende da cose già scritte in questo
repo, ritrovate invece che inventate.

## Il metro, che era già il vincolo permanente

> **Chiarezza per chi guarda.** Sailor esiste perché una persona veda e
> controlli cosa fanno i suoi strumenti. Vale anche per l'aspetto: **un'interfaccia
> che nasconde cosa succede è il contrario del prodotto.**
>
> — `docs/decisioni.md`, fra i vincoli permanenti

Il guasto 30 è la sua violazione più netta: la tela diceva «in attesa» su ogni
nodo di ogni flusso mentre il motore lavorava. **Non nascondeva: raccontava il
falso.** La lezione che ne resta è che una superficie che non sa una cosa deve
dirlo, non riempire il vuoto con un valore plausibile.

## La struttura viene dalle quattro superfici, non da un elenco di pagine

`docs/2026-08-31-le-quattro-superfici.md` dà al sistema quattro categorie, e
ogni azione registrata ne dichiara una:

| superficie | cosa fa | dove si vede oggi |
|---|---|---|
| `sense` | legge il mondo senza toccarlo | da nessuna parte |
| `act` | tocca il mondo | la lavagna, i terminali |
| `remember` | il deposito, **come fonte a cui si fanno domande** | quasi da nessuna parte |
| `gate` | chi può cosa, e dove entra una persona | da nessuna parte |

**Il vuoto più grande è `remember`**, e il documento lo dice già con le parole
di un mandato di agosto:

> *«Sailor registra tutto quello che succede e non torna mai a leggerlo.»*

Questa è la pietra miliare che manca. Non una schermata in più: **il deposito
che diventa interrogabile da chi guarda.**

## Undici voci diventano quattro

La finestra di oggi ha sette posti in fila, tutti dello stesso peso; la mia
prima anteprima ne aveva undici. Entrambe sbagliano allo stesso modo, e
`navigation-patterns` lo nomina: *«mixing navigation levels in the same visual
component»*. Ma il difetto vero è prima della grafica — **è che ogni capacità
del motore chiedeva la sua voce.**

Quattro voci, e ognuna è una domanda che una persona si fa davvero:

| voce | la domanda | cosa contiene |
|---|---|---|
| **Board** | «cosa faccio adesso» | i flussi, la tela, la cassetta dei passi |
| **Terminals** | «cosa sta girando» | i terminali vivi, gli alberi di lavoro |
| **Memory** | «cos'è successo, e quanto è costato» | corse, costi, guasti, quota — il deposito interrogabile |
| **Sailor** | «cosa sa di me, e cosa può fare» | profili, motori, modelli, la casa, la dotazione |

Le prime due sono `act`: dove si lavora. La terza è `remember`. La quarta è il
sistema che mostra sé stesso — ed è dove `sense` e `gate` troveranno posto
quando esisteranno.

## Cosa vuol dire «non in primo piano»

Tre gradi, e la regola per stare in ciascuno:

1. **Sempre visibile** — la barra: dove sei, cosa gira, cosa costa. Tre fatti,
   e nient'altro. Se una corsa è in atto lo devi sapere da qualunque posto.
2. **A una voce di distanza** — le quattro sezioni. Ognuna apre su ciò che
   quella domanda vuole, non su un menu di sotto-domande.
3. **Dentro la sezione** — tutto il resto. I modelli non sono un posto: sono una
   scheda dentro Sailor. La quota non è un posto: è una riga della barra che si
   apre in Memory.

Il metro per decidere il grado: **quante volte al giorno**. Un profilo si
cambia una volta a settimana e oggi occupa la stessa larghezza della lavagna.

## Cosa deve mostrare «Sailor», in concreto

È la voce che oggi non esiste in nessuna forma, ed è quella che risponde alla
frase da cui questo documento nasce.

- **Cosa salva**: i flussi e da quale delle tre sorgenti vengono (sistema, casa,
  progetto); il deposito delle corse; l'inventario della macchina; i guasti; le
  identità di firma. Con **il percorso vero su disco**, perché un dato di cui
  non sai dove sta è un dato che non controlli.
- **Come lo gestisce**: quanto occupa; da quanto tempo; cosa succede quando
  cresce. Il piano del ciclo di vita dello spazio esiste già ed è misurato:
  43 GB di scratchpad, 24 GB di cartelle di compilazione.
- **Cosa può fare**: le azioni registrate con la loro superficie e i poteri che
  pretendono — rete, disco, processi, denaro, segreti. È già il contratto che
  ogni azione dichiara; oggi nessuno lo può leggere.
- **Con cosa lo fa**: motori, profili, il loro stato di accesso, i modelli e i
  prezzi.

## La lingua

`decisioni.md`, decisione del 01/09/2026 presa da Theo: *«English everywhere,
restoring the charter the project was founded with»* — identificatori, commenti,
documentazione, **e ogni messaggio che un utente dello strumento può vedere**.

La prima anteprima era in italiano. Rifatta in inglese: non è una preferenza, è
una decisione già presa che non avevo letto.

## What this document became on the screen, the evening of 02/09/2026

Checked against the window built the same day, on `sorgenti` from `ea2f6bf7`
to `9768998b`, by two fresh-context judges whose findings were fixed in the
last two commits. Outcome first, then what the judges left open, then what is
still Theo's.

- **Four places, not eleven.** `Rail.tsx` draws Board / Terminals / Memory /
  Sailor in a column, grouped «work · what happened · itself», each with a
  sub-rail: Memory has runs / ledger / spend / faults, Sailor has keeps / can
  do / profiles / models / equipment / commands, Terminals has live / projects
  / worktrees. The column counts the flows and the open terminals.
- **The bar speaks from anywhere, and says when it cannot.** Breadcrumbs say
  where you are, down to the ledger table open; chips say what runs, what it
  costs today (a floor when a call had no price; it opens Memory › spend),
  whether the build under the window is old, and who the command lines run
  as. A poll the engine refuses is shown in red with the reason, never read
  as «nothing running». ⌘K opens a palette that reaches any place, any flow,
  and runs one.
- **The ledger is interrogable.** Memory › ledger is a SQL box over the real
  `state.db`, read-only by construction (`PRAGMA query_only`), every table
  listed with its count, every row openable.
- **What Sailor keeps, with the real paths.** Sailor › keeps lists the home,
  every store (flows by source, the ledger, the inventory, the faults, the
  profiles, the prices, the terminals) with where it is, how many, how big,
  whether it exists yet; the binary in service, its build time and commit.
- **The terminals are a grid.** Every open terminal on screen, the focused one
  first and large, each with its own close; a line under each goes to the
  router, and a line that names a flow starts it. The pane says the bytes
  moved and what they amount to in tokens, against the ceiling the relay flow
  declares, marked as the estimate it is.
- **A run can be stopped by hand** before its next step; the step at work
  finishes, and the console says so instead of pretending.
- **The window moves into a project** from Terminals › projects; the board's
  flows follow, the terminals keep the tree they were opened in.
- **A step handed to a person is taken and closed from the window**, under
  the waiting run, through the engine's own `step open` / `step close`; the
  run resumes through the window, so the console follows it and Stop
  applies, and the answer names the root it resumed in.
- **A node nobody ran says «not run yet»**, never «waiting»: fault 30 had
  come back through the default word.
- **The empty board says where it looked**, with the real paths.
- **A dark scheme**, one at-rule the contrast engine reads into, measured on
  every scene the light one is.
- **English on every screen.** The loose-line ratchet fell from 109 to 3, and
  the measurer now reads the JSX lines it used to skip.

### What the judges left open, ranked

1. The bar still carries the board's controls (source word, flow name, Save,
   Run, the Graph tab) beside the three facts; the spec says «three facts and
   nothing else».
2. Sailor › models is the spend screen again; a catalogue with «which is in
   use» distinct from the spend does not exist.
3. Sailor › can do lists families and names, not the surface (sense / act /
   remember / gate) nor the powers each action claims.
4. Sailor › keeps has no «since when», no «what happens when it grows», no
   signing identities.
5. The band on the board has no state, no cost per flow, no legend; the pane
   has no program+model header; the chip says «relay · at step» and not
   «relay 4 of 7».
6. The ledger keeps no root of a run's own: a run resumed after a move into
   another project resumes in the window's root, and says so.

### What is still Theo's: the gestures

1. Open the window and walk the four places; the bar must never go silent.
2. Memory › ledger: type `select * from runs order by 1 desc limit 5`.
3. Sailor › keeps: every path listed must exist where it says.
4. Terminals: open two, type `git status` under one, watch the line route.
5. Start the relay from the board, press Stop, read the console's last line.
6. Terminals › projects: «work here» on another project; the board changes.
7. Set the machine to dark; nothing must become unreadable.
