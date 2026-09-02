# Censimento: dove Sailor non è agnostico

Fatto il 01/09/2026 sull'albero principale (ramo `sorgenti`,
`HEAD` `5b68c48`, ~30 file modificati non committati — letti così come stanno).
Nessun file di codice è stato toccato.

**Il criterio.** «Nessun codice di prodotto deve essere specifico» (Theo,
01/09/2026), e la regola operativa che ne discende: *il nome di un prodotto può
comparire in un'etichetta, mai in una condizione*. Qui la condizione include il
**percorso costruito**: un `join(".claude")` non è un'etichetta, è un ramo che
decide dove il prodotto scrive.

---

## 1. In una riga

**34 violazioni vere in 12 file di prodotto.** La peggiore è `crates/inventory`
per intero: le sue radici sono un elenco chiuso compilato che nomina la casa di
un altro prodotto (`~/.claude/...`) **e le cartelle di lavoro di una persona
sola** — `crates/inventory/src/lib.rs:648-671`.
Su una macchina che non è questa, quel crate risponde «non hai niente» senza
sbagliare nessun controllo.

Il secondo fatto della giornata: **il caso noto è confermato ed è più grande di
com'era descritto.** Il mandato parlava di un ramo morto con un nome di fornitore
alle righe 247-251 di `crates/inventory/src/lib.rs`. Il ramo c'è (riga 249), è
davvero morto *oggi* — `~/.claude/skills/mattpocock-skills` non esiste su questa
macchina, verificato con `ls -d` — ma **non è uno solo: sono tre siti**, e due
sono in `discovery.rs`. Il giorno in cui quella cartella comparisse, il ramo
tornerebbe vivo e deciderebbe la raggiungibilità di una competenza.

---

## 2. Le violazioni, per categoria

### Categoria 1 — nome di prodotto in una condizione o in un percorso

