# Un flusso consuma più o meno di un prompt solo?

**Scritto il 31/08/2026 prima di vedere i numeri**, apposta: un disegno
sistemato dopo aver visto il risultato non è più un disegno, è una spiegazione.

## La domanda, nella forma in cui conta

Theo: «*un prompt scritto bene in una sessione per ora consuma meno di un flusso,
ma noi dobbiamo arrivare a che un flusso ben scritto consumi meno e lavori meglio
di un singolo prompt*».

Non si misura «quanti token brucia un flusso»: si misura **quanto costa arrivare
a un lavoro accettato**. È una differenza che decide tutto, perché include i
rifacimenti — cioè il posto dove un flusso con una verifica indipendente
dovrebbe guadagnare quello che perde in trasporto di contesto.

## Perché solo adesso

Fino al 30/08/2026 il costo di una corsa era la costante zero. Non era una
domanda a cui si poteva rispondere male: era una domanda a cui non si poteva
rispondere. Dal 31/08 `sailor flow cost <flusso>` risponde.

## Il disegno

**Un solo mandato, scritto una volta.** È il punto su cui l'esperimento sta o
cade: se scrivessi due testi — uno per il prompt e uno per il flusso — starei
misurando la mia scrittura, non l'orchestrazione. Il file è
`scratchpad/mandato.txt`, e i due lati ricevono quei byte identici.

**Due cloni separati dello stesso commit.** Tutti e due i lati scrivono davvero
nel codice: sullo stesso albero si pesterebbero i piedi, e il secondo troverebbe
il lavoro del primo già fatto.

| | lato A | lato B |
|---|---|---|
| chi esegue | `claude -p` una volta sola | `sailor flow run sviluppa-sailor` |
| passi | uno | nove, di cui uno giudica e uno commette |
| misura | `--output-format json`, campo `usage` | `sailor flow cost` |

**Il compito**: il ciclo di attesa in `crates/actions/src/lib.rs` fa polling
fisso a 50 ms, quindi un comando da 5 ms viene atteso 50. Piccolo, definito, e
con un giudizio oggettivo — `cargo test --workspace` passa o no.

## I tre modi in cui questo confronto può mentire, e cosa faccio

1. **Un campione solo non vale.** Due giri identici dello stesso flusso hanno già
   dato 4 e 9 su una misura precedente. Un solo giro per lato dice pochissimo, e
   va dichiarato come tale finché non se ne fanno altri.
2. **Chi crea non giudica.** Il verdetto sulla qualità delle due riparazioni non
   lo do io che ho preparato l'esperimento: va a un motore che non ha scritto né
   l'uno né l'altro, e che riceve le due diff **senza sapere quale è quale**.
3. **La misura dei due lati non è la stessa cosa.** Il lato A dichiara il proprio
   consumo nel JSON di uscita; il lato B lo scrive nel deposito riga per riga. Se
   una delle due contasse qualcosa che l'altra non conta, il confronto sarebbe
   fra due unità diverse con lo stesso nome. Va verificato che i token del lato B
   sommati corrispondano a quanto i motori hanno dichiarato, come già fatto il
   30/08 sulla catena della spesa.

## Quello che il confronto non dice, per costruzione

Il flusso fa **più cose** del prompt: un piano che non tocca niente, una verifica
data a un motore che non ha scritto il lavoro, e un commit che dipende dal
verdetto. Se esce che costa di più, quella differenza non è sprecata — è pagata
per una garanzia che il prompt singolo non offre. La domanda giusta resta
«quanto costa un lavoro **accettato**», e un prompt singolo che sbaglia e va
rifatto due volte costa due volte.

## Lato A: il «prompt singolo» non è una chiamata sola

Prima sorpresa, e cambia la domanda. `claude -p` con quel mandato ha fatto
**trenta turni** in cinque minuti. Non è un prompt: è una sessione agentica.

| | token | prezzo/M | costo | quota |
|---|---:|---:|---:|---:|
| input nuovo | 52 | 5,00 | 0,0003 $ | 0,0% |
| output | 14.195 | 25,00 | 0,3549 $ | 25,2% |
| **cache letta** | **1.140.373** | 0,50 | **0,5702 $** | **40,4%** |
| **cache scritta (1h)** | **48.483** | 10,00 | **0,4848 $** | **34,4%** |
| **totale** | **1.203.103** | | **1,4102 $** | |

Il totale calcolato dal nostro listino coincide al centesimo con quello che il
motore dichiara (1,4102 contro 1,4102): la catena di misura regge.

