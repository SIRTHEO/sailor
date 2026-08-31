# Come lo risolvono gli altri — e cosa non risolve nessuno

> **Questo documento è il verbale della ricerca, non lo stato di adesso.** Il
> giorno stesso quattro delle nove voci sono state costruite, e alcune frasi qui
> sotto sono diventate false. Le lascio come sono scritte — un verbale
> riscritto dopo non è più un verbale — e dichiaro qui cosa è cambiato:
>
> - **Voce 1, il tetto.** Costruito. `sailor flow cap` lo scrive, `flow check`
>   lo dichiara coi suoi tre limiti, e `Decision::CapReached` è passato da zero
>   esecuzioni a due, in una corsa vera a costo zero. Resta vero che nessun
>   flusso lo dichiara ancora: tararlo richiede tre corse costate che nessun
>   flusso ha.
> - **Voce 2, la porta.** Costruita. `handed_to_agent`, `sailor step
>   open|close`, `sailor flow resume`, e le corse in attesa che il deposito sa
>   nominare. **L'A/B che la giudica non è stato fatto**: finché non lo è,
>   questa voce è un montaggio provato dalle prove, non una risposta misurata.
> - **Voce 4, il vaglio a secco.** Costruito, e il guasto 27 rimesso nel
>   descrittore vero viene colto a costo zero, con le parole del motore.
> - **Il guasto 25, dichiarato qui «interamente scoperto»: è chiuso.** La
>   radice si dichiara con un marcatore, viaggia nello stato condiviso, e i
>   sette percorsi assoluti di `sviluppa-sailor` sono spariti — tolti da
>   `sailor flow relocate`, non da una mano.
> - **Restano scoperti** il guasto 18 (la dotazione esiste e non arriva ai
>   motori), il 14 (dove dorme una corsa sospesa), e i due dati che non si
>   possono inventare: come `agy` e `gemini` dicono di aver finito la quota.
> - *01/09/2026 — il 18 è ora **chiuso in parte**: l'ambiente del profilo
>   arriva a ogni motore invocato da un passo, sotto ciò che il passo dichiara.
>   Resta scoperto ciò che la cura chiedeva per primo, cioè una dotazione che
>   viaggi col prodotto: `~/.config/sailor/equipment/` non è versionata e
>   nessun codice la legge.*

**31/08/2026.** Cinque passi, 142 turni, 12,5639 $ equivalenti. Il passo di
verifica ha aperto le fonti e il codice e ha detto **approvato**, con quattro
riserve. Questo documento tiene il risultato; il verdetto integrale e le riserve
stanno in fondo, perché una ricerca approvata con riserve non è una ricerca
approvata.

## La domanda

Cinque cose che sembravano diverse e sono la stessa: **dove gira un flusso**, i
**profili** (esistono come crate e nessun motore invocato da un flusso li usa),
il **routing** (quale motore, quale modello, per quale passo), i **modelli** (il
catalogo non porta fornitore né quota residua) e le **CLI** (tre abbonamenti
della stessa persona, nessuna fascia gratuita usata).

Il punto A le tiene insieme. Oggi ogni passo avvia un processo nuovo, che
riscopre il repository da capo. L'alternativa è che il flusso **descriva** e a
eseguirlo sia l'agente già vivo nel terminale. Ma allora sparisce il punto in
cui Sailor scrive nel proprio deposito, e senza quello non c'è né tela né conto.

## La risposta, in una riga

**Nessuno tiene insieme le due metà.** Chi esegue a caldo non registra; chi
registra paga il processo. Vale per i dodici articoli aperti e per tutti i
programmi vivi guardati (Claude Code, goose, Codex, Roo Code, Cline, Amp).

Il caso più netto è dichiarato da chi l'ha costruito: le squadre di agenti di
Claude Code hanno esattamente il registro che servirebbe — lista di compiti con
lock, cassette postali, ganci a ogni compito creato e finito — e la loro pagina
dei costi dichiara **circa 7 volte** i token di una sessione normale, «perché
ogni compagno mantiene la propria finestra di contesto ed è un'istanza
separata». È il nostro 2,79x, ammesso da chi lo paga.

Quindi il montaggio che segue è **nostro**, e va provato con un A/B della stessa
forma di quello del 31/08 — non dato per buono.

## Le nove cose da fare

Ordinate come la sintesi le ha ordinate: prima quelle che mettono il fondo sotto
le misure, poi quelle che muovono i numeri.

### 1. Dichiarare il tetto di spesa su tutti i flussi