| `file:riga` | Cosa c'è | Perché viola | Costo per toglierlo |
|---|---|---|---|
| `crates/inventory/src/discovery.rs:26` | `h.join(".claude/skills/mattpocock-skills/skills"),` | Il nome di un autore terzo dentro un percorso spedito col binario | Una riga, se si accetta di perdere quella sorgente; un descrittore di sorgenti se la si vuole tenere |
| `crates/inventory/src/discovery.rs:95-96` | `if parts.iter().any(\|p\| p == "mattpocock-skills") { return "mattpocock-skills:".to_string(); }` | Condizione su un nome proprio: cambia il prefisso con cui una competenza viene invocata | Due righe, insieme a quella sopra |
| `crates/inventory/src/lib.rs:249` | `} else if on.contains(&plugin) \|\| plugin.contains("mattpocock") {` | **Il caso noto.** Condizione che promuove a `Reach::Active` un plugin che il file dei plugin abilitati non dichiara, per il solo fatto di chiamarsi così | Una riga |
| `crates/inventory/src/discovery.rs:22-32` | `pub fn skill_sources(...)` → quattro `.claude/...` | La mappa di dove un *altro* prodotto carica le cose, compilata dentro Sailor | Rifacimento: va letta da un descrittore, come `toolbox` fa già per gli strumenti |
| `crates/inventory/src/discovery.rs:34-39` | `pub fn agent_sources(...)` → tre `.claude/...` | Stessa cosa per gli agenti | Come sopra, stesso descrittore |
| `crates/inventory/src/discovery.rs:114` | `for path in [h.join(".claude/settings.json"), h.join(".claude.json")]` | Il file di configurazione di un altro prodotto, per percorso | Come sopra |
| `crates/inventory/src/discovery.rs:246` | `let path = h.join(".claude/plugins/installed_plugins.json");` | Idem | Come sopra |
| `crates/inventory/src/lib.rs:348` | `.map(\|h\| h.join(".claude").join("skills").join(&folder).exists())` | Percorso di un altro prodotto in una condizione (`.exists()`) | Una riga, una volta che le radici sono un dato |
| `crates/inventory/src/lib.rs:375` | `let base = root.path.join(".claude").join(folder);` | Idem | Idem |
| `crates/inventory/src/lib.rs:402` | `let base = root.path.join(".claude").join("commands");` | Idem | Idem |
| `crates/inventory/src/lib.rs:434` | `let base = root.path.join(".claude").join("rules");` | Idem | Idem |
| `crates/inventory/src/lib.rs:483` | `let path = root.path.join(".claude").join("settings.json");` | Idem | Idem |
| `crates/inventory/src/lib.rs:684` | `if base.join(".claude").is_dir() {` | «È un repo» è definito come «ha la cartella di un altro prodotto» | Idem |
| `crates/inventory/src/lib.rs:692` | `if path.is_dir() && path.join(".claude").is_dir() {` | Idem | Idem |
| `crates/sailor/src/inventory_cmd.rs:149-152` | `PathBuf::from(home).join(".claude").join("state").join("flussi")` | **Il deposito aperto a mano invece che con `ledger::default_directory()`**, che è esattamente la seconda copia che `crates/ledger/src/lib.rs:82-89` dice di aver già pagato | Una riga: chiamare `ledger::default_directory()` |
| `crates/sailor/src/release_cmd.rs:292` | `.map(\|home\| PathBuf::from(home).join(".claude"))` | Sailor installa il proprio binario dentro la casa di un altro prodotto. `CLAUDE_HOME` lo sposta, ma il predefinito è quello | Una riga per il predefinito; il gesto vero è spostare l'installazione, che tocca i ganci esterni |
| `crates/models/src/store.rs:17` | `PathBuf::from(format!("{home}/.claude/state/modelli.json"))` | La configurazione dei modelli **di Sailor** vive nella casa di un altro prodotto | Una riga: `ledger::sailor_home()` |
| `crates/profiles/src/store_io.rs:22-26` | `.join(".claude").join("state").join("profili.json")` | Idem per i profili | Una riga, stessa cura |
| `crates/ledger/src/lib.rs:56` | `let previous = home.join(".claude/state/flussi");` | Idem per il deposito — **ma è dichiarato come gradino di migrazione** (righe 46-55), con scritto chi lo toglie e come | Una riga, quando il deposito vecchio sarà spostato. Il commento è onesto: conta come debito **datato**, non come sciatteria |
| `crates/models/src/remaining.rs:45` | `const CLAUDE_CREDENTIALS: &str = ".claude/.credentials.json";` | Percorso del fornitore inchiodato | Una riga se il descrittore del motore dichiara dove stanno le sue credenziali |
| `crates/release/src/lib.rs:97-99` | `bin: "claude-hooks", live_rel: "target/release/claude-hooks", safe_rel: "bin/claude-hooks",` | Un bersaglio di rilascio che nomina un binario di un altro prodotto — e che **non è nel workspace**: `Cargo.toml` non ha nessun membro `claude-hooks` | Una voce di tabella; ma va deciso se `release` appartiene ancora a Sailor |

### Categoria 2 — percorsi assoluti o specifici di questa macchina

