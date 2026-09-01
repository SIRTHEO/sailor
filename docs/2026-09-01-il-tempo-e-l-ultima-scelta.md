# Il tempo è l'ultima scelta: i quattro livelli, applicati al ciclo di vita di un ramo

**01/09/2026.** Nato da una richiesta di Theo — *«se necessario facciamo i nodi
di tempo, timer, cron che partano precisi ogni tot»* — e dalla risposta che la
ricerca ha trovato al posto mia, in un repo che non è questo.

I **fatti misurati** stanno separati dalle **decisioni**, che restano a Theo.
Dove un numero compare senza la parola «misurato» è una lettura del codice, non
una prova eseguita.

## La regola esiste già, ed è di other-repo

`a-client/.claude/rules/async-no-cron.md`, scritta mesi fa, apre così:

> «Un cron di pulizia è quasi sempre il sintomo di un ciclo di vita dei dati mal
> modellato.»

È la stessa cosa che Theo ha detto il 31/08 con un'immagine: *se mi ammalo
perché sto a −10 in maglietta, non compro i farmaci — compro i vestiti*. La
regola la traduce in un **ordine di preferenza**, e il cron è il quarto:

1. **L'orfano non deve poter esistere** — vincolo o cancellazione coordinata nel
   gesto stesso che chiude la cosa.
2. **Un evento dice quando agire** — chi fa la cosa emette il segnale.
3. **Scade alla lettura** — nessuno spazza: chi guarda trova il vecchio già
   dichiarato vecchio.
4. **Cron** — solo se intrinsecamente periodico e senza alternativa, oppure come
   **rete di sicurezza** di un percorso a eventi che può perdere il segnale.

Con due righe che valgono più del resto: *«un cron aggiunto deve avere un
commento che spiega perché i passi 1-3 non si applicano»*, e **«il cron non è
mai il motore — chi accoda deve svegliare»**. Una coda drenata solo dal tick ha
per latenza l'intervallo del tick, sempre.

## Il costo di non avere i primi tre livelli, misurato

Il 31/08/2026 su other-repo c'erano **31 rami orfani**. Per decidere che farne sono
serviti **ventuno agenti e circa mezz'ora**: un esaminatore per gruppo e un
giudice avversariale per ogni verdetto. Non era zelo — la domanda «questo lavoro
è già arrivato altrove?» si risponde solo confrontando il contenuto file per
file, perché le richieste si uniscono a schiacciamento e quindi il ramo non
risulta **mai** antenato del tronco: `git cherry` e `--contains` rispondono
sempre «non unito», anche quando il lavoro è dentro da settimane.

Esito: 23 da chiudere, 4 da portare avanti, 4 da decidere. **Un solo verdetto su
31 è stato ribaltato dal giudice** — il che dice che l'analisi funziona, e
insieme che è costata ventuno agenti per confermare quello che chi creò quei
rami sapeva già il primo giorno.

## I quattro livelli applicati a un ramo

### Livello 1 — il ramo non può restare orfano

Chi unisce la richiesta chiude il ramo nello stesso gesto. Non c'è niente da
sorvegliare perché non nasce niente da sorvegliare. Su GitHub questo è una
casella nelle impostazioni del repository, non un flusso: **il posto più
economico dove il problema si risolve non è dentro Sailor.** Vale scriverlo qui
perché un sistema che vuole governare un ciclo di vita deve saper dire anche
«questo pezzo non tocca a me».

### Livello 2 — la fusione è un evento

Chi unisce lo dice, e il ciclo avanza subito. Un flusso che reagisce a un fatto
esterno pretende un potere che oggi Sailor **non ha**: un innesco che si accende
su qualcosa che accade altrove.

### Livello 3 — scade alla lettura

Quando qualcuno apre l'elenco dei rami, quelli oltre la loro condizione di fine
si dichiarano scaduti lì, senza che nessuno abbia spazzato. È il livello con il
miglior rapporto fra valore e costo, e **pretende una sola cosa**: la
dichiarazione di ciclo di vita depositata alla nascita del ramo — cosa lo
chiude, cosa si perde se sparisce. Il flusso che la deposita è già scritto:
`~/.config/sailor/flows/other-repo/un-ramo-dichiara-come-finisce.flow.json`.

### Livello 4 — il timer, come rete

