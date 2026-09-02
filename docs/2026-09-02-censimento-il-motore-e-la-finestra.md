# Il motore e la finestra: censimento del 02/09/2026

Tre cose osservate da Theo, misurate una per una:

1. molto del backend non è presente nel frontend;
2. l'organizzazione del software e del prodotto non rispecchia quanto si sta sviluppando;
3. mancano terminali e interfaccia.

Il metodo: ogni misura è preceduta da un caso di controllo che DEVE risultare
positivo. Tre strumenti usa-e-getta sono risultati ciechi durante questo
censimento e i loro numeri sono stati buttati — sono elencati in fondo, perché
la cecità è a sua volta un risultato.

---

## 1. Il backend nel frontend

### I comandi: il divario è quasi zero

Il guscio espone **26** comandi. La finestra ne chiama **24**.

Mai chiamati:

| comando | cosa fa | perché non è chiamato |
|---|---|---|
| `flow_places` | dove stanno i flussi sul disco | nessuna superficie li mostra |
| `live_status` | lo stato vivo di una corsa | la finestra sonda invece di ascoltare |

**Questa non è la lacuna.** Il ponte fra Rust e la finestra è quasi completo.
La lacuna sta un piano sopra.

### La CLI: quattordici comandi, tre hanno una superficie

`sailor --help` elenca quattordici comandi. Nella finestra:

| comando | cosa fa | superficie |
|---|---|---|
| `flow` | elenca, verifica, esegue i flussi | **sì** — il posto FLUSSI |
| `terminal` | apre una riga di comando che Sailor possiede | **sì** — il posto TERMINALI (guscio) |
| `worktree` | gli alberi in cui il repo è estratto | **sì** — il posto WORKTREES (guscio) |
| `inventory` | competenze, agenti, comandi, regole, ganci | parziale — INSTALLATO |
| `session` | chi entra, cosa succede, cosa c'è sulla macchina | parziale — ADESSO |
| `profiles` | i profili di una riga di comando | **no** |
| `models` | il listino, quale modello è in uso | **no** |
| `remaining` | quanta quota è stata consumata | **no** |
| `run` | avvia una riga di comando sotto il suo profilo | **no** |
| `step` | prende in carico e chiude un passo | **no** |
| `faults` | i guasti incontrati costruendo | **no** |
| `release` | mette in servizio un binario da HEAD | **no** |
| `workspace` | dichiara la radice del progetto | **no** |
| `version` | la versione del binario | **no** |

**Otto comandi su quattordici non hanno alcuna superficie.** Fra questi ci sono
proprio quelli che un utente userebbe ogni giorno: quale modello sto usando
(`models`), quanta quota mi resta (`remaining`), sotto quale profilo gira
(`profiles`), cos'è andato storto (`faults`).

### I flussi: 22 nel motore, 2 nella finestra

    la CLI vede:      22 flussi
    la finestra vede:  2 flussi — «relay» e «prima-corsa»

I due della finestra vengono da **`desktop/src/sample.ts`**: sono dati finti. La
finestra lo dichiara con un contrassegno arancione in alto a destra che dice
`sample data`, ma non dice che i veri sono ventidue e nessuno di loro è lì.

### Correzione, misurata dopo: questo NON è un difetto della finestra

Letto il codice, `App.tsx` fa la cosa giusta e la dichiara:

    // THE CANVAS STARTS AS THE DISK, NOT AS A SAMPLE. Inside the shell it
    // starts empty and waits for the engine [...] Outside the shell there is
    // no engine and the sample is all there is.

Dentro il guscio nativo la finestra chiede al motore e mostra i flussi veri; il
campione esiste **solo** nel browser, dove non c'è motore da interrogare. E
`flow_sources()` legge già da tre posti in ordine — sistema, casa, progetto —
con il commento che ricorda il guasto di allora: «la finestra mostrava "nessun
flusso" mentre la riga di comando ne eseguiva quattro».