| `file:riga` | Cosa c'è | Perché viola | Costo per toglierlo |
|---|---|---|---|
| `crates/inventory/src/lib.rs:652-653` | Due `home.join(...)` su cartelle di lavoro | **La peggiore.** Le cartelle di lavoro di una persona sola, compilate: una è il nome di un datore di lavoro, l'altra una convenzione privata. Su un'altra macchina l'inventario dei repo è vuoto e non lo dice | Rifacimento piccolo: le basi diventano un campo di configurazione (`SAILOR_WORK_ROOTS`, o una chiave nella casa) con questo elenco come predefinito **dichiarato**, non come fatto |
| `crates/inventory/src/lib.rs:658-666` | `(".agents", home.join(".agents").join("skills")),` e un secondo magazzino con il nome del datore di lavoro nell'etichetta | Due magazzini di questa macchina, uno con il nome del datore di lavoro dentro l'etichetta **e** dentro il percorso | Come sopra, stesso file di configurazione |
| `crates/release/src/lib.rs:91` | `label: "com.theo.notte",` | Nome proprio in un valore di prodotto. **Attenuante forte**: le righe 34-42 dichiarano che è un predefinito e `RELEASE_SERVICE_LABEL` lo sostituisce (`crates/sailor/src/release_cmd.rs:650`) | Una riga; oppure zero, se si accetta la dichiarazione |
| `crates/release/src/lib.rs:92` | `in_progress_rel: "state/plancia/coda-notte/in-corso",` | Percorso interno di un altro sistema (la plancia, la coda notturna) inchiodato in una tabella di Sailor | Una riga |
| `flows/chiedi-all-indice.flow.json:266-267` | Un `"who"` col nome di una persona e un `"where"` con la sua casa | Un flusso spedito nel repo che si può eseguire **in un posto solo**: è la forma esatta del guasto 25 che `crates/flow/src/workspace.rs:3-8` racconta di aver chiuso | Due righe: la radice viene da chi lancia |
| `flows/come-lo-risolvono-gli-altri.flow.json:411` | «Guarda com'è fatto Sailor, il progetto in ⟨la radice dei sorgenti su quella macchina⟩, ...» | Percorso di questa macchina dentro il testo consegnato a un motore | Una riga |

### Categoria 3 — fornitore, modello o binario presunto inchiodato

| `file:riga` | Cosa c'è | Perché viola | Costo per toglierlo |
|---|---|---|---|
| `crates/models/src/remaining.rs:53` | `const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";` | Indirizzo di un fornitore, senza scavalco d'ambiente (a differenza di `fetch.rs`, che ce l'ha) | Una riga per l'ambiente; un descrittore per farlo diventare un dato |
| `crates/models/src/remaining.rs:56` | `const BETA_HEADER: &str = "anthropic-beta: oauth-2025-04-20";` | Intestazione proprietaria di un fornitore | Idem, insieme |
| `crates/sailor/src/remaining_cmd.rs:39` | `match remaining::read_from_claude(&home, now) {` | **Il comando `sailor remaining` parla a un fornitore solo, senza smistamento.** Non c'è un `match` sul motore: c'è una chiamata sola | Rifacimento piccolo: la lettura di quota diventa una capacità dichiarata dal descrittore del motore, come già è la lettura dei token |
| `crates/models/src/fetch.rs:11` | `const CATALOG_URL: &str = "https://openrouter.ai/api/v1/models";` | Fornitore inchiodato. **Attenuante**: `MODELS_CATALOG_FETCH_OVERRIDE` (riga 18) sostituisce l'intero comando | Zero, se lo scavalco basta; una riga per renderlo un URL configurabile |
| `crates/models/pricing.default.json:55,69,81,95,107,118,129,140` | Otto voci, `claude-opus-5` … `claude-haiku-4-5` — **tutte dello stesso fornitore** | Il listino spedito conosce un fornitore solo: chi usa Codex o Gemini ha `cost_micros = None` su ogni corsa, cioè il guasto 35 rifatto per gli altri motori | Nessun rilascio: si aggiungono voci al file, ed è già sovrascrivibile per `id` da `$SAILOR_HOME/pricing.json`. È un **buco di dato**, non di codice |
| `crates/models/src/fetch.rs:25`, `crates/models/src/remaining.rs:298` | `Command::new("curl")` | Binario presunto installato | Una riga per un messaggio onesto quando manca; una crate HTTP è il rifacimento che `Cargo.toml:9-11` rifiuta di proposito |
| `crates/supervisor/src/main.rs:211-212` | `command: "npm".to_owned(), args: vec!["run".to_owned(), "dev".to_owned()],` | Gestore di pacchetti presunto (non pnpm, non yarn, non bun) | Una riga se letto dalla configurazione. Attenuante: `supervisor` è attrezzatura di sviluppo, non prodotto spedito |
| `crates/sailor/src/release_cmd.rs:395` | `Command::new("mktemp")` | Binario presunto | Una riga (`std::env::temp_dir`) |