Passa ogni tanto e raccoglie ciò che i primi tre hanno perso. **Se i primi tre
esistono, la precisione del tick conta poco**: nel bot di other-repo la rete gira ogni
cinque minuti e non è il motore di niente.

## Cosa Sailor ha, e cosa gli manca — misurato il 01/09/2026

| serve per | Sailor oggi |
|---|---|
| depositare la dichiarazione | **c'è**: `store_write` / `store_read` / `store_list` |
| chiedere un giudizio a un motore | **c'è**: `external_engine` |
| eseguire un comando | **c'è a metà**: `shell_check` (vedi sotto) |
| accendersi su un fatto esterno | **manca** |
| accendersi a tempo | **manca** |

**`shell_check` sa dire *se*, non *cosa*.** L'uscita del passo è
`{"status": ...}` e basta (`crates/actions/src/lib.rs`, ramo `Ok(ActionOutcome::Went(json!({ "status": status })))`).
L'uscita del comando non arriva al flusso. Quindi «il ramo esiste ancora?» si
può chiedere; «quale richiesta lo riguarda, ed è unita?» no. È il quinto dei
poteri che il censimento di `dev-stack` aveva già isolato — *restituire un
valore invece di un esito*.

**Le forme di innesco sono due, e sono codice.** `trigger::Kind` ha due varianti
sole, `manual` e `terminal`; l'elenco dei descrittori è dato — si aggiunge un
JSON in `~/.config/sailor/triggers.d/` — ma una **forma** nuova è una variante
in più nell'enum. E `terminal` oggi è dichiarato e non ascolta, con una frase
che questo documento fa propria:

> «Un ascolto simulato sarebbe peggio di un ascolto assente, perché un flusso
> verde direbbe che qualcuno ha parlato.»

## Se e quando si scrive il nodo del tempo, cinque cose vanno decise prima

Non sono dettagli di implementazione: cambiano cosa il nodo *è*.

1. **Chi tiene il tempo.** Sailor acceso che conta (non scatta a finestra
   chiusa); il sistema operativo che lo sveglia (preciso, ma Sailor installa
   qualcosa fuori da sé); oppure Sailor che all'avvio guarda cosa si è perso
   («ogni tot» diventa «quando riapro»).
2. **Il portatile che dorme.** «Ogni 30 minuti» su una macchina spenta dodici
   ore: al risveglio scatta ventiquattro volte o una? Entrambe sono giuste, per
   lavori diversi — quindi **lo dichiara l'innesco**, non lo decide il motore.
3. **Intervallo e appuntamento sono due cose.** «Ogni tot» conta dall'ultima
   corsa e slitta; «alle 9» non slitta e pretende un fuso orario.
4. **Chi dice che non è scattato.** È il guasto 12 in un altro vestito: un
   innesco che non parte deve **dirlo**. Se tace, «nessuna corsa» si legge come
   «tutto a posto», che è la bugia peggiore. Nella lingua delle quattro
   superfici: un innesco è un `sense`, e un `sense` deve distinguere «zero» da
   «non ho potuto vedere».
5. **Due tick che si sovrappongono.** Nel bot è stato pagato: il motore di
   eventi non serializza i tick di suo, e ogni rete periodica porta un limite di
   concorrenza a uno, scritto a mano. Un nodo del tempo che non lo prevede
   fabbrica corse doppie.

## Cosa resta a Theo

- **Se il livello 1 si accende su GitHub** — è una casella, e toglie da sola la
  gran parte del problema. Nessun flusso lo fa meglio.
- **Quale potere costruire per primo**: leggere un valore (`shell_check` che
  restituisce), oppure accendersi da soli (l'innesco). Il primo sblocca il
  livello 3, il secondo i livelli 2 e 4. **Il livello 3 costa meno e rende
  prima.**
- **Le cinque domande qui sopra**, se e quando il nodo del tempo si scrive.

## Il controllo che rende rossa questa pagina

Come ogni regola scritta qui: *chi scrive una regola scrive anche ciò che la
rende rossa*. Per questa il controllo è una prova che scorre i descrittori di
innesco e **fallisce se un innesco di forma periodica non dichiara** cosa fa
quando una corsa è stata persa e qual è il suo limite di concorrenza. Nasce
verde oggi, perché inneschi periodici non ne esiste nessuno — ed è il modo in
cui questa pagina resta viva invece di descrivere ciò che avremmo dovuto fare.