**Il difetto era mio, non del codice.** Ma la conseguenza resta, e cambia solo
di posto: chi sviluppa vede i due finti perché **guarda il browser**, e guarda
il browser perché **l'app non è costruita** — nessun `.app`, nessun bundle,
niente in Applicazioni. Solo un binario di debug da lanciare a mano.

La distanza fra il motore e ciò che si vede non sta nel codice della finestra:
sta nel fatto che **la finestra vera nessuno la apre.**

---

## 2. L'organizzazione

### Due navigatori sovrapposti

La finestra ha **due** barre di navigazione, una sopra l'altra:

    y=13   [Graph]                          ← schede, dentro il posto FLUSSI
    y=52   ADESSO TERMINALI WORKTREES STORIA        FLUSSI² INSTALLATO COMANDI

La seconda ha sette voci divise in due gruppi senza che nulla dica perché — a
sinistra quattro, a destra tre. La prima ne ha una sola, `Graph`, che non
avendo compagne non è una scelta ma un'etichetta.

### Il messaggio parla di una colonna che in quel posto non c'è

In alto, sempre, in ogni posto: **«no flow in focus — pick one in the rail»**.

La colonna esiste **solo dentro il posto FLUSSI**. Negli altri sei il testo
indica un posto che il lettore non può vedere.

### Sei posti su sette sono gusci

Aperti nel browser (`npm run dev`), che è come si sviluppa:

| posto | cosa mostra |
|---|---|
| ADESSO | «Non riesco a chiedere cosa sta girando: fuori dal guscio» |
| TERMINALI | «Non riesco a chiedere quali terminali sono aperti: fuori dal guscio» |
| WORKTREES | «outside the desktop shell there is no repository to read» |
| STORIA | idem |
| INSTALLATO | idem |
| COMANDI | idem |
| **FLUSSI** | **l'unico con una superficie vera** |

I comandi Tauri non esistono fuori dall'app, e questo è corretto. Ma vuol dire
che **chi sviluppa nel browser vede sei schede vuote su sette**, e che tutto il
lavoro fatto su quelle superfici non è mai stato guardato mentre lo si faceva.

### La lingua è a metà strada

Nella stessa riga di intestazione: `Graph`, `sample data`, `Save`, `Run`
accanto a `ADESSO`, `TERMINALI`, `STORIA`, `INSTALLATO`, `COMANDI`. Nel corpo,
`CHECK` e `WENT` sui nodi accanto a «FLUSSI REGISTRATI», «Tutti i flussi»,
«Nuovo flusso», «Scegli un flusso nella colonna per aggiungere passi».

---

## 3. Terminali e interfaccia

**I terminali ci sono nel motore**: `crates/terminal` (3.557 righe), sei comandi
esposti (`terminal_open`, `_close`, `_list`, `_press`, `_resize`, `_submit`),
tutti e sei chiamati dalla finestra, `TerminalPane.tsx` e `Terminals.tsx`
scritti.

**Ma non si vedono mai insieme al grafo.** Sono un posto alternativo: o guardi i
flussi, o guardi i terminali. Nel disegno approvato la colonna teneva FLUSSI,
TERMINALI e CASSETTA insieme, e il terminale viveva accanto al lavoro.

Lo stesso vale per i worktree: `crates/workspace` esiste, `worktree.ts` e
`Worktrees.tsx` esistono, i tre comandi sono esposti e chiamati. **Il pezzo
mancante non è il codice: è che vivono in una scheda separata invece che
accanto al flusso che gira in quell'albero.**

---

## 4. I 55 worktree — per la sessione che li unisce

    55 worktree registrati
    50 rami già fusi in `sorgenti` → il worktree si può rimuovere
     2 rami NON fusi
     2 worktree in stato «detached» (/private/tmp/claude/cens, giudizio-finestra)
     0 file non commessi in nessuno di essi

