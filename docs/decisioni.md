# Le decisioni

**Questo file è la memoria delle scelte, e i flussi lo leggono.** Non è un
diario: ogni voce è una decisione che vincola il lavoro futuro, con chi l'ha
presa e perché. Un flusso che sta per scegliere cosa fare, o che sta per
implementare qualcosa, lo consulta **prima** — altrimenti riprende una strada
già scartata, e nessuno se ne accorge finché non è scritta.

**Perché esiste.** La notte del 29/08/2026 sono state prese sette decisioni. Non
esistevano da nessuna parte se non nei messaggi di commit e nella conversazione
in cui erano nate: il flusso lanciato il giorno dopo non poteva conoscerle.
Questo è il difetto che separa un sistema che impara da uno che ricomincia.

**Una decisione si scrive qui quando vincola qualcuno che non era presente.**
Se riguarda solo chi l'ha presa e finisce con lui, non è una decisione: è una
scelta di lavoro, e sta nel commit.

## I vincoli permanenti

Non sono decisioni prese una volta: sono il metro con cui ogni altra si giudica.
Una proposta che li viola si scarta, anche quando è migliore sotto ogni altro
aspetto.

| vincolo | cosa vuol dire in pratica |
|---|---|
| **Indipendenza dal modello** | Sailor funziona con qualunque strumento a riga di comando, compresi quelli che non esistono ancora. Una soluzione che funziona solo su un motore preciso va **dichiarata come capacità** di quello strumento, e chi non ce l'ha deve continuare a funzionare pagando di più. |
| **Chiarezza per chi guarda** | Sailor esiste perché una persona veda e controlli cosa fanno i suoi strumenti. Un'ottimizzazione che rende opaco come i passi si passano le informazioni è **peggio del costo che risparmia**. Vale anche per l'aspetto: un'interfaccia che nasconde cosa succede è il contrario del prodotto. |
| **Lo schermo è il giudice** | Una regola di progetto che non si può verificare guardando un'immagine è un'opinione. Viene dai due difetti che né i tipi né le prove hanno visto. |
| **Chi crea non giudica** | Il verdetto su un lavoro lo dà chi non l'ha scritto. Un motore che verifica se stesso ha già in contesto le proprie conclusioni: non è distratto, è compromesso. |
| **Una prova vale solo se poteva venire diversa** | Dopo averla scritta si rompe apposta ciò che prova, e si guarda che diventi rossa. Chi dichiara di non averlo fatto viene respinto. |
| **Programmiamo a codice solo ciò che tocca il mondo** | Il motore che esegue, il deposito che registra, il gate che autorizza. Tutto il resto è un flusso, modificabile senza ricompilare. Il confine è **il potere**, non «esegue contro decide». |

## Le decisioni prese

### Il potere di un passo: modello Bazel, in osservazione
**29/08/2026 — Theo.** Un passo dichiara cosa gli serve, e il resto per lui non
esiste. Il controllo entra come **avviso** e diventa barriera solo con un cambio
di configurazione, dopo averlo visto funzionare.
**Perché**: un divieto specifico si aggira, un mondo ristretto no; e la fase di
osservazione toglie la paura che rende queste cose impossibili da introdurre.
**Cosa ne discende**: ogni passo dei flussi esistenti dovrà dichiarare cosa
tocca. Non è gratis. *Non ancora costruito.*

### Il file delle autorizzazioni non esiste
**29/08/2026 — Theo.** L'autocura non ha un gate suo: è un flusso come gli
altri, con i poteri che dichiara.
**Perché**: se il modello Bazel vale per ogni passo, un meccanismo speciale per
l'autocura sarebbe difendere due volte la stessa cosa. Ed è coerente col fatto
che i flussi che usiamo per sviluppare Sailor non si spediscono a nessuno.

### I flussi di sistema stanno dentro il binario
**29/08/2026 — Theo.** Incorporati alla compilazione, non installati come file
accanto al programma. Chi ne vuole uno diverso ne scrive uno con lo stesso nome
in casa propria o nel progetto, e vince il suo.
**Perché**: un flusso spedito come file può mancare, invecchiare o essere
cancellato, e allora il prodotto si comporta diversamente su macchine diverse
senza che si capisca perché. *Fatto: `crates/flow/system/`.*

### Niente briglie sul flusso che sviluppa
**29/08/2026 — Theo.** Il passo che implementa scrive senza chiedere permesso.
**Perché**: il perimetro non è ancora applicato dal motore, e aspettarlo avrebbe
fermato tutto. Chi lancia lo sa. **Attenzione**: in un ciclo questo conta il
doppio — chi lascia girare da solo per ore deve poter vedere cosa fa mentre lo
fa, e da questo giro il testo di un passo esce su stderr mentre il passo gira.

### Le prove rosse rompono il passo
**29/08/2026 — dopo il primo giro fallito.** Nessuna tolleranza sul passo che
esegue le prove nel flusso di sviluppo.
**Perché**: la tolleranza c'era perché il verificatore vedesse l'esito anche
quando fallivano, e così **un lavoro che non compilava ha superato il gate** —
con cinque minuti di verifica spesi su codice che non stava in piedi. Un lavoro
che non compila non ha niente da far giudicare a nessuno.

### I flussi si compongono, non si fondono
**29/08/2026 — Theo.** Ricerca, smistamento, sviluppo e interrogazione del
codice sono le fasi di un ciclo unico, ma restano flussi separati che si
chiamano fra loro.
**Perché**: un flusso di dieci passi che fa tutto non si può usare a metà, e la
ricerca serve anche da sola. **Cosa ne discende**: serve `subflow`, un passo che
esegue un altro flusso. *Non ancora costruito.*

