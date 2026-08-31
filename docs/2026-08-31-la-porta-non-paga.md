# La porta non paga: l'A/B del passo consegnato

**31/08/2026.** Il disegno è stato scritto prima di vedere i numeri, e con esso
la condizione di fallimento: *se il lato consegnato non costa meno del lato di
controllo, il montaggio ha fallito la propria ragione di esistere*.

**Ha fallito.** Questo documento tiene la misura.

## Cosa si confrontava

Il montaggio nato dalla ricerca del 31/08: il flusso **descrive** un passo e a
eseguirlo è l'agente già vivo nel terminale, che lo prende in carico con
`sailor step open` e lo richiude con `sailor step close`. L'ipotesi: un flusso
di quattro passi costa 2,79 volte un prompt solo **perché fa il doppio dei
turni**, e togliere gli avvii toglierebbe metà di quel fattore.

## Il disegno

- **Un mandato solo, gli stessi byte da tutte e due le parti.** Non scelto dal
  flusso: dettato dall'innesco, così i due lati fanno lo stesso lavoro. Il
  compito era vero e piccolo — `sailor flow check` esce 0 anche quando dichiara
  righe di comando rotte.
- **Due cloni pinnati sullo stesso commit** (`bbc4186`), depositi separati.
- **Lato di controllo**: `sviluppa-sailor` com'è. `scegli`, `piano`,
  `implementa` sono tre processi.
- **Lato consegnato**: gli stessi tre passi diventano `handed_to_agent`, presi
  da **una sola sessione `claude -p` fredda**.
- **`verifica` resta un processo su entrambi i lati**, apposta: è il giudice
  cieco, e se lo prendesse la sessione che ha scritto il lavoro il giudizio non
  varrebbe niente.
- **Un giudice esterno** sulle due diff, che non ha scritto nessuna delle due e
  non sapeva quale fosse quale (`lavoro-1` / `lavoro-2` per sorteggio).

## I numeri

| | controllo | consegnato | |
|---|---|---|---|
| passi andati | 9 su 9 | 9 su 9 | pari |
| turni | 109 | **95** | −12,8% |
| costo equivalente | 7,3026 $ | **7,2080 $** | **−1,30%** |
| token letti da cache | 5.430.695 | **7.013.495** | **+29%** |
| giudice del flusso | approvato | approvato | pari |
| **giudice cieco sulle diff** | **preferito** | | |

Il dettaglio per chiamata, che è dove sta la spiegazione:

| lato | passo | chi | turni | costo |
|---|---|---|---|---|
| controllo | scegli | processo | 24 | 1,1051 |
| controllo | piano | processo | 19 | 1,2001 |
| controllo | implementa | processo | 54 | 3,8081 |
| controllo | verifica | processo | 12 | 1,1894 |
| consegnato | scegli+piano+implementa | **una sessione** | **75** | **5,5406** |
| consegnato | verifica | processo | 20 | 1,6674 |

## Perché non ha funzionato

**Il problema non erano gli avvii: è quanto contesto attraversa ogni turno.**

Il processo più caro del lato di controllo ha letto 3,9 milioni di token dalla
cache. La sessione viva del lato consegnato ne ha letti 5,9 milioni — e il lato
consegnato nel suo insieme ne ha letti il **29% in più**.

Una sessione sola che fa tre lavori si porta dietro un contesto che **cresce**,
e lo rilegge a ogni turno. Tre processi separati portano ciascuno un contesto
piccolo. Quello che si risparmia non riaccendendo il motore si ripaga
rileggendo tutto ogni volta. I turni scendono del 12,8%; il costo per turno
sale abbastanza da annullarlo.

## E il registro si è rotto — che è peggio del costo

`sailor flow cost` dichiara che la corsa del lato consegnato è costata
**1,6674 $**. È costata **7,2080 $**. Sbaglia di **4,3 volte**.

Perché sui passi consegnati il consumo è **autodichiarato**: l'agente ha
dichiarato 6, 5 e 22 turni — 33 in tutto — mentre la sua sessione ne ha spesi
**75**. Non ha mentito: non sa contare ciò che il proprio harness consuma per
lui. Il 56% del lavoro è invisibile al deposito.

Quindi il montaggio **non ha sciolto la tensione che la ricerca aveva trovato
irrisolta: l'ha spostata.** Chi esegue a caldo continua a non essere
misurabile, e adesso Sailor *crede* di misurarlo — che è peggio di sapere di
non poterlo fare.

## Il giudizio cieco, e cosa dice davvero

Il giudice esterno ha scelto **il lato di controllo**, e ha detto perché: la sua
prova arriva fino al **codice d'uscita** — il numero che il mandato nomina —
mentre quella del lato consegnato si ferma al risultato intermedio; e i suoi
quattro mutanti sono distinti, mentre il lato consegnato ne riusa uno per due
prove, dove per una delle due **non esercita affatto la proprietà dichiarata**.

Ma ha anche detto: «*niente di fatale: è un lavoro solido che soddisfa il
mandato, e il distacco è di margini, non di sostanza*». E ha trovato una
debolezza nel vincitore: un pezzo di lavoro non chiesto.

Quindi: il lavoro consegnato **non è peggiore in modo interessante**. È che non
è né più economico né migliore.

## Cosa resta valido

- **`sailor step open|close` funziona nel mondo reale, non solo nelle prove.**
  Una sessione ha preso tre passi che erano tre processi, li ha fatti, e il
  deposito ha registrato ogni apertura e ogni chiusura. `reconcile` è entrato in
  servizio.
- **Il giudice cieco imposto dal contenitore regge**: su entrambi i lati
  `verifica` è rimasto un processo che non aveva visto il lavoro, e su entrambi
  ha approvato.
- **Il vincolo che questo esperimento non ha toccato resta il più forte**: fra i
  passi passa un artefatto, non una conversazione.

## Cosa NON si deve fare adesso

- **Non convertire `sviluppa-sailor` a passi consegnati.** Non paga.
- **Non «migliorare» il montaggio ingrossando il passaggio fra i passi.** È già
  una delle tre cure provate false: sposta token dalla cache letta al prompt,
  moltiplicati per i turni.
- **Non fidarsi di `flow cost` su una corsa con passi consegnati** finché il
  consumo autodichiarato non viene misurato invece che dichiarato.

## Il limite di questa misura, dichiarato

**Un giro per lato.** Due giri identici su questo repository hanno già dato 4 e
9 in passato. Il −1,30% sul costo è **dentro il rumore**, e va letto come «non
ha risparmiato», non come «ha risparmiato poco». Il −12,8% sui turni e il +29%
sulla cache letta sono differenze più grandi e più probabilmente vere, ma
restano un'osservazione.

Quello che questa misura **non** dice: che un passo consegnato non convenga
mai. Dice che non conviene **così**, su un compito di questa taglia, con tre
passi consegnati alla stessa sessione. Un passo solo, corto, consegnato a una
sessione che ha già il contesto per altre ragioni, è un caso diverso e non
misurato.

## Il costo di scoprirlo

7,30 $ il lato di controllo, 7,21 $ il lato consegnato, 0,72 $ il giudice
cieco: **15,23 $** per sapere che una strada non porta dove sembrava.