I due non fusi:

| ramo | avanti | indietro | file | ultimo commit |
|---|---|---|---|---|
| `work/fusione` | 15 | 230 | 64 | 31/08 — «il guasto 31: nove agenti, quattro conflitti che git non vede» |
| `work/nodo-porte` | 1 | 123 | 12 | 01/09 — «fonde barra e tronco nel nodo» |

`work/nodo-porte` tocca il nodo, che è stato riscritto in `sorgenti` il 01-02/09:
va confrontato per **contenuto**, non per antenato, perché uno squash rompe
`is-ancestor`. `work/fusione` è indietro di 230 commit: i suoi 15 vanno letti
uno per uno prima di decidere.

Nessun lavoro va perso rimuovendo i 50 fusi, e la rimozione va fatta con
`git worktree remove`, non cancellando la cartella.

**Correzione del 02/09, da una sessione pari che l'ha verificato:**
`git worktree remove` **non** cancella il ramo — toglie la cartella, il
riferimento resta. La convinzione contraria viene dal comando `worktree rm` di
**Orca**, che è un'altra cosa: confondere i due è il motivo per cui nessuno
chiudeva la propria copia. Chiudere costa zero.

Quella stessa sessione ne ha già rimosse **52** (36 GB), misurando 50 su 53 già
nel tronco byte per byte. Resta una seconda lezione sua: **non aprire un ramo
per fondere**. Undici delle 53 esistevano solo per integrare, e il tronco porta
47 fusioni per una quarantina di rami di lavoro — ogni sessione rifaceva da capo
gli stessi conflitti. È lì che sono andati i token, non nel codice.

---

## 5. Cosa farei, in ordine

1. ~~La finestra mostri i 22 flussi veri~~ — **cancellato: la finestra già lo
   fa.** Al suo posto: **costruire il bundle dell'app**, perché fino a che
   Sailor non si apre come un'applicazione, tutto ciò che si sviluppa lo si
   giudica dal browser, dove sei posti su sette sono vuoti per costruzione.
2. **Un posto solo, non sette.** Colonna a sinistra con FLUSSI · TERMINALI ·
   CASSETTA come nel disegno; il grafo al centro; il terminale accanto al
   lavoro, non al posto del lavoro.
3. **Le quattro cose che mancano del tutto e servono ogni giorno**: quale
   modello, quanta quota, quale profilo, quali guasti. Sono quattro comandi CLI
   già scritti e collaudati: manca solo dove metterli.
4. **La lingua, in una passata sola.**
5. **I 50 worktree fusi**, che sono rumore in `git worktree list` e rendono
   difficile vedere i 2 che contano.

---

## Appendice: i tre strumenti risultati ciechi

Vale la pena scriverli, perché tutti e tre sembravano funzionare.

1. **Il grep degli `invoke("...")`** ha dato 21 comandi su 26 e sembrava
   mostrare un divario di cinque. Ma `src/worktree.ts` usa un wrapper `ask()`:
   i tre comandi worktree c'erano. Il divario vero è due.
2. **Il grep delle azioni registrate** ha risposto `graph.rs`, `2.0`,
   `worker.pid` — cioè qualunque stringa minuscola. Buttato: il conteggio delle
   azioni in questo censimento viene dal binario.
3. **La misura dell'impronta di ogni posto** tagliava a 400 caratteri e
   prendeva l'intestazione, che è fissa: dava sette posti identici e mi ha
   fatto concludere che la navigazione non funzionasse. Gli screenshot hanno
   mostrato che funziona benissimo. **La prova che non mente è l'immagine.**


---

## Appendice II: perché grep, e cosa dice invece SocratiCode

Theo ha chiesto perché ho usato grep. La risposta onesta è che grep era la
scelta sbagliata — nello stesso censimento mi ha reso cieco tre volte. Ma
misurando SocratiCode su questo repo sono emerse due cose che vanno sapute.