### Categoria 4 — presupposti sulla macchina

| `file:riga` | Cosa c'è | Perché viola | Costo per toglierlo |
|---|---|---|---|
| `crates/sailor/src/release_cmd.rs:624-625` | `Command::new("launchctl").args(["kickstart", "-k", &domain])` | Solo macOS, senza `cfg(target_os)` né ripiego: su Linux il rilascio dice «il servizio esegue ancora quello vecchio» per sempre | Una riga per il ripiego onesto; un rifacimento per un vero riavvio-servizio portabile |
| `crates/release/src/lib.rs:34` | `/// L'etichetta launchd predefinita, per `launchctl kickstart -k gui/<uid>/<label>`.` più il campo `label` che la porta | Il tipo `Service` è modellato su launchd, non su «un servizio» | Rifacimento piccolo: il modo di riavviare diventa un dato |
| `crates/inventory/src/lib.rs:681-697` | `pub fn repos_under(bases: &[PathBuf])`, profondità fissa 2 | Ipotesi su come sono organizzate le cartelle di progetto: un repo sta a un livello sotto una base, mai due | Una riga (profondità configurabile), se le basi diventano già un dato |
| `crates/supervisor/src/lib.rs:143` | `pub const DEV_PORT: u16 = 5183;` | Porta fissa, non configurabile; duplicata in `desktop/src-tauri/tauri.conf.json` | Una riga, se si accetta che resti il predefinito. Attenuante: c'è già una prova che le tiene allineate, ed è attrezzatura di sviluppo |

**Nessun presupposto su zsh, e va detto**: `crates/terminal/src/session.rs:47` legge
`SHELL` e ripiega su `/bin/sh`; ogni `Command::new("sh")` del workspace usa il
programma POSIX, non `zsh`. Su questa categoria il codice si comporta bene.

### Categoria 5 — identificatori non in inglese che la prova non vede

`crates/sailor/tests/identifiers_are_in_english.rs` cerca **parole di un elenco
scritto a mano** (righe 44-53) **in posizione di dichiarazione** (`let`, `fn`,
`const`, …), nei file `.rs`, `.ts`, `.tsx`, `.html` sotto `crates`,
`desktop/src`, `desktop/src-tauri/src`. Dichiara di non guardare commenti,
stringhe, `docs/`, e — per decisione di Theo del 31/08 — i `.flow.json`.

Quello che le sfugge, e che la regola di `AGENTS.md` invece copre:

| `file:riga` | Cosa c'è | Perché viola | Costo per toglierlo |
|---|---|---|---|
| `crates/models/pricing.default.json:2` | `"_leggimi": [` | **Chiave JSON in italiano**, e `AGENTS.md` elenca «chiavi JSON» fra gli identificatori. La prova non apre nessun `.json` | Una riga per file (→ `_readme`) |
| `crates/terminal/descriptors/default.json:2` | `"_leggimi": [` | Idem | Idem |
| `crates/trigger/descriptors/default.json:2` | `"_leggimi": [` | Idem | Idem |
| `crates/toolbox/descriptors/default.json:2` | `"_leggimi": [` | Idem | Idem |
| `crates/toolbox/descriptors/automations.json:2` | `"_leggimi": [` | Idem | Idem |
| `crates/models/src/store.rs:17` | `format!("{home}/.claude/state/modelli.json")` | **Nome di file in italiano nato da una stringa letterale**: `AGENTS.md` dice «nomi di file e cartelle», la prova guarda solo i nomi dei sorgenti sul disco | Una riga, insieme allo spostamento di casa |
| `crates/profiles/src/store_io.rs:25` | `.join("profili.json")` | Idem | Idem |
| `crates/sailor/src/inventory_cmd.rs:152` | `.join("flussi")` | Nome di cartella in italiano da stringa letterale | Una riga |
| `crates/ledger/src/lib.rs:56` | `home.join(".claude/state/flussi")` | Idem (gradino di migrazione dichiarato) | Una riga, insieme alla migrazione |
| `crates/release/src/lib.rs:92` | `in_progress_rel: "state/plancia/coda-notte/in-corso",` | Tre nomi di cartella in italiano in una stringa letterale | Una riga |

