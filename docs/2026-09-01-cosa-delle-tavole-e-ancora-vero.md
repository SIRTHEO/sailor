# Cosa delle tavole è ancora vero

**01/09/2026.** Censimento, non costruzione: nessun file di prodotto è stato
toccato. Nasce da un ordine di Theo — *«basati sui file recenti; non implementare
nulla che potrebbe essere deprecato già da altre scelte»* — e dal fatto che
esistono **due disegni della stessa finestra, approvati a cinque giorni di
distanza, e nessuno dei due nomina l'altro**.

- **A — le tre tavole del 27/08/2026**: `design/Main.dc.html`,
  `design/Agente.dc.html`, `design/Codice.dc.html`, con `design/canvas.json` e
  le immagini in `design/rendered/`. Versionate; commesse il 28/08 alle 12:53
  con *«il flusso dichiarato arriva fino in fondo»*. Il commit del 31/08 20:43
  *«via i bozzetti: l'unica interfaccia è la finestra»* ha cancellato i
  *bozzetti* (`design/direzioni/`), **non** queste tre.
- **B — la bozza approvata il 01/09/2026**:
  `~/personal/.sailor-work/bozza-la-finestra-di-sailor.html`, sette sezioni.
  Non versionata, e il mockup vive dentro il suo script.

**Come si legge un verdetto.** Ogni elemento distinguibile ha una riga e una
prova. Quattro verdetti:

1. **GIÀ COSTRUITO** — esiste nel prodotto oggi, con file e simbolo.
2. **SUPERATO** — una decisione o una richiesta successiva l'ha cambiato o
   eliminato, con la fonte e la data.
3. **NON RETTO DAI DATI** — il disegno lo mostra, il motore non ha il concetto:
   sarebbe un bottone che non salva o una porta che nessun filo può riempire.
   È **lavoro di motore**, non di disegno.
4. **VIVO E DA COSTRUIRE** — regge alle due domande e non c'è ancora.

**Un avviso che vale per tutta la tabella 3.** «Non retto dai dati» non è un
giudizio sul disegno. Il guasto 41 dice come si paga il contrario: quattro
famiglie su nove erano bottoni che creavano nodi impossibili da salvare, e
nessuna prova lo vedeva. Un elemento del disegno che il motore non regge va
costruito **cominciando dal motore**, o non va costruito.

I conti: **31 già costruiti**, **12 superati**, **25 non retti dai dati**,
**19 vivi e da costruire**.

---

## 1 · GIÀ COSTRUITO

