# Dove va il tempo, misurato sulle sessioni

**05/09/2026.** Theo ha chiesto un rapporto sulle sessioni passate per capire
dove si perde tempo e come migliorare i processi con Sailor e i flussi. Questo
è il rapporto, con i numeri accanto e il modo in cui sono stati presi. Ogni
proposta in fondo è legata a un numero qui sopra: una proposta senza numero è
un'opinione.

## Da dove vengono i numeri

Tre sorgenti, tutte su questa macchina:

- le **219 trascrizioni** delle sessioni di Claude Code su questo repository
  (`~/.claude/projects/-Users-theo-personal-sailor/*.jsonl`, 189 MB, dal
  26/08 al 05/09), lette da uno script che conta turni, chiamate a strumenti,
  durata e prima richiesta;
- il **deposito di Sailor** (136 corse, 502 passi, 30 chiamate a motore);
- la trascrizione di **questa sessione** (`bb77a609`, 56 ore, 136 turni di
  Theo, 6.303 chiamate a strumenti), classificando ogni comando di shell.

## Cosa sono le 219 sessioni

| specie | sessioni | minuti | chiamate a strumenti |
|---|---:|---:|---:|
| revisione di sicurezza a ogni commit (plugin `claude-security`) | 155 | 169 | 978 |
| nodi di flusso, giudici, pianificatori accesi da un mandato | 41 | 144 | 657 |
| esecuzione di un piano scritto («Esegui questo piano») | 6 | 100 | 373 |
| motore in un passo di flusso (0 strumenti) | 2 | 1 | 0 |
| **sessioni con una persona dentro** | **15** | 4.687 | 7.252 |

**204 sessioni su 219 hanno al più un turno umano.** Non sono sessioni di
lavoro: sono processi. E il più numeroso non è di Sailor: **155 revisioni di
sicurezza**, una a ogni commit, tre ore di agente in dieci giorni, il cui esito
nessuno ha mai letto in questo repository — nessun commit lo cita, nessun
guasto ne viene. È lavoro che si paga e non si raccoglie.

Le 15 sessioni umane portano tutto il resto: **4.687 minuti su 5.101**. Di
queste, una sola — questa — ne fa 3.360.

## Dove va il tempo di una sessione lunga

4.754 comandi di shell in 56 ore, classificati dal testo del comando:

| cosa | comandi | quota |
|---|---:|---:|
| leggere il codice con `grep`, `sed -n`, `cat`, `head` | 1.580 | 33 % |
| altro (misure ad hoc, `ps`, `lsof`, `sqlite3`, `ls`) | 1.065 | 22 % |
| `cargo test` | 941 | 20 % |
| modificare file con `python3` | 520 | 11 % |
| `git` | 223 | 5 % |
| attese (`until … sleep`, `sleep`) | 142 | 3 % |
| usare Sailor (`sailor …`) | 139 | 3 % |
| `cargo build` | 103 | 2 % |
| `sailor release` | 41 | 1 % |

Tre letture.

**Un terzo dei comandi è leggere codice a mano.** Mille e cinquecento
`grep`/`sed` sono il lavoro che un indice farebbe in una chiamata: sapere chi
chiama una funzione, dove vive una costante, quale prova copre un file.
SocratiCode è configurato ma **in questa sessione non si è mai connesso** (il
server MCP ha rifiutato la connessione dopo un aggiornamento; solo chi è al
terminale può ricollegarlo con `/mcp`). Il costo di quel buco è misurabile: è
questa riga della tabella.

**Un quinto è `cargo test`, e non sono 941 esecuzioni diverse.** Sono per lo
più lo stesso giudice rilanciato per rimisurare un cricchetto dopo aver
accorciato un commento, oppure la suite intera rilanciata perché l'esecuzione
precedente aveva misurato l'albero di lavoro sporco di un'altra sessione. Il
rito «archivio pulito di HEAD, sovrapponi i miei file, tocca il file del
giudice, misura» è stato eseguito a mano più di venti volte in una notte.
Ogni giro sono trenta secondi buoni e due o tre minuti quando la suite intera
gira in parallelo a un rilascio.

**I rilasci rigirano tutta la suite da capo.** 41 rilasci, ognuno clona HEAD,
costruisce e gira la suite intera (5–8 minuti, di più sotto carico). Tre volte
in una notte la suite era **già passata** sullo stesso albero pochi minuti
prima, in un altro processo, e il rilascio non lo sapeva.

## Cosa dice il deposito dei flussi

