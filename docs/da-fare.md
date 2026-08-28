# Le cose da fare

**Questo file è provvisorio, e la sua sparizione è l'obiettivo.** Esiste perché
oggi le cose da fare stanno in quattro documenti da 1.334 righe complessive, nei
prompt di chi lavora, e nella testa di chi sta leggendo — cioè in posti che
scadono. Il giorno che Sailor ha la sua sezione delle voci sospese, questo file
si cancella e non si rimpiange.

**Non ripete numeri che il sistema sa dare**: dove un fatto è già registrato, qui
c'è il rimando, non la copia. Una copia a mano invecchia da sola, e lo sappiamo
perché è già successo.

Aggiornato il 29/08/2026.

## Aspettano una decisione di Theo

Nessuna di queste è bloccata da lavoro: sono bloccate da una scelta.

1. **Il potere di un passo.** Oggi esiste un tipo di passo che esegue qualunque
   comando, ed è già metà di ciò che abbiamo scritto. Finché resta così, il gate
   delle autorizzazioni dell'autocura è una convenzione: un flusso può riscrivere
   il file che dice cosa gli è permesso. *Novità del 29/08*: il flusso di ricerca
   ha trovato la risposta collaudata in Bazel — **ciò che un passo non ha
   dichiarato, il passo non può toccare** — e in OPA Gatekeeper il modo di
   introdurla senza rompere niente: un controllo nuovo entra in osservazione
   (`warn`), non in barriera, e la promozione è un cambio di configurazione.
   *Per esteso*: `docs/2026-08-28-sailor-si-sviluppa-su-se-stesso.md`.