### Il ciclo sta dentro Sailor, non accanto
**29/08/2026.** Un flusso a ronda non è un flusso lungo: è un flusso corto
eseguito molte volte, e chi lo riesegue deve essere Sailor.
**Perché**: uno script che rilancia è stato scritto e cancellato lo stesso
giorno. Sarebbe stato un cerotto fuori dal sistema su un buco dentro il sistema,
e i cerotti restano. **Cosa ne discende**: serve che qualcuno esegua ciò che
`sailor flow due` già calcola. *Non ancora costruito.*

### Il testo non ripete numeri che il sistema sa dare
**29/08/2026.** Dove un fatto è già registrato, il testo ci rimanda invece di
copiarlo.
**Perché**: una copia a mano invecchia da sola. È già successo: un documento
diceva «dieci guasti» mentre il file ne elencava undici, e un verificatore ha
respinto un'intera ricerca per quell'incoerenza — a ragione.

### L'ordine di sblocco: prima le chiamate, poi l'orchestrazione, poi il ciclo
**29/08/2026 — Theo.** Tre blocchi, in quest'ordine, e ognuno si vede funzionare
prima del successivo:

1. **Le chiamate ai modelli**, profili e fornitori insieme. Comprese le quote
   gratuite che i fornitori dichiarano e che oggi non sfruttiamo, e le righe di
   comando che non abbiamo ancora (DeepSeek, Grok, OpenRouter e le altre).
2. **Orchestrare bene**: mandare il lavoro sul modello giusto per quel lavoro, e
   disegnare flussi che si reggano.
3. **Fortificare i flussi di sviluppo**, farli girare in un ciclo, e sotto una
   catena di smistamento vera che usi la macchina invece di un passo alla volta
   — sapendo se la macchina è occupata da chi ci lavora o è libera.

**Perché quest'ordine**: senza il primo blocco ogni corsa dipende da un solo
abbonamento e si ferma quando finisce, come è successo il 29/08. Senza il
secondo, avere più motori vuol dire solo avere più modi di sprecare. Il terzo è
quello che rende il tutto un sistema che va avanti da solo, e va per ultimo
perché fino ad allora ogni difetto si moltiplica per il numero di corse.

**Dopo questi tre**, il resto è miglioria: si seguono le voci in programma.

### Ogni cosa costruita come flusso ha un flusso che la cura
**29/08/2026 — Theo.** L'autocura e lo sviluppo non sono un progetto a parte:
sono la coppia di flussi che tiene in piedi tutto ciò che teniamo a livello di
flusso.
**Perché**: ciò che non è codice non ha né compilatore né prove che lo
sorveglino. Un flusso rotto resta rotto in silenzio finché qualcuno non lo
lancia. Se i flussi sono il posto dove mettiamo tutto ciò che non tocca il
mondo — ed è il vincolo permanente in cima a questo file — allora la loro
manutenzione dev'essere altrettanto seria di quella del codice, e automatica per
la stessa ragione.

### Una voce può essere deprecata o ridecisa, e non da sola
**29/08/2026 — Theo.** Mentre si sviluppano i flussi, le voci di lavoro
cambiano: alcune non hanno più senso, altre vanno ripensate. **Questo si fa
insieme a chi usa il sistema, non in autonomia.**
**Perché**: una voce che sparisce senza che nessuno lo sappia è indistinguibile
da una voce dimenticata, e la seconda è un guasto. Vale anche al contrario: un
flusso che cancella da solo ciò che gli sembra superato decide al posto di chi
deve decidere — ed è lo stesso motivo per cui la prima regola di scelta è «mai
una voce che aspetta una decisione».
**Cosa ne discende**: quando le voci passeranno nel deposito, lo stato non è
«aperta/chiusa». Serve almeno **deprecata** — non si fa più, e c'è scritto
perché — e **da ridecidere**, che è una voce che aspetta te e che nessun flusso
può prendere. E serve che il passaggio a quegli stati sia registrato con chi
l'ha fatto, come ogni altra cosa nel deposito.

## Raccomandato, non ancora deciso

- **La soglia di un flusso che accompagna va sul prezzo, non sulla qualità.**
  Misurato: il degrado della qualità non è osservabile (21 sessioni su 44, una
  moneta); il prezzo di continuare cresce del 34% ed è monotono (37 su 45).
  Aspetta una decisione di Theo.

- ~~**Il terzo blocco ha un antefatto che non è stato ancora fatto.**~~ Fatto il
  30/08/2026: il fronte parte insieme. Due passi indipendenti da sei secondi ne
  impiegano 6,07 invece di 12,07; tre ne impiegano 6,05 invece di 18,14.
  «Sfruttare la macchina» ora ha dove appoggiarsi.

  **Resta una decisione tua**: quanti passi per ondata. Oggi sono quattro, una
  costante dichiarata in `crates/flow/src/executor.rs`. Il numero non è tecnico —
  la macchina ne reggerebbe di più — ma di quota e di sorveglianza: un fronte
  largo di passi che chiamano agenti aprirebbe una decina di conversazioni a
  pagamento per una corsa che nessuno guarda. Il giorno che esisterà un tetto di
  spesa, questo numero dovrebbe diventarne una conseguenza invece che una
  costante.
