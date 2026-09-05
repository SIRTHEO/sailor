# I workflow degli altri e i flussi di Sailor

**05/09/2026.** Theo: *«Claude e altri agenti hanno i workflow che praticamente
rispecchiano un po' i nostri, ma noi definiamo processi stabili e consolidati».*
Questo documento mette le due cose una accanto all'altra, parola per parola, e
dice dove Sailor è già più forte, dove è più debole, e quali tre gesti chiudono
la distanza. Quel che è misurato su questo albero porta il file che lo decide.

## In una riga

I workflow di Claude Code (e i «custom prompts» di Codex, le estensioni di
Gemini CLI) sono **programmi di una sessione**: nascono dentro una conversazione,
vivono quanto lei, e ciò che producono lo ricorda chi li ha lanciati. I flussi
di Sailor sono **file dichiarati, senza fornitore, con un registro**: si
eseguono da tre sedie (finestra, riga di comando, battito), ogni passo lascia
una riga nel ledger con costo e modello, e un flusso che non sta in piedi non
si salva nemmeno. Le primitive si corrispondono quasi una a una; la differenza
è dove stanno e chi le ricorda.

## Il vocabolario, uno accanto all'altro

| concetto | Claude Code | Sailor | dove sta in Sailor |
|---|---|---|---|
| l'unità di lavoro | `agent(prompt, {schema, label, phase})` in uno script `Workflow` | un **passo** `external_engine` con `stdin`, `answer_shape`, `kind`, `tool` (catena) | `crates/flow/src/graph.rs`, `crates/actions/src/lib.rs` |
| la forma della risposta | `schema` (JSON Schema) sull'agente | `answer_shape` nel `with`, ricopiata in `output_schema` del passo | ogni `*.flow.json` |
| in parallelo | `parallel([...])` | passi con le stesse `deps`: l'esecutore apre un **fronte** alla volta, con un tetto | `executor.rs`, `std::thread::scope` |
| in cascata | `pipeline(items, stage1, stage2)` | `deps` tra passi; `{"$from": "/passo/campo"}` porta l'uscita di uno nell'ingresso dell'altro | `graph.rs`, il validatore `accepts` |
| fasi | `phase('Review')` nel `meta` | `phase` facoltativo sul passo, disegnato dalla finestra | `graph.rs`, `StepNode.tsx` |
| un sotto-programma | `Agent` (subagente) o `Skill` | `subflow` — un flusso che ne esegue un altro, con il registro che lega il figlio al passo | `crates/flow/src/subflow.rs` |
| consegnare a una persona | `AskUserQuestion` (blocca la sessione) | `handed_to_agent`: il passo aspetta chi è vivo nel terminale, con opzioni chiuse e un tempo; la corsa resta «in attesa di una persona» nel ledger | `crates/actions/src/handoff.rs` |
| quando parte | `/loop`, `CronCreate`, routine | `schedule` nel file (`every_seconds`, `daily_at`, peso) letto dal **battito** della finestra e da `sailor flow tick` | `crates/flow/src/schedule.rs`, `desktop/src-tauri/src/beat.rs` |
| al partire, al fermarsi | hooks `SessionStart`, `Stop`, `PreToolUse` | `sailor session open/event/close` innestati nelle CLI dal binario, uguali per ogni motore | `crates/sailor/src/session_cmd.rs` |
| chi lavora a cosa | agent teams (`~/.claude/teams`), solo dentro Claude | **annunci** nel deposito `work-claims`, con i nomi OpenTelemetry e gli stati A2A; `work_survey` li legge da qualunque motore | `crates/actions/src/presence.rs` |
| memoria | `MEMORY.md` per progetto, file per fatto | la collezione `memories` del deposito, la pagina `state/memory.md`, `sailor search` (FTS5 su flussi, corse, deposito, eventi, guasti), e `consolidate-memories` una volta al giorno | `crates/actions/src/memory.rs`, `crates/ledger/src/search.rs` |
| quale modello | il modello della sessione, o `model:` sul subagente | **la tabella delle forze per `kind`**, e il carburante che resta: nessun nome di fornitore nel codice | `crates/models`, giudice `no_engine_is_named_in_the_code` |
| il costo | non misurato per passo | ogni chiamata ha una riga (`model_calls`) e ogni flusso un tetto di spesa (`spend_cap_micros`) | `crates/ledger`, `sailor flow cap` |
| quando qualcosa si rompe | il testo dell'errore nella conversazione | l'evento nel ledger, e `write-down-what-broke` che scrive la riga nel registro dei guasti | `crates/flow/system/write-down-what-broke.flow.json` |
| dove vive | `.claude/workflows/`, `~/.claude/workflows/` | `flows/` del progetto, `~/.config/sailor/flows/`, e i flussi di sistema **dentro il binario** | `crates/flow/src/system.rs`, `FLOWS` |