2. **Dove vive il file delle autorizzazioni**: fuori dall'albero (difende, non si
   vede nei diff) o versionato dentro (si vede, ma è a portata dell'autocura).
3. **Se i flussi si spediscono col prodotto.** Chi costruisce il rilascio non
   nomina i flussi nemmeno una volta: la scelta va fatta prima che qualcuno
   scriva quella riga.
4. **La soglia di un flusso che accompagna: prezzo o qualità.** Misurato: il
   degrado della qualità non è osservabile (una moneta), il prezzo di continuare
   sì (+34%). Raccomandato il prezzo. *Per esteso*:
   `docs/2026-08-28-il-flusso-che-accompagna.md`.
5. **Prima l'orologio o prima la staffetta.** Ciò che calcola quando un flusso è
   dovuto esiste già; nessuno esegue ciò che calcola.
6. **Se rimettere le istruzioni globali di Claude Code** (`~/.claude/CLAUDE.md`,
   cancellate nella pulizia del 28/08, presenti nell'archivio).
7. **L'ambiente Python del plugin sicurezza**, 240 MB, si ricrea da solo.

## Chieste da Theo, non ancora iniziate

- **I processi non muoiono quando si chiude la finestra o la riga di comando**, e
  si riprendono da soli. Lo stesso problema visto dall'altro lato: oggi Sailor
  avvia processi e **non sa quali ha avviato**, quindi non può né spegnerli né
  riprenderli (è il guasto 4).
- **Profili e account multipli**, per qualunque strumento a riga di comando.
- **La repo contiene solo i flussi che si spediscono.** Oggi contiene anche i
  nostri di lavoro e di prova. La parte che conta non è spostarli: è il controllo
  che impedisce che ci tornino.
- **I flussi personali si condividono**: in locale o su una repo versionata che
  si crea da sola; se pubblica altri li importano, se privata no. Il pezzo
  difficile è **rifiutare di pubblicare un flusso che contiene un segreto**:
  senza quel controllo la condivisione è una fuga di dati con l'aspetto di una
  funzione.
- **Una sezione delle voci sospese dentro Sailor**, che rende questo file
  inutile.
- **Ricerca su tutto ciò che il sistema conserva, anche per significato**, dalla
  finestra e da qualunque agente. Come nodo di sistema, non come funzione della
  finestra.
- **Spazi di lavoro come pagine**: una pagina per i flussi di sistema, una per i
  generici, una per ogni spazio di lavoro, coi nodi collegabili fra pagine.

## Difetti noti e non riparati

I guasti, con come si sono visti e cosa li impedirebbe, stanno in
`docs/guasti-incontrati.md` — **cinque aperti su undici**. Qui solo quelli che
non sono guasti ma limiti dichiarati:

- **Il testo di un passo non scorre**: si vede solo a passo concluso. Un flusso
  che lavora venti minuti è cieco per venti minuti. È un punto solo in
  `crates/actions`. *Bloccante per il dogfooding.*
- **I passi girano in fila, non insieme.** Il codice lo dichiara, la
  documentazione diceva il contrario (corretta il 29/08).
- **La cassetta dei passi offre otto tipi, il motore ne esegue tre.** Chi usa uno
  degli altri cinque costruisce un flusso che non parte. La cura è la stessa
  degli strumenti: chiedere al motore, non tenere una lista.
- **Un passo che esegue un comando non conserva il proprio testo**: di lui resta
  solo l'esito.
- **`sample.ts` contiene dati finti** scritti «finché la finestra non legge dal
  motore», che ormai legge.
- **Il crate dei ganci di Claude Code serve un mondo che stiamo smontando**, e
  tre delle sue prove leggono la configurazione della macchina di chi le esegue.
- **In modalità viva, un errore di compilazione in un crate qualunque uccide la
  finestra** invece di lasciarla all'ultima versione buona.

## Risposte già trovate, da mettere in pratica

Il flusso `come-lo-risolvono-gli-altri` ha coperto sette guasti su undici con
pratiche collaudate altrove. Le risposte complete stanno nel deposito, nella
corsa di quel flusso; qui l'indice:

| guasto | dove è già risolto |
|---|---|
| 1, 3 — strumenti e argomenti sbagliati scoperti solo eseguendo | actionlint: validare nome e argomenti contro l'auto-descrizione dello strumento |
| 2 — passo fallito registrato come riuscito | HyProv: strato di provenienza separato dal risultato grezzo |
| 4 — il processo orfano capitato due volte | Sillito & Kutomi: il seguito di un guasto è un controllo, non un compito |
| 5 — prove che leggono la macchina | Bazel: ambiente chiuso, dichiarato |
| 7 — la descrizione dice il falso sul codice | DocPrism: contraddizione locale con filtro deterministico |
| 8 — un campo ignoto scarta il componente | n8n: la versione del costrutto è scritta nell'artefatto |
| 9 — l'elemento invisibile | Playwright: lo schermo di riferimento nasce dal primo giro rosso |
| il caso di prova | Temporal: la storia della corsa rotta si scarica e si rigioca |
| introdurre un controllo senza rompere | OPA Gatekeeper: `warn` prima di `deny`, promozione come configurazione |

## In corso adesso

- Il flusso di ricerca `come-lo-risolvono-gli-altri`, al terzo passo su cinque.
- Un cantiere sui flussi di sistema e sulla ricerca degli strumenti come flusso.
- `sviluppa-sailor` è scritto e valido, in attesa che il workspace compili.
- **Un flusso può guardare com'è andata**: fatto il 29/08/2026. L'azione
  `history_ask` risponde a quattro domande nominate — quante volte un passo è
  fallito e con che classe, quali guasti sono i più frequenti, com'è andata
  l'ultima corsa chiusa di un flusso, quanto ci mette di solito un passo — e
  compare fra le «azioni disponibili» di `sailor flow check`. Niente SQL nei
  file di flusso: lo schema resta in `ledger`, e le domande sono chiuse.
  `input` e `output` non escono mai; `said` esce solo su richiesta esplicita,
  dai soli passi rotti di una corsa sola, troncato. Deposito assente, deposito
  vuoto e zero guasti sono tre risposte diverse, e nessuna delle tre rompe il
  passo.
  Restano aperti due limiti che nessun controllo locale mostra — restano verdi:
  - **Le durate sono in secondi interi**, perché l'orologio del motore conta
    secondi: un passo che dura meno di un secondo misura zero, e due zeri non
    si confrontano. La risposta lo dichiara nel campo `unit`, ma dichiararlo
    non lo risolve.
  - **I passi di corse mai registrate in `runs` restano fuori da ogni finestra**:
    il flusso di appartenenza vive solo nell'intestazione della corsa, quindi
    senza intestazione quei passi non si possono attribuire e non si contano.
