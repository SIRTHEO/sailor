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