Codex e Gemini CLI hanno meno di tutto questo: prompt personalizzati e
`AGENTS.md` da una parte, comandi ed estensioni dall'altra; nessuno dei due ha
un grafo, un registro o un orologio. Per loro Sailor non «rispecchia» niente:
**aggiunge** il processo che non hanno.

## Dove Sailor è già più forte

1. **Il processo è un file che si valida prima di esistere.** Un workflow di
   Claude è JavaScript: si scopre che è sbagliato eseguendolo. Un flusso di
   Sailor con un ciclo, una dipendenza mancante, un'azione che il motore non
   ha, o uno schema che non accetta l'uscita del passo prima **non si salva**
   (`Graph::validate`, `flow_draft`, `save_flow`). Misurato oggi sul flusso
   `draft-a-flow`: il primo tentativo aveva un `input_schema` che non
   accettava l'uscita del passo `author`, e il caricamento lo ha rifiutato con
   il nome del passo.
2. **Si ricorda.** Ogni corsa, passo, chiamata e costo è una riga; la finestra
   la mostra, `sailor flow cost` la somma, `history_ask` la rilegge da un
   flusso. Un workflow di Claude sa quanto è costato solo se qualcuno guarda
   la fattura.
3. **Non appartiene a un fornitore.** Lo stesso file corre su Claude, Codex,
   Gemini e il modello locale, con la catena `tool` che decide chi risponde e
   il carburante che decide se; un giudice tiene il codice pulito da ogni
   nome. Un workflow di Claude Code corre su Claude Code.
4. **Le tre sedie.** Finestra, riga di comando e battito eseguono lo stesso
   flusso con lo stesso registro; il `Workflow` di Claude esiste solo dentro
   la sessione che lo ha scritto.

## Dove Sailor è più debole, e cosa costa

1. **Il fan-out dinamico.** `pipeline(findings, f => verify(f))` apre un agente
   per ogni risultato del passo prima, quanti che siano. Un flusso di Sailor
   ha tanti passi quanti sono scritti nel file: un passo può ricevere una
   lista, non diventare una lista di passi. La via che c'è oggi è `subflow`
   dentro un motore che decide quanti figli aprire — ma è il motore a
   deciderlo, non il grafo. *Mancava:* un passo `for_each` che apra un figlio
   per elemento, con il tetto del fronte già esistente. C'è dalla stessa notte
   (sotto, gesto 2).
2. **Le fasi come cosa detta.** `phases` nel `meta` dà alla persona il nome del
   momento in cui si trova; il nostro grafo lo sa ma non lo dice. *Manca poco:*
   un campo facoltativo `phase` sul passo, letto dalla finestra e da
   `RunConsole`, niente altro.