**Il 98,8% dei token che il modello ha visto è contesto** — riletto o riscritto —
e **il 74,8% del costo non è produzione**. L'input davvero nuovo è 52 token: lo
zero per cento.

Prima conseguenza, e vale per tutti e due i lati: **anche una sessione ripaga il
contesto a ogni turno**, trenta volte qui. La differenza fra prompt e flusso non
è *se* si ripaga il contesto.

## Lato B: quattro sessioni, non quattro chiamate

| passo | turni | durata | costo | cache letta | 1º turno: già in cache |
|---|---:|---:|---:|---:|---:|
| `scegli` | 10 | 84 s | 0,6227 $ | 211.647 | 46.702 |
| `piano` | 7 | 130 s | 0,5526 $ | 94.959 | 31.651 |
| `implementa` | 27 | 294 s | 1,5008 $ | 1.322.565 | 63.266 |
| `verifica` | 18 | 172 s | 1,2657 $ | 915.938 | 71.173 |
| **totale** | **62** | **679 s** | **3,9418 $** | **2.545.109** | |

Anche qui il calcolato dal nostro listino coincide col dichiarato: 3,9418 contro
3,9418, su quattro processi separati. La catena di misura regge in tutti e due i
sensi.

**3,9418 $ contro 1,4102 $: il flusso costa 2,79 volte il prompt.** In tokens:
2.757.958 contro 1.203.103, cioè 2,29 volte. Il costo cresce più dei token
perché la cache *scritta* — 10 $/M, la voce più cara dopo l'output — è il 44,8%
della spesa del flusso contro il 34,4% di quella del prompt: **quattro sessioni
scrivono quattro cache invece di una.**

## Perché il flusso costa di più, e non è quello che pensavo

Un'ora fa, con solo il lato A misurato, avevo scritto qui che un passo di flusso
paga il contesto come **input nuovo** a 5,00 $/M mentre una sessione lo rilegge
dalla cache a 0,50, e che la cura era mettere il contesto comune in testa.
**È falso, e il lato B lo mostra nell'ultima colonna della tabella.**

Il primo turno di `piano` — un processo appena nato, che non ha ancora fatto
niente — legge **31.651 token dalla cache**. Quello di `verifica`, 71.173. La
cache dei prefissi attraversa già i processi: il preambolo dell'armatura è
identico da un passo all'altro e viene agganciato. L'input davvero nuovo su
tutto il flusso è **100 token**, lo 0,0% della spesa. La cura che avevo annotato
curava una malattia che non c'è.

Quello che il flusso paga davvero sta nella colonna «cache letta», e si legge
meglio così: `implementa` da solo ne legge 1.322.565, più dell'intero lato A.
Ogni passo apre una sessione che **riscopre il repository da capo**. `scegli`
legge `da-fare.md` e `decisioni.md`; `piano` va a rileggersi il codice; `implementa`
lo rilegge una terza volta; `verifica` una quarta, e per giunta rifà le mutazioni.
Nessuno passa al successivo ciò che ha già letto: passano solo la propria
risposta in JSON, poche migliaia di caratteri.

**Il flusso non paga il contesto al prezzo sbagliato. Paga quattro volte per
scoprirlo.**

## Ma il lato B ha fatto qualcosa che il lato A non ha fatto

Il verificatore del flusso non si è fidato del racconto di chi aveva scritto:
si è copiato l'albero in `$TMPDIR`, **ha rimesso il difetto** in due modi diversi
e ha guardato che le due prove nuove diventassero rosse una per una, poi ha
cancellato la copia. È esattamente il controllo che `docs/decisioni.md` pretende
— «una prova vale solo se poteva venire diversa» — e nessuno gliel'aveva chiesto
in quei termini: era nel mandato che il passo `scegli` si era scritto da solo.

Il lato A dichiara «RIPARATO: sì, 523 prove passate». È vero. Ma è il suo stesso
autore a dirlo, e il mandato che gli era arrivato non conteneva la domanda sul
cronometro, perché quella domanda l'ha aggiunta il passo `scegli` del flusso
leggendo la chiusura del guasto 7. Il flusso ha speso 0,62 $ per scriversi un
mandato migliore di quello che gli avevo dato io.

## Quello che è andato storto nell'esperimento, e che va detto