- **39 flussi esistono, 32 hanno girato almeno una volta.** 136 corse.
- **`sviluppa-sailor`**: 6 fallite, 3 complete, venti minuti a corsa, 13 unità
  di costo su 30 chiamate a motore totali. È il flusso che dovrebbe far
  lavorare Sailor su Sailor, e fallisce due volte su tre.
- **`prova-della-vista`**: 21 fallite su 21. Un flusso che fallisce sempre e
  viene rilanciato ventuno volte è un guasto che nessuno ha scritto.
- **Passi rotti, per classe**: `check_failed` 30, `invalid_input` 11,
  `engine_exit_error` 6, `answer_not_json` 4. La prima classe è la metà: sono
  i controlli dichiarati dal flusso stesso che respingono la risposta del
  motore. Cioè il flusso sa cosa vuole, il motore non lo dà, e la corsa muore.
- **30 chiamate a motore in dieci giorni.** Tutto il resto del lavoro — le
  6.303 chiamate a strumenti di questa sessione — è passato **fuori** dai
  flussi, in una sessione di Claude Code con una persona che scrive mandati a
  mano. Sailor sviluppa Sailor solo nel nome del flusso.

## Il ciclo con il remoto

Al 05/09: 64 rami locali, 56 con albero identico al tronco; cinque copie di
lavoro aperte da giorni, pulite e già fuse; il tronco 196 commit avanti a
`origin/sorgenti`, e `origin/main` fermo alla storia precedente alla
riscrittura. `AGENTS.md` misurava già il 02/09 «53 copie abbandonate, 39 GB, 47
fusioni per una quarantina di rami». La regola c'era; il gesto no. Guasto 85.

## Dove si perde, in una frase per voce

1. **Leggere a mano quello che un indice sa** — un terzo dei comandi.
2. **Rimisurare i cricchetti a mano** dopo ogni commento tagliato — decine di
   giri da trenta secondi, e i seed sbagliati quando la misura è presa
   sull'albero sporco.
3. **Rigirare la suite intera al rilascio** anche quando è appena passata.
4. **Pagare 155 revisioni di sicurezza** che nessuno legge.
5. **Flussi che falliscono in silenzio** (`prova-della-vista` 21/21) o due
   volte su tre (`sviluppa-sailor`) senza che nessun guasto ne nasca.
6. **Il lavoro vero passa fuori dai flussi**: 30 chiamate a motore da Sailor,
   migliaia da una sessione a mano.
7. **Rami e copie che nessuno chiude**, e un tronco che nessuno spinge.

## Cosa si fa, e con quale numero si misura

Ogni voce nomina il numero che deve scendere.

1. **`sailor ratchet`**: un comando che rifà il rito — archivio pulito di HEAD,
   sovrapposizione dei soli file modificati dall'albero corrente, misura di
   tutti i cricchetti — e **stampa i seed da scrivere**, o li scrive con
   `--write`. *Scende*: i 941 `cargo test`, e i seed presi sull'albero sporco
   (due volte il 04/09).
2. **Il rilascio ricorda la suite che ha passato**: dopo una suite verde su un
   albero, il rilascio scrive l'hash dell'albero accanto al timbro; un rilascio
   sullo stesso albero costruisce e sostituisce senza rigirare. *Scende*: i
   5–8 minuti per rilascio quando la suite è già verde.
3. **Il rilascio spinge il tronco**, e dice quando il remoto è indietro.
   *Scende*: i 196 commit non spinti, a zero e per sempre.
4. **La revisione di sicurezza diventa un passo di flusso al rilascio**, con
   un giudice che ne legge l'esito e scrive un guasto quando trova qualcosa —
   o si spegne, se l'esito non serve. *Scende*: 155 sessioni pagate e non
   lette, a una per rilascio, letta.
5. **Un flusso che fallisce N volte di fila scrive un guasto da sé**:
   `write-down-what-broke` esiste già; gli manca un innesco — «la stessa
   entità ha fallito le ultime tre corse». *Scende*: le 21 corse di
   `prova-della-vista` senza un guasto.
6. **`check_failed` si spiega**: quando un controllo dichiarato respinge una
   risposta, la riga del deposito porta *quale* controllo e *cosa* ha visto,
   e `sailor flow cost` le somma per passo. *Scende*: 30 passi rotti per una
   classe che oggi dice solo «no».