3. **L'attesa è più costosa.** `handed_to_agent` con 86 400 secondi lascia la
   corsa «in attesa di una persona» per un giorno, e il saluto di ogni
   terminale la nomina finché qualcuno non la riprende. Giusto — è così che
   una consegna non si perde — ma la forma della domanda è più rigida di
   `AskUserQuestion`: opzioni chiuse, niente anteprime. *Non è un difetto da
   correggere oggi:* l'attesa nel ledger è ciò che rende la consegna ripresa
   da un altro terminale, un altro giorno.
4. **Scriverli.** Un workflow lo si scrive parlando con l'agente che poi lo
   esegue; un flusso lo si scriveva a mano in JSON o nodo per nodo nella
   finestra. Da oggi c'è `draft-a-flow`: la lavagna — blocchi, parole, frecce —
   diventa un file che sta in piedi, e la finestra lo apre accanto agli
   altri. È la risposta a questa riga, e va misurata su uno schizzo vero.

## I tre gesti che chiudono la distanza

1. **`draft-a-flow` prende anche uno script `Workflow`.** Il flusso di bozza
   riceve uno schizzo; uno script JavaScript di Claude Code è uno schizzo
   più preciso — `agent(...)` è un passo `external_engine`, `schema` è
   `answer_shape`, `parallel` sono passi con le stesse `deps`, `pipeline` è una
   catena di `deps`, `phase` è il futuro campo `phase`. Nessun codice nuovo: il
   mandato del passo `author` deve dire che uno script è uno schizzo
   ammesso e come si legge. Chi ha un workflow che funziona lo consolida in un
   flusso con un gesto, e da lì corre su ogni motore con un registro. Fatto: il
   mandato del passo `author` in `crates/flow/system/draft-a-flow.flow.json` ha
   la sezione «IF THE SKETCH IS A SCRIPT» con le quattro corrispondenze, e
   `crates/sailor/tests/draft_a_flow.rs` prova che ci siano. Il campo `phase`
   è arrivato la stessa notte (sotto); il mandato dell'autore va aggiornato per
   usarlo invece dell'id.
2. **`for_each`.** *Fatto la stessa notte:* `crates/flow/src/for_each.rs`.
   Il passo riceve `items` (una lista, o un `$from` che il risolutore ha già
   sostituito) e `flow`; apre un figlio per elemento a gruppi larghi quanto il
   fronte dell'esecutore, ogni figlio è una corsa `subflow` nel ledger legata
   al passo, e l'uscita è `{"items": [...]}` nell'ordine degli elementi. Lista
   vuota, nessun figlio; un figlio che fallisce fa fallire il passo con
   l'indice dell'elemento. `Graph::validate` rifiuta un `for_each` senza
   `flow` o senza `items`. Dieci prove, ognuna col suo mutante; uno dei
   mutanti ha trovato un difetto vero (l'offset dei gruppi) prima del commit.
   Era l'unica primitiva di Claude che non avevamo: la revisione a più
   dimensioni con una verifica per risultato ora si scrive in un file.
3. **`phase`.** *Fatto la stessa notte:* `Step.phase` facoltativo in
   `crates/flow/src/graph.rs`, `sailor flow check` lo stampa accanto al passo,
   la finestra lo disegna sopra il nome del nodo (`StepNode.tsx`,
   `.step-node__phase`); prove sul file, sul rapporto del check e sul nodo.
   Nessun flusso spedito lo usa ancora.

## Cosa non va copiato

- **Il router «all'89 %»** di ruflo e simili: chi decide quale agente parte è
  un modello, e nessuno può dire perché ha sbagliato. In Sailor decide la
  tabella delle forze per `kind` e il carburante, e la ragione è una riga.
- **Il worker residente** di claude-mem: un processo sempre acceso per la
  memoria. Il vincolo di Sailor è girare su macchine accese per anni senza
  farsi notare; l'indice FTS5 si ricostruisce in memoria alla domanda e costa
  meno di tenerne uno fresco.
- **La memoria per prompt** («ricorda di…» nel testo di sistema): non si
  misura, non si revoca. Ciò che Sailor ricorda sta nel ledger con una
  provenienza, o non è ricordato.