**Il caso noto va smentito per questo albero.** `~/.claude/state/sessioni-vive/`
**non compare in nessun file dell'albero**: `rg -n 'sessioni' --no-heading -g '!target' -g '!node_modules' .`
risponde solo con `docs/2026-08-28-il-flusso-che-accompagna.md`, in prosa.
Quella cartella la crea qualcosa che sta fuori da questo repo (i ganci in
`~/.claude`), e censirla non è lavoro per questo albero.

### Categoria 6 — elenchi chiusi che dovrebbero essere configurazione

La domanda, per ognuno: *aggiungere una voce richiede un rilascio, o basta un file?*

| `file:riga` | Cosa enumera | Aggiungere una voce | Costo per aprirlo |
|---|---|---|---|
| `crates/profiles/src/lib.rs:57-116` | `KNOWN_CLIS`: quattro righe di comando (`claude`, `codex`, `gemini`, `antigravity`), con eseguibile e variabile di casa | **Rilascio.** Il commento alla riga 41 dice «allungala aggiungendo una voce, non serve altro» — vero per chi compila, falso per chi installa | Rifacimento piccolo: la tabella diventa un descrittore come quelli di `toolbox`, con lo stesso meccanismo di sostituzione per `id` |
| `crates/inventory/src/discovery.rs:22-39` | Le radici da cui si caricano competenze e agenti (7 percorsi) | **Rilascio** | Rifacimento: è lo stesso lavoro della categoria 1 |
| `crates/inventory/src/lib.rs:648-671` | Le cartelle di lavoro e i due magazzini di questa macchina | **Rilascio** | Rifacimento piccolo (vedi categoria 2) |
| `crates/release/src/lib.rs:83-…` | `TARGETS`: cosa si rilascia (`notte`, `hooks`, `sailor`) | **Rilascio** | Una tabella da leggere da file; ma prima va deciso se questi bersagli sono di Sailor |
| `crates/supervisor/src/lib.rs:143` | La porta di sviluppo | **Rilascio** | Una riga |
| `desktop/src/ToolMark.tsx:35-…` | 13 segni grafici (`claude-code`, `codex`, `gemini-cli`, `ollama`, `git`, `gh`, `docker`, `node`, `npm`, `cargo`, `kubectl`, `curl`, `python`) | **Rilascio, ma non serve**: le righe 9-13 dichiarano che il ripiego è il caso normale e uno strumento sconosciuto prende un monogramma dignitoso | Zero. **Questo elenco è chiuso e va bene così**: nessuna condizione, e non aggiungere una voce non toglie niente a nessuno |

**I contro-esempi virtuosi, da imitare e non da toccare.** Dove il progetto ha
già fatto il lavoro giusto, aggiungere una voce **non richiede un rilascio**:
`crates/toolbox/descriptors/default.json` (36 strumenti, si estende in
`~/.config/sailor/tools.d/`), `crates/trigger/descriptors/default.json`
(`~/.config/sailor/triggers.d/`), `crates/terminal/descriptors/default.json`
(`~/.config/sailor/routes.d/`), `crates/models/pricing.default.json`
(`$SAILOR_HOME/pricing.json`, sostituzione per `id`). E la riga che dichiara la
regola: `crates/toolbox/src/lib.rs:7` — *«nessun `if id == "docker"`, nessun
percorso di questa macchina»*.

---

## 3. I falsi positivi — trovati, guardati, scagionati

Nessuno di questi è una violazione. Sono elencati perché ricompaiono a ogni
`rg`, e ricensirli costa una giornata.

**Nome di prodotto in un'etichetta, una nota o un commento** (permesso esplicito
della regola):