Non costa una riga di codice. `spend_cap_micros` esiste in
`crates/flow/src/file.rs:62`, `SpendStop` e `how_many_fit` lo controllano prima
di aprire ogni fronte, e una corsa fermata esce `cap_reached` e non `failed`.
**Nessun flusso lo dichiara**, quindi `Decision::CapReached` è stato eseguito
zero volte: un tetto mai scattato non è provato.

E i flussi **non sono sette**: `sailor flow list` ne mostra **tredici** — 5 del
progetto, 6 nella casa dell'utente, 2 incorporati nel binario. Sette è quanti ne
tiene git. Sei stanno **fuori dal controllo di versione** e due **dentro il
binario**, dove un tetto scritto da una persona può esistere solo come file
omonimo in casa. «Dichiarare il tetto su tutti i flussi dell'albero» non è
raggiungibile modificando l'albero: è la scoperta che cambia questa voce.

Finché non c'è, i 3,9418 $ e gli 1,4102 $ dell'A/B non sono confrontabili fra
corse successive, e ogni misura delle voci seguenti poggia su un fondo che può
scorrere.

*Numero da muovere:* `Decision::CapReached` da 0 esecuzioni ad almeno 1 su una
corsa deliberatamente stretta.

**E la taratura proposta era pericolosa.** «Costo mediano registrato più il
50%» non si può calcolare: nel deposito vero **6 corse su 34 hanno un costo
diverso da zero**, e nessun flusso ne ha tre — non c'è nessuna mediana da
estrarre. Peggio: i 28 zeri non sono misure, sono il guasto 22 (il costo era la
costante zero fino al 30/08). Una mediana su quella colonna darebbe **zero per
ogni flusso**, cioè `Some(0)`, cioè ogni flusso si ferma prima del primo passo —
di notte. E anche col dato buono la regola sbaglierebbe verso: su
`come-lo-risolvono-gli-altri` la mediana dei due campioni più il 50% fa 9,47 $,
e **avrebbe ucciso l'unica corsa completa e riuscita di quel flusso**, costata
12,56 $. Una mediana taglia il fratello grande della corsa normale; il tetto
serve contro la corsa impazzita.

Al suo posto: il tetto **si dichiara**, e Sailor al massimo suggerisce — con
almeno tre corse costate, `peggiore osservata + chiamata più cara osservata`, e
sotto le tre **rifiuta di suggerire** dicendo cosa c'è. Un numero calcolato su
un campione è un dato inventato con la faccia di una misura.

### 2. La porta che manca alla cerniera che c'è già