| elemento | in quale disegno | dove sta oggi |
|---|---|---|
| Tela a nodi con disposizione calcolata | A, B | `desktop/src/layout.ts` → `buildUnifiedLayout`; React Flow in `App.tsx` |
| Nodo largo 248 px, testata col genere e lo stato, corpo col nome | B | `desktop/src/StepNode.tsx`, `styles.nodes.css` — *«il nodo porta il tipo nella forma»*, 01/09 13:57 |
| Porte col tipo nella forma (cerchio testo, rombo struttura, quadrato valore), **vuote se scollegate, piene se cablate** | B | `layout.ts::portsOf` + `StepNode.tsx`; misurato nel commit: 137 porte, 111 cablate, 26 vuote |
| Il cablaggio delle porte segue il motore, non un disegno | B | `portsOf` compone gli ingressi come `flow::step_input`: `with` vince, una dipendenza sola passa la propria uscita, più dipendenze passano un oggetto chiavato |
| Cassetta dei passi **dentro la lavagna**, sette attrezzi in tre gruppi, con campo di ricerca | B (in parte) | `desktop/src/Toolbar.tsx` — `TOOL_FAMILIES` (Source / Engines / Governance), `Panel position="top-left"`; *«la cassetta dei passi diventa una barra dentro la lavagna»*, 01/09 13:38 |
| Il dettaglio del nodo cade da lontano (soglia 0,62) | B | `StepNode.tsx`, dichiarato «il prodotto ce l'ha già» dalla bozza stessa |
| Pannello del passo scelto: identificativo | A | `StepEditor.tsx:148` |
| Pannello: **Motore** con la catena in ordine di preferenza | A | `StepEditor.tsx:175-232`; la catena non si perde più al salvataggio (guasto 53, chiuso il 01/09 alle 13:57) |
| Pannello: **Modello** | A | `StepEditor.tsx:252` |
| Pannello: **Tetto tentativi** | A | `StepEditor.tsx:326-333` → `flow::Step::max_attempts` (`crates/flow/src/graph.rs:23`); zero è rifiutato da `GraphError::ZeroAttempts` |
| Pannello: **Ingresso e uscita** come schemi | A | `StepEditor.tsx:348` → `Step::input_schema` / `output_schema`, `flow::ValueSchema` |
| Pannello: **Runs when** (la condizione) | — | `StepEditor.tsx:340` → `Step::when`, `flow::Condition` |
| Pannello: **Dipende da** | A (implicito) | `StepEditor.tsx:364` → `Step::deps` |
| Bottone «Crea un passo nuovo» | A | `Toolbar.tsx` (`onAdd`) e `App.tsx` |
| Bottone **Salva** | A | `flows.rs::save_flow` (comando Tauri) |
| Bottone **Esegui** | A, B | `run.rs::start_run(name, mandate)`, `TriggerNode.tsx` |
| Legenda «come finisce un passo» / gli stati del passo | A | `flow.ts::StepState` — `waiting`, `running`, `went`, `broke`, `capped`, `handed_to_human`; tinte in `StepNode.tsx::STATE_COLOR` |
| Il badge del ritentativo sul nodo («2ª») | A | `flow.ts::StepRun.attempt`, `StepNode.tsx` |
| Consumo per passo: **gettoni e costo sul nodo** | B (celle di collaudo) | `desktop/src/stepusage.ts::StepUsage` (`models`, `inputTokens`, `outputTokens`, `costMicros`, `calls`, `callsWithoutCost`), reso in `StepNode.tsx` |
| Il costo che non si sa non si scrive zero | B | `StepUsage.costMicros: number \| null`; `flow::CostReading::{Nothing, Exact, AtLeast}` e `ui::dashboard::how_the_cost_reads` — decisione 01/09 «un totale con dentro un'incognita si mostra come pavimento» |
| Riga di collaudo con «almeno 1,22 $, e il vero è più alto» | B | `ui::dashboard::how_the_cost_reads` produce esattamente quella frase |
| Tempo trascorso di un passo in corso | A, B | `flow.ts::StepRun.elapsed_secs`; `ui::dashboard::OpenStep.open_for_secs` |
| Cronologia di un passo (cosa è entrato, nel tempo) | B (scheda «Tentativi») | `desktop/src/StepHistory.tsx` + `run.rs::step_history(flow, step, limit)` |
| Console della corsa con un riquadro per passo | A (tab «Corse») | `desktop/src/RunConsole.tsx::panesFromEvents` |
| Tela vuota che insegna il primo gesto | B | `desktop/src/BlankCanvas.tsx` — e ha già i **tre** stati che la bozza chiede a uno solo |
| Flusso rotto col motivo del rifiuto | B | `flow.ts::FlowEntry` = `{state:"broken", broken:{name, reason}}` |
| Il terminale nella finestra: apertura, scrittura, tasti grezzi, ridimensionamento, chiusura, elenco | A (implicito), B (voce «Terminali») | `crates/terminal/` (pty vero, `Terminals::open/list/find/close`), sei comandi in `desktop/src-tauri/src/terminal.rs`, gli eventi `terminal_output` e `terminal_closed`, e `desktop/src/{Terminals,TerminalPane}.tsx` + `terminal.ts` |
| Lo smistamento di ciò che si scrive nel terminale (riga → flusso o → shell) | — | `terminal::routing`, `terminal_submit` → `Submitted::{Command, Flow}` |
| Voce di navigazione «Terminali» come luogo di prima classe | B | `App.tsx:1151-1154`, `:1221` |
| Le famiglie del vocabolario e il registro del motore non possono più divergere | (cura del guasto 41) | `desktop/src-tauri/src/flows.rs` — `the_window_knows_every_action_the_engine_can_run`, `the_window_vocabulary_names_only_actions_the_engine_registers` |
| Il flusso su disco è **un formato solo** per finestra e riga di comando | C (l'idea «grafo e codice sono la stessa cosa») | `crates/flow/src/file.rs::FlowFile` + `desktop/src/flow.ts` |
| Tetto di spesa del flusso, e la corsa che si ferma con una parola sua | — | `FlowFile::spend_cap_micros`, `Decision::CapReached(SpendStop)`; stato `capped` disegnato sul nodo |

---

## 2 · SUPERATO

| elemento | in quale disegno | chi l'ha superato, con la data |
|---|---|---|
| **La cassetta dei passi nella colonna a fianco** | A **e B** | Theo, 31/08 sera: *«la possibilità di creare flussi dovrebbe essere una barra di strumenti dentro la lavagna non nella colonna affianco»*. Costruito il 01/09 alle 13:38, *«la cassetta dei passi diventa una barra dentro la lavagna»*. **Vale anche per la bozza approvata oggi**: il suo `App` mette la cassetta dentro `nav.colonna`, e la richiesta di Theo è arrivata dopo. |
| **Nove famiglie di passo** (Agente, Verifica, Ramo, Attesa, A una persona, Deposito, Sotto-flusso + i due impliciti) | A | Guasto 41, chiuso il 01/09: le famiglie vere sono **sette**. `Toolbar.tsx::TOOL_FAMILIES` ne offre sette in tre gruppi. |
| L'attrezzo **«Attesa»** nella cassetta | A | Guasto 41: nessuna azione registrata vi si risolve; premere il bottone creava un nodo che non si salvava. `flow.ts::DEFAULT_ACTION_FOR_KIND` non ha voce per `wait`. |
| L'attrezzo **«Ramo»** nella cassetta | A | Idem, guasto 41. `branch` è nell'enum `StepKind` e non ha azione. |
| I nomi d'azione del disegno: `pane_until_idle`, `signal_is_gone`, `deposit_write`, `pane_send`, `hand_to_human`, `pane_read` | A, C | Guasto 41: **sei nomi che non esistono in nessun crate**, tolti da `ACTION_KIND` il 01/09. Le azioni vere sono sedici, elencate in `crates/registry/src/lib.rs::default_registry`. |
| `kindOf` che ripiega su «verifica» per un nome sconosciuto | A (i sette nodi `trigger` disegnati come verifiche) | Guasto 41, verso tre: il ripiego silenzioso è stato tolto insieme alla causa. |
| **Tutta la tavola C: il flusso scritto in TypeScript stile Mastra** (`@sailor/core`, `createStep`, `createFlow`, `.then()`, `.branch()`, `.commit()`, `steps/*.ts`) | C | Il formato è `.flow.json` letto da `flow::FlowFile`; non esiste nessun pacchetto `@sailor/*`. E il 31/08 sera (*«via i bozzetti»* alle 20:43, *«via `sailor ui`»* alle 21:18) **l'unica interfaccia è la finestra**: `sailor ui` è stato tolto e i bozzetti alternativi cancellati. Il disegno di C resta valido come *idea* — «grafo e codice sono la stessa cosa» — e il formato che la realizza è il JSON, non un DSL. |
| La scheda **«Codice»** come vista che mostra file sorgente del flusso | C | Stessa fonte: il flusso è un file di dati, e la finestra lo modifica dal pannello. Nessuna delle voci di `docs/da-fare.md` chiede una vista di codice. |
| La tab **«Corse»** come terza vista accanto a Grafo e Codice | A | Sostituita il 31/08 sera dai luoghi «Adesso» (*«la finestra si apre su cosa sta succedendo adesso»*, 20:55, e *«la finestra impara le tre cose che solo la plancia sapeva dire»*, 21:05) e «Cronologia`: `App.tsx::Place` = `now \| history \| flows \| installed \| manual \| terminals`. |
| Le etichette della finestra in italiano | A, B | **«English everywhere»**, 01/09, decisa da Theo: *«ogni messaggio che un utente dello strumento può vedere»* è in inglese. Il prodotto ha già convertito `Toolbar.tsx` e `StepEditor.tsx`; `Terminals.tsx` è ancora in italiano ed è debito, non disegno. Vale per ogni etichetta di tutte e due i disegni: «Passo scelto», «Tetto tentativi», «Cassetta dei passi», «Parla con Sailor». |
| Il tema scuro | B | Superato **dalla bozza stessa**, che lo dichiara escluso: mancano le misure di contrasto sul fondo scuro, e inventarle sarebbe promettere una cosa non misurata. |
| «`~/.claude/settings.json`: 6 ganci su 57 · `staffetta.sh` ritirato» come pannello della vista Codice | C | Il *contenuto* è vivo (`docs/da-fare.md`: Sailor legge e migra la configurazione delle righe di comando), il *posto* no: la vista Codice non esiste più. |

---

## 3 · NON RETTO DAI DATI

Ogni riga qui è **lavoro di motore**. La colonna a destra dice quale tipo manca.

| elemento | in quale disegno | che cosa manca nel motore |
|---|---|---|
| **La specie del passo scelta a mano** (Ripetibile / Compensabile / Da consegnare a una persona, coi tre radio) | A (pannello), B (`specie: compensabile` nella testata) | `flow::Step` ha **otto campi e nessuno è `species`** (`crates/flow/src/graph.rs:8-24`). `StepSpecies` esiste (`record.rs:104-108`) ma **la dichiara l'azione**, non il passo: `Action::species()` (`executor.rs:158`), letta in `species_for` (`executor.rs:1294`). Un flusso non può scriverla in un `.flow.json`. Serve: un campo `species` in `Step` con `deny_unknown_fields` aggiornato, e la regola di chi vince fra passo e azione. |
| **«Se cade adesso», col rimedio scritto prima** (`engine.stop(output.session)`) | A (radio «Compensabile»), B (riquadro intero), C (`async undo(...)`) | `StepSpecies::Compensable` **non ha nessuna implementazione di produzione**: l'unica `fn compensate` di tutto l'albero è una prova (`crates/flow/tests/crash_recovery.rs:141`). E `external_engine::species()` è `HandToHuman` (`crates/actions/src/lib.rs:2912`), non `Compensable`. Il riquadro oggi direbbe la stessa frase su ogni passo. Serve: `Action::compensate` implementata su `external_engine`, e una specie dichiarabile. |
| **Il tetto di tempo del passo** («Limite 20 min») e **la barra del limite** («2:14 di 20:00») | A (pannello), B (barra) | `Step` non ha nessun campo di tempo. `timeout_secs` è un campo **dentro l'ingresso dell'azione** (`EngineSpec`, `actions/src/lib.rs:1501`; `CheckSpec`; `AskSpec` in `mcp.rs`), quindi vive dentro `with` e non risale a nessuna vista. Serve: o un campo sul passo, o che la finestra legga `with.timeout_secs` e lo dichiari come parametro dell'azione invece che come proprietà del passo. |
| **Il ramo di guasto come porta del nodo** | A (attrezzo «Ramo» col filo viola), B (porta `guasto`, «il ramo di guasto porta a `store_read`», «nessun ramo di guasto dichiarato») | **Verificato ancora vero oggi.** `Graph` ha `steps: Vec<Step>` e `skippable_dependencies: BTreeSet<DependencyEdge>`; `DependencyEdge` è `{step, dependency}` e serve **solo** a marcare una dipendenza come saltabile. Non esiste nessun `on_error`, nessuna porta d'errore, nessun arco etichettato. Un `Broke` a monte non soddisfa niente a valle (`executor.rs::dependencies_satisfied`). Serve: un secondo insieme di archi in `Graph`, la sua validazione in `Graph::validate`, e una regola in `decision_from` che lo percorra. |
| **La biforcazione condizionale come nodo con due uscite** («Il prompt è vuoto?» con le porte «vuoto / presa» e «domanda in sospeso») | A, C (`.branch([...])`) | Esiste `Step::when: Option<Condition>` — una **guardia sul singolo passo**, non una biforcazione: `Condition` ha tre varianti (`Equals`, `PointerEquals`, `PointerExists`) e restituisce un booleano che decide se *quel* passo gira. Non c'è nessun nodo con più uscite. Serve: un tipo di passo con uscite multiple, oppure la dichiarazione esplicita che la biforcazione si scrive come due passi con `when` opposti — che è come i flussi veri la scrivono oggi. |
| **Il nodo «Attesa»** («Il pannello è fermo», «Il segnale è sparito») | A | Nessuna azione registrata. `ActionOutcome::Waiting` (`executor.rs:117`) e `Outcome::Waiting` sono **esiti** di un passo, non un passo. Serve: un'azione che aspetti — e prima ancora la decisione su chi tiene il tempo, che `docs/2026-09-01-il-tempo-e-l-ultima-scelta.md` elenca in cinque domande aperte. |
| **«Strumenti chiamati: 9»** | B (contatore del nodo aperto) | `tool_call` non compare **da nessuna parte** in `crates/`. `ModelCallRecord` conta le chiamate **al motore** (`turns`, `ledger/src/lib.rs:303`), non gli strumenti che il motore ha usato dentro di sé. `docs/da-fare.md`, voce 6 del costo, lo dice per esteso: *«`EngineResult` porta stdout e stderr e nient'altro, Sailor non vede le chiamate a strumento del motore»*. Serve: un canale di osservazione del motore che oggi non esiste. |
| **`pid 41822` sul nodo** | A, B | `StepRun.held_by_pid` esiste (`flow.ts:99`, `StepRecord::held_by_pid` a `record.rs:55`) ma si riempie **solo** per i passi consegnati (`handed_to_agent`), e la decisione del 31/08 dice che perfino lì resta vuoto apposta: *«chi tiene un passo consegnato è una scadenza, non un processo»* (guasto 12). Un `external_engine` non registra il pid del figlio. |
| **«pannello `generale-2`»**, il pannello dove l'agente sta | B | Nessun legame fra un passo e un terminale: `terminal::Summary` porta `{id, workspaceRoot, workspaceName, alive, processId}` e nessun `run_id`/`step_id`. E `Terminals` è un registro **solo in memoria** (`session.rs:159-166`): nessuna persistenza. Serve: un campo di legame e un posto dove tenerlo. |
| **Il terminale dal vivo dentro il nodo** | B (la tavola intera), A (mini-terminale nel nodo agente) | Due cose separate, tutte e due mancanti. *(a)* Un `external_engine` gira su **pipe**, non su pty: `run_with_timeout_watched` (`actions/src/lib.rs:212+`) con due fili che drenano stdout e stderr. Il pty di `crates/terminal` apre una **shell in uno spazio di lavoro**, non il processo di un passo. *(b)* Il guscio registra le azioni con `default_registry(&ledger, **None**)` (`desktop/src-tauri/src/run.rs:333`): il sorvegliante `actions::StepSinks` **non è collegato**, quindi oggi il testo di un passo in corso non arriva alla finestra affatto — `RunConsole.tsx:529` scrive letteralmente *«gira; il suo testo comparirà alla chiusura»*. |
| **«Apri nel terminale»** | A, B | Nessuna strada da un passo a un terminale: nessun comando Tauri li lega, e il pid del passo non è registrato. |
| **«Sospendi»** | A, B | Il concetto **non esiste in nessun crate**: né in `flow` (gli esiti sono `Went`, `Broke`, `Waiting`, `Stopped`, `Skipped`, e `Stopped` non si riprende), né in `terminal` (c'è solo `close`), né in `supervisor`. |
| **«Termina»** come gesto sul passo o sulla corsa | A, B | Non c'è nessun comando che fermi una corsa dalla finestra: i sette comandi di `run.rs` sono tutti di lettura tranne `start_run`. Il più vicino è `terminal_close`, che parla di un terminale. |
| **L'innesco «Contesto oltre l'85%»** (`onContextAbove(0.85)`) | A, C | `trigger::descriptor::Kind` ha **due varianti sole**: `Manual` e `Terminal`, e il commento dice che `Terminal` *«oggi si dichiara e non si ascolta»*. Una forma nuova è una variante in più nell'enum, cioè codice. Non esiste nessuna forma «a soglia». |
| **Chi sveglia i flussi dovuti** («prossima sveglia 03:00 · fra 5 h 12 m», l'innesco «ogni notte 03:00») | B, A | Il **dato c'è**: `flow::Schedule { recurrence, weight, perimeter }`, `Recurrence::{EverySeconds, DailyAt}`, `is_due(schedule, last_run, now)`, e `FlowFile::schedule`. Ma **nessuno lo esegue**: `is_due` è chiamato in un solo posto, `crates/sailor/src/flow_cmd.rs:1242`, cioè `sailor flow due`, che *calcola* e basta. La decisione del 29/08 lo dice già: *«serve che qualcuno esegua ciò che `sailor flow due` già calcola. Non ancora costruito.»* Nel disegno il campo mostra un numero che nessun filo può riempire finché quel qualcuno non esiste. |
| **«Permessi: scrive in `~/personal/sailor`, rete negata»** | B (pannello) | Nessun tipo `Surface` esiste in `crates/` — cercato, zero occorrenze. Il modello Bazel è **deciso il 29/08 e dichiarato «non ancora costruito»**; le quattro superfici e i poteri sono decisi il 31/08 con una prova che *«nasce rossa su tutte e nove le azioni di oggi»*. Serve: la dichiarazione sull'azione, e poi la lettura per passo. |
| **«Approvazione: richiesta a una persona, in attesa da 12 m»** come campo del passo | B (pannello) | L'unica forma di consegna è l'**azione** `handed_to_agent`, non una proprietà del passo. Non esiste nessun cancello di approvazione per passo. La decisione del 29/08 sul file delle autorizzazioni dice che non ci sarà un gate speciale: sarà un potere dichiarato. |
| **Le bandierine «salta questo passo» e «congela il risultato»** | B | Nessun campo. La cosa più vicina, `Graph::skippable_dependencies`, è dell'**arco** e non del passo, e non è un interruttore d'editor. Il riuso senza riesecuzione è la voce 9 di `docs/da-fare.md` («cache per hash dell'ingresso»), non costruita. |
| **Il quadratino di modo** (valore fisso / legato a monte / «c'è un legame configurato ma adesso vale il fisso») | B | Il terzo stato **non è rappresentabile**: `with` vince sempre su ciò che arriva dalle dipendenze (`step_input` → `overlay_input`), e non c'è modo di tenere un legame inattivo — metterlo in `with` lo attiva. Serve un posto dove un rinvio possa esistere spento. |
| **Il pallino blu «modificato e non ancora in servizio»** | B | Non esiste nessuna nozione di «in servizio» distinta dal file su disco: `save_flow` riscrive e basta. |
| **«v7» e «modifiche non salvate»** | A | Nessun versionamento del flusso. |
| **«Esegui solo questo passo» / «Esegui fino a qui»** | B (barra del foglio) | L'unico ingresso è `start_run(name, mandate)`: una corsa parte dall'inizio e basta. `Decision` non ha nessuna forma per un sottoinsieme. |
| **La sincronia grafo ↔ codice** («sposta un nodo e cambia una riga») | C | Il `.flow.json` non ha coordinate: `Step` non porta né `x` né `y`, e la disposizione la calcola `buildUnifiedLayout` a ogni apertura. Spostare un nodo non cambia niente su disco, oggi per costruzione. |
| **«Il controllo gira quando il file si carica, non a metà corsa»** applicato a `undo` («ogni passo compensabile ha il suo `undo`») | C | `Graph::validate` rifiuta cicli, dipendenze mancanti, tetti a zero e fusioni distruttive — ma **non** può controllare la compensazione, perché la specie non sta nel grafo. È la stessa lacuna della prima riga di questa tabella. |
| **«9 passi, nessun ciclo, nessuna dipendenza mancante»** come esito mostrato nella finestra | C | Il controllo esiste (`Graph::validate` e `sailor flow check`, che dal 31/08 monta e prova anche le righe di comando), ma non c'è nessun comando Tauri che lo esponga: i quattro comandi dei flussi sono `flows`, `flow_places`, `save_flow`, `delete_flow`. È la voce 10 dei dieci lavori sulla finestra in `docs/da-fare.md`. |

---

## 4 · VIVO E DA COSTRUIRE

| elemento | in quale disegno | cosa serve |
|---|---|---|
| **I tre gesti sul nodo agente** — la parte che regge | A, B | **Distinzione importante, ed è quella che l'ordine chiedeva.** I bottoni `apri / sospendi / termina` **c'erano** in `StepNode.tsx` e sono stati tolti il 01/09 alle 13:57 (*«il nodo porta il tipo nella forma»*) perché non collegati a niente. Non sono superati: sono un bottone tolto perché morto. Di tre, **uno solo** regge oggi — «Termina», che avrebbe `Pty::close` e `terminal_close` dietro se esistesse il legame passo↔terminale; «Sospendi» non regge (tabella 3); «Apri nel terminale» non regge finché il legame non c'è. Serve: prima il legame, poi i due gesti. |
| **«Il passo prima» e «Il passo dopo»**, con esito e durata | B (tavola Agente) | Retto interamente: `Step::deps` dà i predecessori, il grafo i successori, `layout.ts` calcola già gli archi, e `StepRun` porta stato e `elapsed_secs`. Serve: mostrarli nel pannello del passo. Poche ore. |
| **La scheda «Mandato»** — il mandato che il passo ha ricevuto | B | Retto: il prompt sta in `with` sotto `PROMPT_KEY = "prompt"` (`tools.ts:214`), e `StepRecord::input` conserva l'ingresso **come il passo l'ha ricevuto**, con la regola scritta nella decisione del 01/09 sui rinvii. Serve: una scheda che lo mostri come testo invece che come JSON. |
| **La scheda «Ingresso e uscita»** del passo in corsa | B | Retto: `StepRecord::input` / `output`, già trasportati negli eventi `sailor://run`. Serve solo la scheda. |
| **La scheda «Tentativi»** | B | Retto e **già costruito altrove**: `StepHistory.tsx` + `step_history` fanno esattamente questo. Serve: portarlo dentro il pannello del passo aperto. |
| **I contatori gettoni e costo nel pannello del passo aperto** | B | Retto e già calcolato: `stepusage.ts::StepUsage`. Serve: riusarlo nel pannello. |
| **Il testo di un passo in corso, mentre esce** | A (mini-terminale nel nodo), B (riquadro del terminale) | **Il tipo esiste già**: `actions::StepSinks` con `sink_for(step) -> Arc<dyn LiveSink>`, e `registry::default_registry(ledger, watcher)` lo accetta. Il guscio passa `None` (`run.rs:333`). Serve: implementare `StepSinks` in `run.rs` che emetta i pezzi come eventi `sailor://run`, e una prova che nasca rossa perché il testo di un passo vivo non arriva. **Questa è la metà portante di `Agente.dc.html` che si può costruire senza toccare il motore.** |
| **Tira un filo da una porta e lascialo nel vuoto → il menu delle sole azioni compatibili** | B | Retto: `ValueSchema::accepts(produced)` (`crates/flow/src/schema.rs:60`) fa già esattamente questo giudizio, e i tipi delle porte sono già calcolati da `portsOf`. Serve: il gesto, e la mappa tipo→azioni che la bozza chiama `accetta`. |
| **Trascina un attrezzo dalla cassetta sulla tela** | B | Retto: i bottoni della cassetta creano già il nodo (`onAdd`). Serve: il drag&drop e il punto di rilascio. |
| **Carta millimetrata a due passi** (fine 12 px, grosso 96, tutti e due sotto 1,5:1) | B | Foglio di stile. Serve: `styles.css` e una prova di contrasto — `contrast.ts` c'è già. |
| **I due registri dell'attenzione** (categoria quieta per tutte le corse vive, isolamento per una cosa sola per vista, al massimo due elementi animati) | B | Regola di disegno più una guardia: il prodotto ha già `stylesheet.test.ts` e `contrast.test.tsx` come posto dove una regola così diventa rossa. |
| **Il filo che dice dove il flusso è già passato** (inchiostro pieno dietro, filo chiaro davanti) | B | Retto: gli stati dei passi sono in `StepRun`, gli archi in `layout.ts`. Serve: la tinta dell'arco derivata dallo stato del passo di partenza. |
| **I quattro tagli d'angolo sul nodo scelto** | B | Foglio di stile puro. |
| **«Sto leggendo» come scheletro invece che come rotella** | B | Retto: `ask.ts` distingue già i tre esiti (`asking` / `asked` / `mute`). Serve: il disegno dello scheletro. |
| **Il rail degli spazi, col badge «aspettano te»** | B | Retto: `flow::workspace` trova la radice, `flow_places` elenca i posti, `open_runs` sa chi aspetta una persona (stato `handed_to_human`). Serve: la vista, e la scelta di quale spazio è attivo. |
| **La riga di collaudo del foglio** (corse aperte · aspettano te · speso stanotte · prossima sveglia) | B | Tre valori su quattro sono retti e già calcolati: `open_runs`, lo stato `handed_to_human`, `day_summary(since)` con la frase del pavimento. Il quarto — «prossima sveglia» — è in tabella 3 finché non esiste chi sveglia. |
| **`⌘K`: cerca flussi, passi, corse** | B | Retto: `flows()`, `known_runs()`, `step_history` danno tutto il materiale. Serve: la palette. |
| **La barra di navigazione col nome dello spazio e il flusso** (crumb) | A, B | Retto: `flow_places` e `FlowEntry`. Serve: il crumb. |
| **«Parla con Sailor»** | B | Retto solo come superficie: il motore per rispondere esiste (`external_engine`, i descrittori, i profili), e la bozza dichiara la sua conversazione **finta**. Serve: un flusso che risponda, e la regola su cosa può cambiare senza chiedere. È il pezzo più grande di questa tabella dopo il primo, e il più aperto: non è disegno, è prodotto. |
| **Un terminale sopravvive alla finestra** | (contratto del 01/09) | **Il contratto lo pretende e il codice non lo fa**, e i due lati dicono cose diverse: `desktop/src-tauri/src/terminal.rs:47-54` dichiara che il `OnceLock` non viene mai lasciato cadere, quindi `Drop for Terminals` non gira alla chiusura; ma `desktop/src/terminal.ts:147-151` e `Terminals.tsx:4-8` scrivono in prosa che la sessione sopravvive. **Le due metà divergono in prosa, non in codice** — è la forma esatta che il contratto stesso vieta: *«chi lo scopre diverso dal codice apre un guasto invece di adeguare in silenzio la propria metà».** Va aperto un guasto. |

---

## Il pezzo più grande che è vivo, retto dai dati, e non ancora costruito

**È il passo aperto: il pannello che `Agente.dc.html` disegna, alimentato dal
vivo.** Non il terminale dentro il nodo — quello è lavoro di motore, e sta in
tabella 3. Il passo aperto **senza** il pty.

Cosa comprende, e perché è uno solo e non sei: tutti i pezzi leggono la stessa
corsa nello stesso istante, e costruirne uno per volta vuol dire montare sei
volte lo stesso ponte.

| pezzo | cosa legge, che esiste già |
|---|---|
| Il testo che esce mentre esce | `actions::StepSinks` collegato a `registry::default_registry`, emesso come evento `sailor://run` |
| La scheda Mandato | `with["prompt"]`, `StepRecord::input` |
| La scheda Ingresso e uscita | `StepRecord::input` / `output`, `Step::input_schema` / `output_schema` |
| La scheda Tentativi | `step_history(flow, step)`, `StepRun::attempt`, `Step::max_attempts` |
| Gettoni, costo, chiamate | `stepusage.ts::StepUsage` |
| Il tempo trascorso | `StepRun::elapsed_secs` |
| Il passo prima e il passo dopo | `Step::deps` + il grafo, `StepRun` per il loro stato |

**Quanto lavora.** Una giornata e mezza, non tre, e la ragione è che **niente
di questo elenco chiede un tipo nuovo**. Il conto:

- **Rust, mezza giornata.** Un tipo che implementa `actions::StepSinks` in
  `desktop/src-tauri/src/run.rs`, che per ogni pezzo di stdout/stderr emette un
  `RunEvent` (kind nuovo, o `note` con `step_id` valorizzato); e la riga 333 che
  smette di passare `None`. La prova nasce rossa da sola: oggi un passo che gira
  non manda niente, e `RunConsole` lo dice in prosa
  (`«gira; il suo testo comparirà alla chiusura»`) — quella frase è il canarino.
- **TypeScript, una giornata.** Il pannello con quattro schede, i contatori, i
  due vicini. `RunConsole.tsx` ha già `panesFromEvents`, `StepHistory.tsx` ha già
  la cronologia, `stepusage.ts` ha già i numeri: si compone, non si scrive.

**Le tre cose che il disegno mostra e questo cantiere non deve fingere di dare**,
perché sono in tabella 3 e vanno dette invece che disegnate vuote: la barra del
limite di tempo (non c'è un tetto sul passo), «strumenti chiamati» (Sailor non
vede dentro il motore), e «Se cade adesso» (nessuna azione spedita è
compensabile). Disegnare un contenitore per un numero che nessun filo può
riempire è il guasto 41 in un altro vestito.

**Il secondo per grandezza**, e costa molto meno: **i gesti per comporre** — tira
un filo e lascialo nel vuoto, trascina un attrezzo. Reggono in pieno, perché
`ValueSchema::accepts` fa già il giudizio di compatibilità e `portsOf` conosce
già i tipi delle porte; è mezza giornata di finestra e zero di motore. La bozza
lo chiama «ciò che mancava», e ha ragione: senza quello, la tela si guarda e non
si compone.

**Il terzo, e non è disegno**: chi sveglia i flussi dovuti. `Schedule` e `is_due`
sono scritti e provati dal 28/08; l'unico che li interroga è `sailor flow due`,
che calcola e non fa partire niente. Finché quel pezzo non esiste, «prossima
sveglia 03:00» resta una porta che nessun filo può riempire — ed è una decisione
di Theo del 29/08 rimasta aperta, non un lavoro che qualcuno può scegliersi.
