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

### La lingua: identificatori in inglese, tutto il resto in italiano
**31/08/2026.** Ogni cosa che il compilatore legge sta in inglese — funzioni,
tipi, campi, variabili, moduli, costanti, **nomi di file**, classi CSS, chiavi
JSON. Ogni cosa che legge una persona sta in italiano: commenti, messaggi
d'errore, testo nella finestra, documenti, e i **dati** delle prove.
**Perché sta qui e non solo in `AGENTS.md`.** Ci stava solo lì, e il 31/08 se ne
contavano 136 violazioni — quasi tutte scritte nei tre giorni precedenti, da
sessioni che avevano ricevuto «rispondi in italiano» come istruzione forte e
questa riga come una fra molte in un documento. Questo file è la memoria che si
rilegge prima di correggere qualunque cosa: se una regola non è qui, non è
vincolante nei fatti, qualunque cosa dica altrove.
**E soprattutto ha una misura.** `cargo test -p sailor --test
identifiers_are_in_english` cerca parole italiane in posizione di dichiarazione,
e conosce anche i nomi dei file. Non è un analizzatore: è un elenco di parole,
che non ha falsi positivi e lascia passare quelle che non conosce. Il prezzo è
dichiarato; l'alternativa era continuare a non misurare niente.
**La lezione, che vale oltre la lingua.** Una regola che nessun controllo
interroga non diventa rossa mai — è lo stesso difetto del puntatore morto che
`AGENTS.md` racconta di sé, e del guasto 22, dove uno zero mai calcolato è
passato per una misura. Chi scrive una regola nuova scrive anche ciò che la
rende rossa.

### Gli identificativi dei flussi e dei passi restano in italiano
**31/08/2026 — Theo.** `sviluppa-sailor`, `verdetto`, `implementa`, i nomi dei
file `.flow.json`: restano come sono. Il confine non è fra codice e dati in
astratto — è questo: **ciò che il compilatore legge sta in inglese; ciò che il
deposito conserva è un dato, e i dati non si rinominano per stile.**
**Perché**, con le due conseguenze che nessun compilatore prende. (1) Il
deposito ha corse già registrate con quegli `step_id`: un passo `verdetto`
diventato `verdict` fa apparire il vecchio come sconosciuto e il nuovo come mai
eseguito, e la ripresa dopo crash non ritrova più i propri passi. (2) La
decisione «i flussi di sistema stanno dentro il binario» dice che chi ne vuole
uno diverso ne scrive uno **con lo stesso nome** in casa propria, e vince il
suo: cambiare il nome spedito farebbe smettere di vincere, **in silenzio**, un
flusso che qualcuno ha già scritto.
**Cosa ne discende.** Il controllo `identifiers_are_in_english` non guarda i
`.flow.json` e non guarderà mai gli `id`: non è una dimenticanza da completare.
Chi in futuro lo estende ai dati sta rompendo questa decisione, non applicandola.
Resta l'asimmetria dichiarata: `flows/smista-il-lavoro.flow.json` ha l'id in
italiano e i passi in inglese, e va bene così — sono tutti e due dati.

### Il tetto di spesa è del flusso, e la larghezza del fronte ne discende
**31/08/2026.** Un flusso può dichiarare `spend_cap_micros`: quanto una sua
corsa può spendere. Prima di aprire ogni fronte l'esecutore chiede al deposito
quanto è stato speso; se il tetto è raggiunto la corsa si ferma con una parola
sua — `cap_reached`, non `failed` — e dice quali passi non sono partiti.
**Perché prima di aprire e non dentro l'azione**: un passo che scopre a metà di
aver sforato ha già pagato. L'unico istante in cui fermarsi costa zero è prima
di aprire il fronte.
**Perché una parola sua e non un guasto**: un flusso notturno che tocca il
proprio tetto ogni notte apparirebbe rotto ogni notte, e chi guarda smetterebbe
di guardare.
**Che cosa il tetto non promette**: si misura sui costi che i motori
dichiarano. Codex dichiara il totale dei token e non i due lati, quindi la sua
riga resta senza costo e non entra nel conto. Il tetto è una garanzia **su ciò
che si sa**, e la corsa fermata scrive quante chiamate erano fuori — perché chi
sta per alzarlo e rilanciare deve saperlo prima, non dopo.
**Il predefinito è nessun tetto.** `None` non è `Some(0)`: il primo è «nessuno
ha messo un limite», il secondo è «questo flusso non deve spendere niente». Un
tetto che comparisse da sé fermerebbe corse che nessuno ha chiesto di fermare, e
lo farebbe la notte.

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

