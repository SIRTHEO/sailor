# The window: four places today, three questions proposed

Niente codice in questo file: è la decisione che manca prima di toccare
`places.ts`. Le prime due domande («cosa mi aspetta», «cosa gira e quanto
costa») sono già risposte sulla stessa schermata (`WaitingScreen`, riga
"Running now"). Quello che resta aperto è la navigazione primaria intorno a
loro: oggi ha 4 voci, la proposta di w30 ne vuole 3.

## Oggi — 4 voci in `places.ts`

| # | voce | cosa chiede/mostra |
|---|---|---|
| 1 | **Waiting for you** (gruppo "work") | cosa vuole una decisione, cosa è successo mentre eri via — ora anche cosa gira e il suo costo |
| 2 | **the work** ("terminals", gruppo "work") | i terminali vivi, e cosa costano |
| 3 | **Why** ("memory", gruppo "what happened") | perché ogni cosa è stata fatta così, cosa è costata, il deposito sotto |
| 4 | **the machine** ("sailor", gruppo "itself") | cosa è configurato qui, uguale ovunque ti trovi |

```text
+------------------------------------------------------------------------------+
| [Waiting for you] [ the work ] [ Why ] [ the machine ]                       |
+------------------------------------------------------------------------------+
|                                                                              |
|  ( il contenuto della voce scelta )                                        |
|                                                                              |
+------------------------------------------------------------------------------+
```

**Cosa un utente medio non trova**: quattro voci senza gerarchia visibile
fra loro — «Why» e «the machine» rispondono a domande che l'utente medio non
si è ancora posto quando apre la finestra (perché è stato fatto così, cosa è
configurato), eppure siedono allo stesso livello di «cosa mi aspetta». La
parola dell'owner del 13/09 («un utente medio non capirebbe nulla, è un casino
di gerarchia») si misura esattamente qui: quattro pulsanti di peso uguale, di
cui solo il primo risponde a una delle tre domande che contano appena la
finestra si apre.

## Proposta — 3 voci intorno alle tre domande

| # | voce | domanda | cosa succede alle 2 voci escluse |
|---|---|---|---|
| 1 | **Waiting for you** | cosa mi aspetta + cosa gira e costa | resta com'è oggi, invariata |
| 2 | **the work** | dove sta il mio terminale | resta com'è oggi, invariata |
| 3 | **the machine** (o una voce "Setup") | cosa è configurato qui | resta, ma unica voce fuori dalle tre domande — la macchina non è mai la prima cosa che un utente scansiona |
| — | **Why** (memoria/storico) | — | non è una delle tre domande a colpo d'occhio: scende a secondo livello (una riga "Why" dentro il run che l'ha generato, come già propone la ricerca w25) o resta raggiungibile solo dalla CLI |

```text
+------------------------------------------------------------------------------+
| [Waiting for you] [ the work ] [ the machine ]                               |
+------------------------------------------------------------------------------+
|                                                                              |
|  ( il contenuto della voce scelta )                                        |
|                                                                              |
+------------------------------------------------------------------------------+
```

Nota onesta: anche a 3 voci, «the machine» non è una delle tre domande del
mandato — è configurazione, non stato. Tenerla in navigazione primaria è un
compromesso (qualcuno deve pur trovare Engines/Profiles/Models), non una
quarta domanda nascosta. L'alternativa pulita sarebbe 2 voci primarie più
"the machine" ridotta a un'icona di sistema separata dalla fila delle
domande — non decisa qui, segnalata per l'owner.

## In fondo

- **Cosa perde chi ha 4 voci**: la scansione a colpo d'occhio che il mandato
  chiede. Con 4 pulsanti di peso uguale, un primo utente deve leggere tutte e
  quattro le etichette per capire qual è la sua, invece di riconoscere la
  prima come «casa».
- **Cosa guadagna chi ne ha 3**: la fila di navigazione smette di mentire
  sulla gerarchia — le due voci di lavoro reale (Waiting, the work) pesano
  come le tre domande che contano, "Why" smette di competere con loro per
  l'attenzione di chi ha appena aperto la finestra.
- **Quale giudice si accende dopo la scelta**: la regola di w30 «massimo 3
  elementi nella navigazione primaria» (parsing dei figli del nodo di
  navigazione in `App.tsx`, seme sulla lista dei posti) — oggi è falsa (4),
  quindi resta spenta finché questa pagina non ha un sì; il giorno dopo il sì
  diventa il quarto pezzo verificabile di questo ramo.