**L'isolamento dichiarato non c'è stato.** Il disegno prevedeva due cloni
separati. `flows/sviluppa-sailor.flow.json` però scrive `"workdir":
"/home/someone/personal/sailor"` in chiaro su ogni passo: lanciato dal clone
`ab-flusso`, il lato B ha comunque lavorato nel repository principale, e ci ha
anche commesso sopra. Il confronto delle due diff regge — stesso commit di
partenza, stesso file, nessuna delle due ha visto l'altra — ma **un flusso con
un `workdir` assoluto non si può eseguire su un clone**, ed è un guasto suo, non
dell'esperimento.

**Un campione per lato resta un campione per lato.** Era il primo dei tre modi
di mentire elencati sopra, e non è stato tolto: due giri dello stesso flusso
hanno già dato 4 e 9 su una misura precedente. I rapporti 2,79× e 2,29× sono
**una osservazione, non una costante**.

## Il giudizio cieco

Le due diff sono andate a un motore che non aveva scritto né l'una né l'altra,
in una cartella dove non poteva risalire alla provenienza — il repository
l'avrebbe tradito, perché il lavoro del lato B era già in `HEAD`. Quale fosse
`diff-1` l'ha deciso un sorteggio, non io. (Prima ci ho provato con `codex`, che
è stato ucciso senza uscita dopo aver letto i file: guasto suo, non del disegno.)

**Ha scelto la diff 1, che era il flusso.** La ragione decisiva è esattamente la
domanda che il flusso si era aggiunto da solo nel mandato e che il mio non
conteneva:

> «*la sola prova discriminante della diff 2 cronometra dieci spawn di `sh`
> contro un budget di 250 ms e dipende pure dalla corsa "il figlio è già morto
> al primo `try_wait`", quindi può venire rossa a caso e verde col difetto
> rimesso*»

Le due riparazioni del codice il giudice le dà per equivalenti. A separarle è
solo la prova — cioè la cosa su cui il flusso aveva speso un passo in più.

### Ma il vincitore aveva un buco, e il giudice l'ha trovato

`the_poll_pause_grows_up_to_the_cap_and_stays_there` chiedeva che la sequenza
fosse **non decrescente**, mai sopra il tetto, e finisse sul tetto. La sequenza
del polling fisso, `[50, 50, 50…]`, soddisfa tutte e tre. **La prova passava col
difetto rimesso.** Verificato rimettendolo davvero, non leggendolo:

    test tests::the_first_poll_pause_is_short_not_fifty_milliseconds ... FAILED
    test tests::the_poll_pause_grows_up_to_the_cap_and_stays_there ... ok

Il verificatore del flusso aveva fatto le mutazioni — sul serio, in `$TMPDIR` —
ma aveva mutato *il tetto*, non *la crescita*, e su quella mutazione la prova
diventava rossa davvero. **Rompere una prova non basta: bisogna romperla nel
punto in cui stava il difetto originale.** È il buco che «chi crea non giudica»
esiste per trovare, e la catena l'ha trovato solo al terzo passaggio — autore,
verificatore, giudice cieco.

Riparato: la prova ora chiede che ci sia una salita *prima* del tetto, e ne è
nata una terza che interroga la regola di crescita da sola, senza avviare
niente — cosa che un commento prometteva a chi legge e nessuno faceva. Con
l'attesa fissa rimessa ora falliscono tutte e due; con `*3` al posto di `*2`
fallisce solo la terza. 526 prove verdi, uscita 0.

## Esito

Alla domanda di Theo — «*un flusso ben scritto deve consumare meno e lavorare
meglio di un singolo prompt*» — oggi la risposta misurata è: **lavora meglio,
consuma 2,79 volte tanto.**

E la seconda metà non è una legge di natura. Il 44,8% della spesa del flusso è
cache *scritta* da quattro sessioni che si ignorano, e il 92,3% dei suoi token è
cache *letta* per riscoprire quattro volte lo stesso repository. Sono i due
numeri su cui si lavora, e nessuno dei due si cura mettendo il contesto in testa.

Il «meglio» invece è misurato, non dichiarato: l'ha detto un terzo che non
sapeva quale diff fosse quale, e la ragione che ha dato — la prova del flusso
regge sotto carico, quella del prompt no — è la stessa che ha permesso di
trovare, un passaggio dopo, il buco che il flusso stesso si era lasciato dentro.

**Dove si lavora, in ordine di quanto rende:**

1. **Passare al passo successivo ciò che il precedente ha già letto**, invece
   della sola risposta in JSON. Oggi `implementa` rilegge da zero il file che
   `piano` aveva appena finito di studiare, e da solo consuma più dell'intero
   lato A. È il 92,3% dei token del flusso.
