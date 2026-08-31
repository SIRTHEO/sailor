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

## Esito

*Da riempire quando i due lati hanno finito. Se questa sezione resta vuota,
l'esperimento non è stato concluso e nessuna delle righe sopra è una conclusione.*