- `crates/trigger/descriptors/default.json:44-53` — il descrittore `orca-terminal`,
  con `"label": "I pannelli di Orca"` e una `note` che spiega perché
  `output.log` non è una sorgente onesta. **È un dato in un file sostituibile,
  non una condizione**, e la nota dice da sé che lo strumento `orca` «non è
  spedito: si aggiunge in `~/.config/sailor/tools.d/`». Al limite si discute se
  spedire un fornitore terzo nel catalogo predefinito; non è una violazione della
  regola come è scritta.
- `crates/toolbox/descriptors/automations.json:74-79` — `vscode-tasks`, con i
  percorsi dei file di VS Code. Stessa forma: un dato, in un catalogo estensibile.
- `crates/toolbox/descriptors/automations.json:87-105` — i tre descrittori
  launchd, con `"(macOS)"` **scritto dentro l'etichetta**. È esattamente il
  comportamento voluto: la specificità è dichiarata a chi legge, non nascosta in
  un ramo.
- `crates/inventory/src/lib.rs:11` — «quella che Orca gli passa davvero», in un
  commento di modulo.
- `crates/inventory/src/lib.rs:541-542` — `orca-cleanup --close` in un commento.
- `tools/cargo-mutants.sh:38,75` e `tools/mutants.sh` (tutto il file) — Orca in
  commenti e in un banco di mutazione. `tools/mutants.sh:73` nomina
  `crates/claude-hooks/src/orca_cleanup.rs`, **un crate che non esiste**
  (`Cargo.toml` non lo elenca): è attrezzatura scaduta, non prodotto.
- `flows/dispatch-the-work.md:130,143,149` — prosa di progetto.
- `desktop/src/ToolMark.tsx:5` — `Non esiste nessun `if (tool === "claude")``,
  cioè un commento che dice di non avere la condizione.
- `crates/toolbox/src/lib.rs:7` — `nessun `if id == "docker"``, idem.