2. **Non aprire quattro sessioni dove ne bastano meno.** Ogni sessione paga la
   propria cache scritta a 10 $/M: è il 44,8% della spesa. `scegli` e `piano`
   fanno due letture della stessa cosa a otto minuti di distanza.
3. **Non toccare l'ordine del prompt.** Misurato: non serve.

Nessuno di questi tre punti è stato provato. Sono la lista da cui partire, e il
modo di sapere se hanno funzionato è rifare questa misura — che adesso si fa con
due comandi.

## Il punto 1, provato: la riscoperta cala, la spesa no

**31/08/2026, sera.** Il punto 1 è stato costruito — un passo può **ramificare**
la sessione di un altro invece di riaprire un processo che non sa niente — ed è
stato misurato. Il risultato non è quello che la lista prometteva, e va scritto
com'è.

**Il disegno.** Due flussi gemelli, `esamina-la-repo-ramificando` e
`esamina-la-repo-riscoprendo`: gli stessi quattro mandati parola per parola, lo
stesso albero, lo stesso motore. L'unica differenza è che nel primo i tre passi
indipendenti dichiarano `{"fork": "scopri"}`. Il motore è `codex`, perché è
l'unico dei quattro installati su cui il giro completo si è potuto verificare —
`claude --print --session-id` accetta l'identificativo e lo ridichiara
nell'uscita, ma su questa macchina non scrive nessun file di sessione, e il
`--resume` dopo risponde «No conversation found».

**I numeri, due giri per lato, in token dichiarati da `codex`:**

| | scopri | struttura | rischi | attrito | totale |
|---|---:|---:|---:|---:|---:|
| riscoprendo, 1º giro | 32.519 | 40.861 | 57.103 | 32.365 | **162.848** |
| ramificando, 1º giro | 23.912 | 33.106 | 39.627 | 15.976 | **112.621** |
| riscoprendo, 2º giro | 32.505 | 28.132 | 32.548 | 24.413 | **117.598** |
| ramificando, 2º giro | 43.691 | 27.192 | 47.311 | 47.168 | **165.362** |

**280.446 contro 277.983: nessuna differenza.** Lo 0,9% che separa i due totali
è più piccolo della distanza fra i due giri dello stesso lato — il passo
`scopri`, mandato identico e albero identico, è costato 23.912 token una volta e
43.691 un'altra, cioè 1,83 volte. **La varianza di questo motore su un ingresso
fermo è più grande dell'effetto che si voleva misurare**, ed è il terzo dei tre
modi di mentire elencati in cima a questo documento: un campione per lato non
vale, e due nemmeno.

**Ma una cosa si vede, e non è ambigua: quanti comandi ha eseguito ogni passo
per riscoprire l'albero.**

| | struttura | rischi | attrito | totale |
|---|---:|---:|---:|---:|
| riscoprendo, i due giri | 2 · 3 | 5 · 2 | 3 · 2 | **17** |
| ramificando, i due giri | 0 · 0 | 2 · 1 | 0 · 1 | **4** |

Quattro contro diciassette, e nessun passo ramificato ha superato il **minimo**
dei passi che riscoprono. Un ramo che risponde senza aprire un file ha davvero
il contesto del tronco: il meccanismo fa quello che dichiara.

**La lettura onesta è questa.** Il gesto funziona — la riscoperta cala del 76% —
e **il risparmio in token non è osservabile** a questo numero di corse. Le due
frasi stanno insieme senza contraddirsi: `codex` dichiara un totale unico, senza
separare ingresso, uscita e cache, quindi il conto che vediamo mescola il
contesto ereditato (che il ramo paga, ed è grosso) con la riscoperta risparmiata
(che non paga). Il primo cresce quanto il secondo cala, e il totale non si
muove.

**Cosa servirebbe per rispondere davvero**, e non è stato fatto: rifare la
misura su un motore che i due lati li dichiara — `claude`, con `usage` separato
per cache letta e scritta — dove si vedrebbe se il contesto ereditato arriva
dalla cache a 0,50 $/M invece che come ingresso fresco a 5,00. È lì che la
ramificazione o vince o non vince, e su `codex` quella domanda **non si può
nemmeno formulare**. Finché il giro non si chiude su `claude` su questa
macchina, il punto 1 resta: *costruito, provato nel meccanismo, non provato nel
prezzo.*