Un'azione che ritorna `ActionOutcome::Waiting`, più i comandi `sailor step
open|close` che scrivono nel deposito **da fuori**, portando corsa e passo.

È la risposta al punto A, e nessuna delle due ricerche esterne l'ha trovata
perché il pezzo difficile è dentro Sailor e non fuori. `crates/flow/src/executor.rs`
ha già `EffectStatus` (65), `ActionOutcome::Waiting` (76), `inspect_effect`
(104) e `reconcile` (527): Sailor **sa già** aprire un passo, consegnarlo a un
processo che non ha avviato, e chiuderlo quando l'effetto compare. Il buco è
altrove: `COMMANDS` in `crates/sailor/src/main.rs:36-45` elenca otto comandi e
nessuno apre o chiude un passo.

L'intenzione si scrive prima, l'esito dopo. Così **il registro per passo smette
di dipendere dall'aver generato il processo**, che è esattamente la tensione che
nessuno ha sciolto.

*Numero da muovere:* i 62 turni verso i 30 — cioè il 2,07x verso 1,0-1,2x, che
per costruzione porta con sé il 2,79x e il 2,29x.

### 3. Il modello per passo, risolto dal descrittore

`model` (e poi `effort`) come campo di `EngineSpec`, risolto attraverso la
capacità `choose_model` dentro `command_line(&recipe)` — **mai accodato a mano
agli argomenti**.

Oggi `crates/actions/src/lib.rs` mette `declared_usage: None` quando il passo si
scrive gli argomenti da sé, e cinque blocchi `args` sono già nei flussi
(`smista-il-lavoro` due volte, `sviluppa-sailor` tre). Quei passi **perdono la
misura in silenzio**. `choose_model` è già dichiarato `{"args": ["--model"],
"takes_value": true}` per tutti e quattro i motori: il modello per passo non
richiede un formato nuovo, richiede che la ricetta continui a dettare la riga.

*Numero da muovere:* i passi che azzerano `declared_usage`, da 2 (o 5 contando
ogni blocco) a 0, senza toccare `with.env`.

### 4. Vaglio a secco della catena prima di spendere

Estendere `sailor flow check` a montare la riga di comando di ogni motore di
ogni catena e a provarla nella sua forma innocua. `capabilities_wanted` (457) e
`capabilities_into` (511) fanno già metà del lavoro e distinguono «dichiara di
non averla» da «nessuno ha guardato»; manca il pezzo che monta gli argomenti
veri e li guarda.

È la sola voce che muove il metro dichiarato — quanto costa arrivare a un lavoro
accettato — invece del costo di un giro: una catena di tre motori mai provata
sarebbe caduta a costo zero.

### 5. Il profilo risolto fuori dal passo e appuntato alla corsa

`crates/actions/Cargo.toml` dipende da `flow`, `ledger`, `models`, `serde`,
`serde_json` — **`profiles` non c'è**. Il meccanismo però esiste già intero:
`profiles::build_environment` produce la sovrapposizione d'ambiente, che è
esattamente ciò che oggi un flusso deve scriversi a mano in `with.env`,
legandosi alla variabile di un fornitore.

Il valore non è la comodità ma la **confrontabilità**: Roo Code appunta il
profilo alla corsa e lo tiene anche quando l'utente cambia la scelta globale.
Senza quello due corse dello stesso flusso non sono la stessa misura. Le due
colonne dove scriverlo — `mandate_name`, `mandate_version` — sono già nel
deposito, vuote.

### 6. Il giudice cieco imposto dal contenitore

Oggi «chi crea non giudica» regge per un caso fortunato: il passo `verifica` di
`sviluppa-sailor` è un processo a sé, quindi della trascrizione di `implementa`
non sa niente. **È isolamento comprato con un avvio.** Nel momento in cui la
voce 2 toglie gli avvii, quel passo è il primo che non deve seguire il flusso
dentro la sessione calda — o si perde la sola cosa che il flusso ha vinto
nell'A/B: il giudice cieco che ha scelto la sua riparazione senza sapere quale
fosse quale.

Il descrittore dichiara già `fork_session` come `--fork-session` per
claude-code, `exec fork` per codex e `false` per agy: misurato e assente.

### 7. Quota residua letta prima della chiamata

Oggi `Ask.unusable_when` si legge dentro il ramo di errore d'uscita, cioè **dopo
aver speso**. Il punto dove non si spende è cinquanta righe più su ed è già
costruito: `candidates` produce già `Refused { id, reason, unresolved: false }`.

I canali esistono e sono letture dirette, coerenti con la decisione del 30/08:
l'oggetto `rate_limits` che la sessione passa alla propria barra di stato
(percentuale della finestra di cinque ore e di sette giorni, e quando si
azzera), le intestazioni di quota residua che l'API restituisce a ogni risposta,
`GET /api/v1/key` di OpenRouter, e i file locali del proprio consumo.

Con un fatto tariffario che ribalta la lettura del conto: **la cache letta non
conta verso il limite di token in ingresso, la cache scritta sì.** Il 92,3% dei
token di Sailor non consuma quota; il 44,8% del costo sì.

### 8. Un vocabolario chiuso di stati e di esiti

`pending | in_progress | completed | failed` come secondo canale accanto ai byte
di `LiveSink` — accanto, non al posto: i byte grezzi sono una scelta dichiarata.
E una forma di ritorno obbligatoria del sotto-agente: **o un artefatto o un
rifiuto tipizzato** (contesto mancante, compito ambiguo, fuori portata, motore
sbagliato).

Sono la stessa cosa vista da due lati. La tela ha bisogno di qualcosa che cambi
durante il lavoro; il deposito ha bisogno che «non ha prodotto niente di
usabile» sia un esito registrato invece di un artefatto vuoto che il passo dopo
consuma.

### 9. Il catalogo che decide — ma prima una domanda a Theo

`crates/models/src/config.rs:75-88` impone in scrittura **e in lettura** che si
possa scegliere solo un modello gratuito, con ripiego cablato su
`DEFAULT_FREE_MODEL`. Finché quella regola vale, un catalogo che sa instradare
«questo passo merita il modello caro» non ha dove atterrare.

È una decisione del 27/08 e non si tocca da soli. Va nominata perché rende
inutile l'allargamento fatto prima.

Sui campi: scaricando i dati il 31/08 si è misurato che **nessun catalogo
pubblico porta quota residua né patto sui dati**. models.dev dà fornitore,
famiglia, prezzi e limiti — zero occorrenze di «quota», «rate_limit»,
«data_policy». OpenRouter aggiunge salute e prezzo per endpoint, e 18 modelli
col suffisso `:free`. Quindi metà si prende da fuori e metà sono colonne di
Sailor alimentate dai canali della voce 7.

## Le dieci cose da non fare

Questa è la parte che vale di più: sono strade che sembrano buone e non lo sono.

| Proposta | Perché no |
|---|---|
| **ACP** al posto di `invoke_external_engine` | Cura un male che Sailor non ha: le differenze fra motori sono già un dato nel descrittore, e il codice non conosce il nome di nessuno strumento. In cambio toglie due cose: nel rapporto di una chiamata non c'è nessun campo di consumo, e l'intero deposito pende da `Usage`; e via ACP «l'agente esterno di solito possiede la propria scelta del modello», cioè il contrario della voce 3. Si prende la forma degli eventi (voce 8), non il protocollo. |
| **Router appreso per turno** | Morto all'avvio a freddo, e il deposito non ha traiettorie etichettate. Presuppone che cambiare modello a metà sia gratuito: falso, misurato. E goose aveva lead/worker per turno e **l'ha rimosso**, sostituendolo con un confine di fase. |
| **Condividere la sessione fra i passi** (`--resume`) | Misurato: una chiamata ripresa dichiara `num_turns: 1`, cioè il consumo della singola invocazione — condividere la sessione **non toglie** il conto passo per passo, quindi non compra la registrazione. E costa: ogni ripresa riscrive la trascrizione e la cache scritta cresce, sulla voce che è già il 44,8%. **Sessione calda dentro un passo, artefatto fra i passi.** |
| **Le agent teams come sede del registro** | Hanno il registro giusto e costano circa 7x. Sarebbe pagare tre volte il 2,79x per curare il 2,79x. |
| **Qualunque intermediario multi-fornitore** | Deciso il 30/08: si fa in casa. Da dichiarare: la pagina dei costi di Claude Code raccomanda esplicitamente un gateway di terzi e nomina LiteLLM. Non lo adottiamo, e tutti i canali delle voci 7 e 9 sono letture dirette. |
| **`SKILL.md` come formato del passo** | Un passo di Sailor è un nodo di un grafo validato, con dipendenze ed effetto ispezionabile — non un documento che una sessione carica quando ne ha voglia; e un passo scelto dal modello non è un passo. **Si prende la tabella dei campi** (modello, sforzo, portata degli strumenti, isolamento) e si mette in `EngineSpec`. Va detto che il frontmatter non scrive niente da nessuna parte: è metà del problema del punto A. |
| **Firma crittografica dei salti di delega** | Nel programma si scrive solo ciò che tocca il mondo. L'invariante «chi crea non giudica» si impone molto più a buon mercato col contenitore (voce 6), che è imposizione e non verifica postuma. E il valore dello schema viene da identità distinte con chiavi distinte — che è proprio ciò che collassa quando un solo agente caldo firma quattro salti. |
| **Un servizio di contesto del repository per commit** | Motivo aritmetico: agisce sui token **letti** per turno, e la cache letta è il 92,3% dei token a 0,50 $/M — la voce economica. Il 44,8% del costo è cache scritta a 10 $/M, che l'indice non tocca. Il fattore turni vale 2,07x e si prende con la voce 2. |
| **Affinamento supervisionato dei sotto-agenti** | La parte che rende credibile l'astensione è quella che non si può comprare: non c'è traffico etichettato, e i motori sono CLI di terzi che non si affinano. Si prende il protocollo nel prompt (voce 8), non la calibrazione. |
| **Un ricevitore OTLP come colonna del deposito** | Sorgente eccellente, architettura sbagliata. `actions::recording_for` rifiuta di scrivere una riga senza corsa e passo, e ha ragione: un flusso di eventi che nomina `skill.name` non sa niente della corsa di Sailor. **Va portata la chiave, non l'impianto.** In più quegli attributi dipendono da impostazioni dei vicini: senza quelle il flusso c'è ed è anonimo — il guasto 18 travestito da soluzione. |

## Quello che non copre nessuno

Vale più del resto, perché è dove non c'è niente da copiare.

**Il guasto 25 — il workspace mancante.** Nessuna pratica, dentro o fuori,
costruisce la nozione di «questo flusso gira su un clone qualunque». Le due che
lo rivendicavano non lo fanno: le coordinate relative al commit stanno dentro
una proposta rifiutata, e «il passo eseguito dall'agente lavora nel `cwd` di chi
lo ospita» **sposta** il percorso assoluto dal file di flusso alla sessione, non
lo elimina. È il guasto più vecchio della lista e resta interamente scoperto.

**Il guasto 18 — la dotazione presa dai vicini.** Qui le pratiche non solo non
impediscono il guasto: **lo aggravano**. Ogni canale proposto per sapere quanto
resta della quota è una lettura da casa d'altri — la barra di stato di Claude
Code, i file locali del consumo, `~/.codex/config.toml`, models.dev, OpenRouter.
Nessuna fonte aperta dice come si possiede una dotazione propria invece di
dedurla.

**Il guasto 27 — la classe, non l'istanza.** Il vaglio a secco prende quella
istanza, ma due blocchi di un descrittore corretti separatamente e incompatibili
insieme si vedono **solo montandoli contro il motore vero**. Il guasto 1 è
tornato da un'altra porta una volta; le porte non sono finite.

**Il guasto 14 — dove si posa una corsa sospesa.** Nessuna delle ventinove
pratiche dice dove si ferma una corsa che alle 23 ha esaurito la quota e
potrebbe riprendere alle 7. Il momento del reset dice *quando*, non *dove*.

**Per codex e le CLI a forfait non esiste nessun canale** equivalente a
`rate_limits`: sapere che sono esaurite prima di chiamarle, oggi, si può solo
indovinandolo dal proprio consumo passato. La voce 7 vale per claude-code e
OpenRouter; sugli altri due dei tre abbonamenti resta `unusable_when`, cioè
scoprirlo sbattendoci contro.

**Il montaggio della voce 2 è nostro**, e porta il suo rischio: un passo che
l'agente dimentica di chiudere **resta `Waiting` per sempre**, e `reconcile` non
ha un processo da interrogare quando nessuno è stato avviato.

## Il verdetto della verifica, e le quattro riserve

**Approvato.** Il verificatore ha interrogato l'archivio su tutti e undici gli
identificatori citati — esistono tutti, con titolo e autori come dichiarato — e
sui cinque che portano i numeri decisivi ha letto il testo intero: i numeri
combaciano parola per parola. Ha scaricato tre pagine di documentazione di
prodotto e verificato i campi citati. E ha aperto il codice per ogni
affermazione su Sailor: dipendenze, righe, nomi, conteggi. Regge tutto.

Le riserve, in ordine di peso:

1. **Il guasto 28 non è nominato, e serviva.** È il perno delle voci 2 e 8.
   *Con una correzione al verificatore stesso, trovata mentre si costruiva il
   piano della voce 2*: la conseguenza scritta nella riga 28 — «il deposito non
   può ricevere niente da un passo precedente» — **è falsa da `a11902b`**.
   `store_write`, `store_read` e `store_list` risolvono i rinvii; restano fuori
   `history_ask` e `detect_tools`. Il guasto **resta aperto** perché la cura
   chiesta non è stata fatta: i punti di chiamata sono passati da due a sei,
   cioè il difetto è stato riparato copia per copia invece che alla radice. E
   la riga ha affermato il falso per un giorno **senza che nulla diventasse
   rosso**, perché la prova che sorveglia la tabella controlla struttura e
   conteggi, non verità.
2. **Il 7x delle agent teams è citato senza la sua condizione**: la pagina dice
   «quando i compagni girano in modo piano». L'argomento «pagare tre volte il
   2,79x» è sovrastimato, anche se il rifiuto regge lo stesso sulla riga «ogni
   compagno è un'istanza separata».
3. **«11 flussi» contro i sette veri.** La sintesi corregge e verifica, ma il
   numero sbagliato resta nel materiale a monte.
4. **`catches 30` ripetuto in cinque voci gonfia il valore apparente del
   piano**: il guasto 30 risulta chiuso il 31/08 con sette mutanti. Le voci lo
   nominano al passato e quindi non mentono, ma il conteggio inganna.

Nota collegata del verificatore: **la voce 1, la più economica, consiste nel
modificare a mano sette file di flusso** — che è esattamente il guasto 15 aperto
(«ogni cosa che una persona deve fare su un flusso è un comando di Sailor»).

## Il costo di questa ricerca

| | |
|---|---|
| passi | 5 su 5, nessuno rotto |
| turni | 142 |
| token | 142 in ingresso · 144.541 in uscita · 6.606.258 letti da cache · 564.649 scritti in cache |
| costo equivalente | 12,5639 $ (quanto sarebbe costato via API, non una spesa) |

La forma del consumo conferma la misura del 31/08: **l'ingresso nuovo è
irrilevante** (142 token in tutto), quasi tutto è cache letta, e il costo vive
nella cache scritta.