**Percorsi di questa macchina dentro dati di prova o documentazione** (dati, non
identificatori, e la prova sull'inglese lo dichiara):

- `crates/flow/src/executor.rs:1328,1336`; `crates/ledger/src/tests.rs:2025`;
  `crates/terminal/tests/a_routed_request_reaches_the_trigger.rs:64,72,127`;
  `crates/trigger/src/action.rs:204,210`; `crates/actions/src/store.rs:411,423,434`;
  `desktop/src/contrast.test.tsx:249`; `crates/sailor/src/flow_cmd.rs:4265,4362`;
  `crates/actions/tests/the_equipment_reaches_the_engines.rs:54,80,115`
  (`/opt/homebrew/bin/codex`, `/usr/local/bin/claude`);
  `crates/profiles/src/lib.rs:306-392` e `crates/sailor/src/profiles_cmd.rs:139-141`
  (`/home/profiles/...`, `/home/theo/.acme` — fixture);
  `crates/toolbox/tests/one_home_for_everything.rs` e
  `crates/trigger/tests/triggers_live_in_the_same_home.rs` (`/home/tizio`).
- `crates/flow/src/schedule.rs:157,205` — `perimeter: vec!["~/.claude"]` e
  `["~/.claude", ⟨la radice dei sorgenti⟩]` stanno **dentro `#[cfg(test)] mod tests`**
  (che comincia alla riga 140). Fixture — **tolte il 01/09**: una fixture che
  nomina la casa di chi l'ha scritta si pubblica come tutto il resto.

**Commenti che raccontano un percorso già tolto** — sono la cicatrice, non la
ferita:

- `crates/flow/src/workspace.rs:4` — cita il `"workdir"` assoluto come il guasto
  25 *chiuso*.
- `crates/ui/src/gather.rs:90-91,128-130` — cita `~/.claude/state` e la radice
  dei sorgenti come i percorsi **rimossi** il 28/08.
- `crates/release/src/lib.rs:50` e `crates/sailor/src/flow_cmd.rs:1350,1529`.
- `desktop/src/tools.ts:208` — «`/Users/tizio/.local/bin/claude` solo su una».

**Guardie che nominano il male per impedirlo:**

- `crates/sailor/src/flow_cmd.rs:1542` — `const ABSOLUTE_PREFIXES: [&str; 4] = ["/Users/", "/home/", "/private/", "~/"];`
  è il **controllo** che rifiuta i percorsi assoluti nei flussi.
- `crates/sailor/tests/dispatch_the_work.rs:198-199` —
  `assert!(!text.contains("/Users/"), "un percorso di casa è cablato");`
  Una prova che già difende questa politica, su un pezzo del codice.

**Nomi di motore in stringhe di prova e in campi dati:**

- `"claude-vivo"` in `crates/actions/src/handoff.rs:276,288,311,341` e
  `crates/sailor/src/step_cmd.rs:693,972` — è il valore del campo `holder` in
  una fixture, un dato.
- `"claude-code"`, `["codex", "claude-code"]` nei `.flow.json` e nei commenti di
  `crates/sailor/src/flow_cmd.rs:1245,1936` — è l'**identificativo di strumento**
  che `toolbox` risolve, cioè la forma corretta descritta in
  `crates/actions/src/lib.rs:29-33`. Il contrario di una violazione.
- `crates/toolbox/src/descriptor.rs:1217,1387` — `.find(|l| l.descriptor.id == "agy")`
  dentro le prove del caricatore: verificano che *un id qualsiasi* si carichi.

---

## 4. Le categorie e le ricerche che sono risultate vuote

Nessun risultato, col comando esatto, così si può ricontrollare.

**Nessuna condizione su una variabile d'ambiente di un terminale.** Niente
`ORCA_*`, `TERM_PROGRAM`, `WARP_*`, `ITERM*`, `TMUX`:

```
rg -n 'ORCA_|VSCODE|TERM_PROGRAM|WARP_|ITERM|TMUX' crates desktop/src desktop/src-tauri/src -g '!target' -g '!node_modules'
```

L'elenco completo delle variabili lette dal prodotto è: `CLAUDE_HOME`, `HOME`,
`LEDGER_*`, `MODELS_CATALOG_FETCH_OVERRIDE`, `MODELS_CONFIG_PATH`,
`PROFILES_HOME_ROOT`, `PROFILES_STATE_PATH`, `RELEASE_SERVICE_LABEL`,
`SAILOR_FLOWS`, `SAILOR_HOME`, `SAILOR_LEDGER`, `SHELL`, `TMPDIR`, `USER`,
`XDG_CONFIG_HOME`. Una sola porta il nome di un altro prodotto (`CLAUDE_HOME`,
categoria 1).

**Nessun `match` o confronto su un nome di terminale o di motore che cambi il
comportamento** (a parte i tre `mattpocock` già censiti):

```
rg -n '(==|!=|contains|starts_with|ends_with|matches!|case )\s*.{0,12}"(claude|codex|gemini|orca|warp|vscode|iterm|tmux|ollama|agy|antigravity)[a-z0-9\-]*"' crates/*/src desktop/src desktop/src-tauri/src -g '*.rs' -g '*.ts' -g '*.tsx'
rg -n '"(claude|codex|gemini|orca|ollama|agy|antigravity)[a-z0-9\-]*" =>' crates/*/src desktop/src desktop/src-tauri/src
```

Il primo restituisce solo prove e commenti (elencati fra i falsi positivi); il
secondo **non restituisce niente**: non esiste un `match` sul nome di un motore
in tutto il workspace.

**`desktop/src/` e `desktop/src-tauri/src/` sono puliti** su percorsi assoluti e
di macchina — l'unica occorrenza è in un file di prova
(`desktop/src/contrast.test.tsx:249`) e due in commenti:

```
rg -n '/Users/|/opt/|/usr/|~/\.|homebrew|launchctl|open -a|osascript' desktop/src-tauri/src/*.rs desktop/src/*.ts desktop/src/*.tsx
```

**Nessun presupposto su zsh:**

```
rg -n '"(sh|zsh|bash|/bin/sh|/bin/zsh|/bin/bash)"|SHELL' crates/*/src desktop/src-tauri/src -g '*.rs'
```

Ogni invocazione usa `sh` (POSIX) o legge `SHELL` dall'ambiente.

**Nessun identificatore italiano con morfologia riconoscibile sfuggito
all'elenco della prova** (`-zione`, `-mento`, `-atore`, `-anza`, `_del_`, …):

```
rg -n '\b(let|fn|struct|enum|mod|const|static|type|trait|interface|class|function) [a-z_A-Z]*(zione|mento|atore|itore|anza|enza|aggio|ella|etto|ismo|tura|_di_|_del_|_la_|_il_|_che_|_per_|_con_)[a-zA-Z_]*' crates/*/src desktop/src desktop/src-tauri/src -g '*.rs' -g '*.ts' -g '*.tsx'
```

I sei risultati sono tutti `price_per_million` e parenti: inglese. La prova
sull'inglese, per quel che copre, tiene.

---

## 5. L'ordine in cui li toglierei

**1. `crates/inventory/src/lib.rs:652-653` — le due cartelle di lavoro compilate.**

Prima di tutto, e per una ragione sola: **è l'unica violazione che risponde una
bugia invece di rompersi.** Le altre, su una macchina diversa, danno un errore
che si legge — `launchctl` non c'è, `~/.claude` non esiste, il listino manca e il
costo resta `None`. Questa no: `repos_under` cammina in due cartelle che non
esistono, `read_dir` fallisce, il `continue` alla riga 688 se lo mangia, e
l'inventario dice **«zero repo»** con uscita 0. È la forma esatta della lezione
che questo repo ha già pagato due volte — `crates/ui/src/gather.rs:128-133`, i
quattordici flussi che sembravano zero — e l'unica che nessun controllo può
distinguere da una macchina davvero vuota.

**2. `crates/sailor/src/inventory_cmd.rs:149-152`** — una riga, e chiude una
seconda copia della scoperta della casa che `crates/ledger/src/lib.rs:82-89`
dichiara di aver già unificato. Costo minimo, debito già diagnosticato altrove.

**3. I tre siti `mattpocock`** (`discovery.rs:26`, `discovery.rs:95-96`,
`lib.rs:249`) — tre righe. Sono morte oggi e nessuno se ne accorgerebbe; per
questo vanno tolte adesso, prima che qualcuno le trovi vive e le tenga.

**4. Le case che stanno in `~/.claude`** (`models/src/store.rs:17`,
`profiles/src/store_io.rs:22-26`, `sailor/src/release_cmd.rs:292`) — una riga
ciascuna, tutte verso `ledger::sailor_home()`, che esiste già ed è provato.
Vanno insieme: farne una sola lascerebbe la configurazione di Sailor divisa fra
due case, che è il guasto raccontato in `crates/ledger/src/lib.rs:82-89`.

**5. `_leggimi` → `_readme`** nei cinque `.json` spediti — cinque righe, e nel
gesto si estende la prova sull'inglese ai `.json` **spediti** (non ai
`.flow.json`, che la decisione del 31/08 esclude). Senza quell'estensione la
correzione non ha chi la tenga.

**6. Il listino con un fornitore solo** — è un buco di dato, non di codice, e si
chiude scrivendo voci in `crates/models/pricing.default.json`. Va dopo le altre
perché richiede numeri veri, non una rinomina.

**7. `KNOWN_CLIS` e le radici di `discovery`** — l'unico rifacimento vero.
Vanno fatti insieme, con lo stesso meccanismo dei descrittori di `toolbox`, e
solo dopo che i sei punti sopra hanno tolto le violazioni che costano una riga:
un rifacimento fatto per primo si porterebbe dietro tutte le altre.

**Fuori da questo elenco, e va deciso da Theo, non da chi ripara:**
`crates/release` — l'intera tabella `TARGETS` rilascia binari (`notte`,
`claude-hooks`) che **non stanno in questo workspace**. Non è una violazione da
correggere: è la domanda se quel crate appartenga ancora a Sailor.