**L'indice era vecchio di un giorno e mezzo: 918 chunk.** Aggiornato,
**2.937** — 262 file su 302 erano cambiati. Con l'indice vecchio, `App.tsx`
non usciva in nessuna ricerca: il file principale della finestra era invisibile.
Aggiornato, esce con punteggio 0,83 sul commento esatto che cercavo. Il file
watcher risultava «active» e non aveva aggiornato niente.

**Il grafo non ha la grammatica di TypeScript.** Le quindici caricate sono
bash, c, cpp, csharp, dart, go, java, kotlin, lua, php, python, ruby, rust,
scala, swift. Manca il linguaggio in cui è scritta tutta la finestra. Perciò
`codebase_impact` e `codebase_symbol` rispondono **«nessuno dipende da questo»**
su file che hanno chiamanti: `useAsk` risulta con zero chiamanti quando ne ha
almeno quattro, `sample.ts` con zero dipendenti quando `App.tsx` lo importa.
Non è un difetto dell'indice semantico — è il grafo, e `Unresolved: 77%` lo
dichiara.

**Quindi, su questo repo:**

| domanda | strumento |
|---|---|
| «dov'è la cosa che fa X» | `codebase_search` — funziona bene anche su TS |
| «chi chiama questo» in Rust | il grafo, ma dopo `codebase_graph_build` |
| «chi chiama questo» in TS/TSX | **il grafo mente**: serve `cargo`/`tsc`, o una prova |
| «questo nome esiste» | grep, e solo dopo un caso di controllo |

È lo stesso difetto dei falsi orfani di Rust dentro le macro, in un altro
vestito: uno strumento che risponde «zero» quando la risposta giusta è «non so
leggere questo linguaggio».

---

## Appendice III: cosa è stato fatto durante il censimento

**Una riparazione.** La barra del programma è disegnata in tutti e sette i
posti e, senza un flusso in fuoco, dice «no flow in focus — pick one in the
rail». La colonna esiste solo sulla lavagna: in sei posti su sette quella riga
mandava chi legge in un posto che non c'era. Adesso la riga compare solo dove
la colonna esiste.

La prova è stata scritta **prima**, e vista rossa. Al primo colpo era rossa per
la ragione sbagliata — chiedeva `.react-flow`, che è montato sempre e solo
nascosto — e questo è un modo di fallire che assolve: una prova rossa per un
motivo qualunque sembra una prova che funziona. Corretta in `.body[hidden]`, è
tornata rossa sul difetto vero. Porta dentro il proprio controllo: la stessa
riga deve **esserci** sulla lavagna, perché un'assenza passa altrettanto bene
quando il selettore è sbagliato.

**I tre cricchetti sono usciti rossi, tutti e tre per colpa di questo lavoro**,
di uno o due:

    blocchi oltre 6 righe:   522 (tetto 521)  — il commento della prova
    commenti con una data:   202 (tetto 201)  — «02/09/2026» in quel commento
    righe non inglesi:      8078 (tetto 8076) — «Adesso» e «Flussi» in prosa inglese

Le ultime due sono la stessa lezione già scritta: un termine italiano dentro una
frase inglese la fa contare come italiana, e la cura è **spostare il termine,
mai la regola**. Nessun seme è stato alzato: i tre testi sono stati riscritti
finché i numeri non sono tornati sotto.

**Il bundle.** `cargo tauri build` produce `Sailor.app` (10 MB) in
`desktop/src-tauri/target/release/bundle/macos/`. Non esisteva: è il motivo per
cui tutto il lavoro sulla finestra veniva giudicato dal browser, dove sei posti
su sette sono vuoti per costruzione. Non è installata in `/Applications` —
quella è una scelta di Theo, non mia.

Stato: 247 prove della finestra verdi, `tsc --noEmit` pulito, cinque cricchetti
verdi. Niente è stato commesso.