7. **Il lavoro di sviluppo entra nei flussi con un mandato alla volta**:
   `sviluppa-sailor` prende il prossimo guasto aperto come mandato, senza che
   una persona lo scriva. *Sale*: le chiamate a motore che passano da Sailor,
   da 30 in dieci giorni.

Le prime tre sono strumenti di sviluppo e costano un giorno. La quarta è una
decisione di Theo su un plugin suo. Le ultime tre sono flussi e innesco, e
sono il modo in cui Sailor smette di essere sviluppato **accanto** a sé.

## Cosa è entrato la stessa notte, e cosa no

Misurato sull'albero a `b03afc54`, la notte fra il 4 e il 5.

- **1, `sailor ratchet`: c'è.** Trentasei giudici — quelli che leggono le
  sorgenti, trovati e non elencati — su `git archive HEAD` in
  `target/ratchet-tree`, con sopra i file modificati e i nuovi che entrano in
  cartelle che `HEAD` conosce, elencati uno per uno. Un minuto. Nella sua prima
  notte ha trovato dodici rossi miei prima di ogni commit: una frase scritta nel
  codice, un blocco di sette righe fatto incollando due commenti, tre semi da
  abbassare, una data in un commento, due righe in italiano.
- **2, il rilascio ricorda la suite: c'è.** L'albero (`HEAD^{tree}`) va in
  `state/<bersaglio>-suite-tree` quando la suite è verde; un rilascio dello
  stesso albero lo dice e non la rifà.
- **3, il rilascio spinge il tronco: c'è.** Tre rilasci hanno detto la verità
  — «non spinto, il remoto è indietro: 403» — e il quarto, alle 03:00, ha
  spinto: `origin/sorgenti` a `b70e37e8`, oltre 200 commit. Nessuno ha toccato
  una credenziale; il gestore di sistema ne aveva una buona.
- **Misurato il rilascio di un commit di sola documentazione: 7 min 44 s**
  (02:52:58 → 03:00:42), di cui 42 s di compilazione e il resto la suite, per
  un albero in cui nessun crate era cambiato. La causa è il clone in una
  cartella `mktemp` nuova a ogni rilascio: cargo giudica per percorso e mtime,
  e ricompila tutto e rilega ogni binario di prova con LTO. Il rimedio è entrato
  la stessa notte — l'albero di rilascio vive in `target/release-tree` e viene
  portato a HEAD con un checkout — e il numero dopo va scritto qui.
- **4, 5, 6, 7: no.** Non iniziati.

E i cinque concetti di Theo della stessa sera:

1. **Le sessioni analizzate**: questo documento.
2. **Memoria e comunicazione fra agenti**: la ricerca sui flussi (`flow_search`,
   `sailor flow search <parole>`, FTS5 in memoria) e sul ledger (`ledger_search`,
   `sailor search <parole>`: corse, passi, deposito) sono entrate; le memorie
   pure — `remember` come azione e `sailor remember` a mano, quattro tipi,
   provenienza, `valid_from` che resta e `valid_until` invece della
   cancellazione, **un segreto rifiutato prima di essere scritto** — e il saluto
   di ogni terminale ne dice il numero e le ultime tre. Manca il file generato
   per le tre righe di comando (la funzione che lo rende c'è, `page()`, provata
   su 250 memorie) e l'indice su eventi e guasti. Gli annunci fra agenti c'erano
   già (D.1 del mandato).
3. **I workflow degli altri e i nostri**:
   `docs/2026-09-05-i-workflow-degli-altri-e-i-flussi-di-sailor.md`.
4. **La lavagna**: la metà del motore c'è ed è provata dal vivo. `draft-a-flow`
   prende uno schizzo — blocchi, parole, frecce — e scrive un flusso che sta in
   piedi dove vivono i flussi della persona; quattro corse, tre difetti trovati
   e chiusi (86, 87 e un puntatore), la quarta ha scritto
   `traduci-documentazione-in-inglese.flow.json`, tre passi, valido. E la tela:
   il luogo «Whiteboard» sotto l'albero, accanto a Board e Changes — blocchi
   con un genere, le parole sopra, «dopo quale» — e un pulsante che manda il
   disegno a `draft-a-flow`; la corsa si guarda dove si guardano le corse, e a
   fine corsa la tela rilegge i flussi. Da provare nella finestra rilasciata
   (`sailor release window`); dalla riga di comando lo schizzo resta testo,
   `sailor flow run draft-a-flow "…"`.
5. **Le prestazioni**: non iniziato. Va fatto con ricerca vera, non a memoria.