### Il multi-fornitore si costruisce in casa, e non è un proxy
**30/08/2026 — Theo, dopo aver guardato free-claude-code.** Non si integra
`free-claude-code` né nessuno degli altri intermediari (Claude Code Router,
LiteLLM, OmniRoute, 9router). Il pezzo si fa qui.

**Perché, coi numeri che l'hanno deciso.** Quel progetto è 143.000 righe di
Python 3.14 con 157 pacchetti bloccati e un server sempre acceso, da mettere
sotto un workspace Rust che tiene tre dipendenze per scelta scritta nel
`Cargo.toml`. Ha 51.600 stelle e **una persona sola** che lo scrive. E soprattutto:
**il pezzo per cui lo si voleva non c'è dentro.** Il suo catalogo ha
identificativo, URL e nome della variabile d'ambiente — nessuna quota, nessun
limite, niente su cosa il fornitore fa dei dati che riceve. L'«oltre 1,3
miliardi di token gratis al mese» è **una riga di README senza un dato che la
sostenga**. Il pezzo caro è la traduzione dei formati fra fornitori: dodicimila
righe che loro riscrivono due volte e mezza a trimestre, e che diventerebbero
nostre per sempre.

**Cosa si prende comunque, e cosa si rifiuta.** Da rifiutare senza discussione:
due dei loro cinquanta fornitori si presentano come un altro programma — il
client OAuth della CLI di Codex e il suo `User-Agent` — per far passare un
agente sull'abbonamento di qualcun altro. Non è aggirare una quota, è fingere di
essere un altro software, e non entra qui. Da prendere, invece, **un dato che
esiste già ed è sotto MIT**: il catalogo delle fasce gratuite di OmniRoute, che
è l'unico dei quattro a portare quote mensili documentate per fornitore, la
metodologia con cui le ha misurate, e — la cosa che vale di più — un verdetto
sui termini d'uso che marca diciassette fornitori come «da evitare, i loro
termini vietano il passaggio da un intermediario». Si prende il dataset, non il
programma.

**La strada che questo apre, e che costa quasi niente.** Sailor lancia già gli
agenti come sottoprocessi con un ambiente configurabile (`launch.env`), e
esistono endpoint che parlano **nativamente** il protocollo che quelle CLI già
usano: si fa puntare lì una variabile, e non c'è nessuna traduzione da scrivere
né da mantenere. Il lavoro che resta nostro è quello che nessuno ha fatto:
un catalogo dei fornitori che porti **quanto danno gratis**, **a che patto sui
dati**, e **quanto ne resta**. Sta in `crates/models`, che già tiene i modelli.

**Cosa ne discende, e non è ancora costruito**: dove vivono le credenziali (oggi
i profili spostano file e fanno collegamenti simbolici, cioè il segreto sta in
chiaro sul disco); e la dimensione che va messa fin da subito nella regola di
instradamento — **non tutti i lavori possono andare ovunque**, perché su certe
fasce gratuite il patto è che i tuoi dati addestrino il modello, e un flusso che
legge codice privato non ha lo stesso insieme di destinazioni ammesse di uno che
riassume un documento pubblico. Aggiungerla dopo vuol dire aver già mandato
qualcosa nel posto sbagliato.

## Raccomandato, non ancora deciso

- **La soglia di un flusso che accompagna va sul prezzo, non sulla qualità.**
  Misurato: il degrado della qualità non è osservabile (21 sessioni su 44, una
  moneta); il prezzo di continuare cresce del 34% ed è monotono (37 su 45).
  Aspetta una decisione di Theo.

- ~~**Il terzo blocco ha un antefatto che non è stato ancora fatto.**~~ Fatto il
  30/08/2026: il fronte parte insieme. Due passi indipendenti da sei secondi ne
  impiegano 6,07 invece di 12,07; tre ne impiegano 6,05 invece di 18,14.
  «Sfruttare la macchina» ora ha dove appoggiarsi.

  ~~**Resta una decisione tua**: quanti passi per ondata.~~ **Sciolta il
  31/08/2026, e non con una scelta: con un'aritmetica.** Quattro non è più il
  numero, è il soffitto. Sotto un tetto di spesa la larghezza del fronte la
  calcola `how_many_fit` dal residuo diviso la chiamata più cara vista in quella
  corsa. Il motivo per cui non poteva restare una costante: **un tetto non si
  rispetta con un fronte largo** — quattro chiamate partono nello stesso istante,
  nessuna sa delle altre, e quando la prima registra il proprio costo le altre
  tre hanno già speso. Lo sforamento peggiore non è di una chiamata, è di quante
  ne sono in volo. Senza nessun costo osservato non si stringe: restituire 1 «per
  prudenza» renderebbe seriale ogni corsa con un tetto, per sempre, sulla base di
  un numero che non esiste.
