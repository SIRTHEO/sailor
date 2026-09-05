# Dove Sailor è lento, misurato

**05/09/2026.** Theo: *«migliorare di gran lunga le performance; su questo fai
tanta ricerca e cerca di capire come veramente migliorare Sailor, non solo a
calcolo matematico ma in tutti gli aspetti».* Questo documento parte dalle
misure, non dalle opinioni: ogni riga porta il numero e come è stato preso, e
ogni rimedio dice cosa si misura dopo per sapere se ha funzionato. Dove non ho
misurato, lo dico.

## In una riga

Il binario è veloce e la macchina non se ne accorge; **lento è il ciclo di chi
lo sviluppa** — il rilascio rifaceva da capo tutto il lavoro di compilazione a
ogni chiamata — e **costoso è ogni passo che chiama un motore**, venti-trenta
secondi e mezzo dollaro l'uno. Il primo si ripara in casa ed è entrato stanotte;
il secondo si governa con la tabella delle forze e con il modello locale, non
con l'ottimizzazione.

## Le misure, una per aspetto

### 1. Il binario e i comandi: niente da riparare

| misura | valore | come |
|---|---|---|
| `sailor --help`, `session list`, `flow list`, `flow search` | < 5 ms ciascuno (`real 0.00`) | `/usr/bin/time -p` sul binario in servizio, ledger aperto compreso |
| dimensione di `sailor` | 6,4 MB | `ls -la ~/.config/sailor/bin/sailor` |
| dimensione della finestra | 13 MB | idem, `sailor-desktop` |
| profilo di rilascio | `opt-level 3`, `lto`, `codegen-units 1`, `strip`, `panic abort` | `Cargo.toml` |

I ganci che ogni riga di comando chiama a ogni evento (`sailor session
event`) pagano l'avvio del processo e un'apertura di SQLite: sotto i cinque
millisecondi. Non è qui che si perde.

### 2. Il rilascio: da 7 min 44 s a 47 s per un commit di sola documentazione

**Misurato** il 05/09 alle 02:52:58 → 03:00:42, rilascio di `ed0ca332` (un
commit che tocca solo `docs/`): **7 min 44 s**, di cui 42 s di compilazione di
quattro crate in cui niente era cambiato, e il resto la suite intera —
centouno file di prova d'integrazione più i binari di unità, **ognuno rilegato
con LTO pieno** (`lto = true`, `codegen-units = 1` valgono anche per
`cargo test --release`).

**La causa** non era la suite: era il clone. `make_temporary_tree` creava una
cartella `mktemp` nuova a ogni rilascio, e cargo giudica se ricompilare per
percorso e mtime dei sorgenti: entrambi nuovi, quindi niente era mai
incrementale. Un secondo rilascio dello stesso albero rifaceva il lavoro del
primo per intero.

**Il rimedio, entrato in `5b2a36f8`:** l'albero di rilascio vive in
`target/release-tree`, clonato una volta e poi portato a HEAD con `git fetch` +
`checkout --force` + `clean -fd`. Un checkout riscrive solo i file cambiati, e
un crate che nessuno ha toccato tiene i suoi mtime e i suoi binari di prova.
La prova (`the_release_tree_is_kept_and_only_what_changed_is_touched`) tiene
il mtime di un file non toccato attraverso due rilasci.

**Prime misure con l'albero persistente:** il rilascio 20 (`04792a7b`, tre
crate toccati) è durato **5 min 27 s** (03:29:50 → 03:35:17), 38 s di
compilazione — i binari di prova dei crate toccati e di chi dipende da loro
sono stati rilegati, gli altri no. Il rilascio 19, sola documentazione, ha
compilato per 34 s soltanto `catalogue` (incorpora `i18n/*.json`, che erano
cambiati) e `sailor`, ma è uscito rosso per il guasto 89 e non conta come
misura.

**Misurato, il rilascio 21 (`e5b7b5a7`, sola documentazione): 47 s**
(03:35:42 → 03:36:29), 0,10 s di compilazione, il resto l'*esecuzione* della
suite — 122 binari già legati — e il confronto del binario, che era già in
servizio. **Da 7 min 44 s a 47 s, dieci volte**, per la stessa classe di
commit. La previsione detta prima («sotto i due minuti») era prudente.

**Il passo successivo, se il numero non basta:** i binari di prova non hanno
bisogno di LTO pieno. Un profilo `[profile.suite]` che eredita da `release`
con `lto = "thin"` o `false` e `codegen-units = 16`, usato solo dalla suite
del rilascio, si misura nello stesso modo. Non entra finché la misura di prima
non dice che serve: due rimedi insieme non si misurano.

E già entrato prima, la stessa notte: **il rilascio ricorda la suite che ha
passato** (`state/<bersaglio>-suite-tree`), quindi il *secondo* rilascio dello
stesso albero non la rifà. Misurato in `rilascio-cli16`: il memo ha parlato
quando l'albero era lo stesso.

### 3. I passi che chiamano un motore: il costo vero di un flusso

**Misurato** sulle quattro corse di `draft-a-flow` della notte, motore
`claude-code`:

| corsa | durata del passo `author` | costo | token in cache |
|---|---|---|---|
| 1 | 30,2 s | 0,63 $ | 78 k letti, 25 k creati |
| 2 | 18,7 s | 0,48 $ | 13 k letti, 20 k creati |

Il nostro prompt è piccolo (schizzo + vocabolario + regole: qualche migliaio di
token); i 20–25 k «creati in cache» a ogni corsa sono il prompt di sistema
della riga di comando del motore, che non governiamo. Quindi:

- **Non c'è ottimizzazione da fare sul passo**: è il tempo del modello.
- **C'è da scegliere il modello giusto per `kind`**, ed esiste già: la tabella
  delle forze mette prima i motori adatti, e `sweep-the-tree` usa `ollama` per
  il lavoro `mechanical` a costo zero. La misura da tenere è `sailor flow cost
  <nome>` corsa dopo corsa.
- **`max_attempts: 2`** su un passo da mezzo dollaro raddoppia il costo del
  peggior caso. Va deciso passo per passo, e il ledger dice quante volte il
  secondo tentativo è servito.

### 4. Il ledger: piccolo, e un `fsync` a ogni evento

| misura | valore |
|---|---|
| `state.db` | 6,1 MB — 140 corse, 519 passi, 34 chiamate, 6 voci di deposito |
| `events.db` | 9,8 MB — 1 538 eventi |
| `sessions.db` | 5,4 MB |
| `journal_mode` | WAL, `page_size` 4096 |
| `synchronous` | **FULL** su `state.db`, NORMAL su `events.db` |

Con `synchronous = FULL` ogni commit aspetta un `fsync`; in WAL, `NORMAL` resta
sicuro contro la corruzione — aspetta l'`fsync` solo ai checkpoint — e accetta
di perdere le *ultime* transazioni se manca la corrente ([SQLite, WAL e
`synchronous`](https://www.sqlite.org/wal.html); la stessa lettura in
[Sensible SQLite defaults](https://briandouglas.ie/sqlite-defaults/) e in
[SQLite performance tuning](https://phiresky.github.io/blog/2020/sqlite-performance-tuning/)).
Il ledger scrive a ogni gancio di ogni terminale: `sailor session event`
finisce in `Ledger::put_record` (la presenza del terminale, `announce` in
`session_cmd.rs`), cioè una transazione sul log e una sulla proiezione, e con
FULL la seconda paga un `fsync`. **Misurato il 05/09** con
`crates/ledger/tests/how_much_an_fsync_costs.rs` — ignorata di default, si
lancia con `cargo test -p ledger --test how_much_an_fsync_costs -- --ignored
--nocapture` — duecento scritture in fila su un ledger di prova in
`temp_dir()`, stesso volume APFS della casa. Il ledger non espone
`synchronous`, e non deve: il confronto con NORMAL rifà a mano le stesse due
transazioni, istruzione per istruzione, e la riga «a mano, FULL» dice quanto
la copia somiglia all'originale. Tre corse; vale la terza, la mediana.

| mediana su 200 scritture (totale) | 1ª corsa | 2ª corsa | **3ª corsa** |
|---|---|---|---|
| `put_record` dal ledger, FULL | 0,226 ms (48,5 ms) | 0,244 ms (51,2 ms) | **0,241 ms (50,3 ms)** |
| `record_run` dal ledger, FULL | 0,175 ms (38,1 ms) | 0,180 ms (39,7 ms) | **0,200 ms (42,2 ms)** |
| `put_record` a mano, FULL | 0,231 ms (49,2 ms) | 0,246 ms (50,8 ms) | **0,227 ms (47,8 ms)** |
| `put_record` a mano, NORMAL | 0,192 ms (71,3 ms) | 0,199 ms (41,8 ms) | **0,187 ms (39,6 ms)** |
| `put_record` a mano, FULL e `fullfsync` | 3,866 ms (779 ms) | 3,993 ms (818 ms) | **3,940 ms (753 ms)** |

FULL costa **0,04–0,05 ms a scrittura** più di NORMAL: dieci millisecondi ogni
duecento eventi, un quarto di millisecondo per gancio. **Resta FULL**: un
registro che non perde l'ultima riga vale più di quel ventesimo di
millisecondo, e nessun pragma cambia. (I 71,3 ms della prima corsa NORMAL
sono una scrittura sola da 17,5 ms, un checkpoint del WAL: capita in entrambi
i modi e non è l'`fsync`.)

Perché costa così poco: su macOS `fsync` non svuota la cache del disco, e
SQLite chiede lo svuotamento vero (`F_FULLFSYNC`) solo con `PRAGMA fullfsync =
ON`, che il ledger non imposta. Con quello ogni scrittura costa **3,9 ms**,
sedici volte tanto: è il prezzo della durabilità contro la mancanza di corrente
che FULL promette e che su questo sistema oggi non compra. Se la si vuole è
una decisione a sé, di Theo, e quello è il suo prezzo; il documento non la
propone, perché nessuno ha ancora perso una riga.

`mmap_size` e `cache_size` idem: si provano quando un'interrogazione risulta
lenta, e oggi nessuna lo è.

Crescita: `watch-the-crew` ogni mezz'ora scrive 5 passi, 240 righe al giorno,
~90 000 l'anno — una decina di MB. Su una macchina accesa per anni è il costo
di sapere chi c'era; resta.

### 5. La finestra

| misura | valore |
|---|---|
| bundle JavaScript | 1,0 MB (`index-*.js`), 129 KB di CSS, cinque file `woff2` di Inter (85 + 48 + 26 + … KB) |
| `node_modules` | 283 MB (solo sviluppo) |
| battito | ogni 60 s: rilegge i flussi dal disco e giudica `is_due` |

Un megabyte di JavaScript per una finestra che è soprattutto terminale è molto:
il battito e il rendering non sono misurati, e vanno misurati con il profiler
del WebView prima di toccare qualcosa. I consigli generali ([Tauri v2,
prestazioni e dimensione](https://www.oflight.co.jp/en/columns/tauri-v2-performance-bundle-size),
[WebView2 performance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance))
sono quelli attesi — meno DOM, liste virtuali, IPC a lotti — e nessuno vale
finché non c'è un numero che dice dove la finestra rallenta. Non ce n'è.

### 6. Il processo che vive per anni

Vincolo: leggerezza su macchine accese per anni. La misura di riferimento è
l'host dei terminali, 336 KB dopo 13 ore (memoria del 02/09). Stanotte nessun
processo di Sailor era vivo oltre ai comandi: non c'è niente di nuovo da
misurare. Il battito vive dentro la finestra, non in un demone: giusto per il
vincolo, e il prezzo — nessun orologio a finestra chiusa — sta in `da-fare.md`.

### 7. Il ciclo di chi sviluppa, oltre il rilascio

- `sailor ratchet`: **55 s** la prima volta, ~**60 s** ogni volta (36 giudici;
  `target/from-head` caldo). Prima era un rito a mano di minuti e di errori.
- `cargo build -p sailor` incrementale: 15–50 s secondo il crate toccato.
- La suite intera in debug (`cargo test --workspace --no-fail-fast`): ~10 min
  con compilazione, misurata stanotte una volta sola dal log; da rimisurare.
- Dodici rossi trovati dal cricchetto prima del commit, quattro dei quali
  «un crate alla volta» dallo stesso giudice, che ora li dice tutti insieme.

## L'ordine delle cose da fare, e la misura di ciascuna

1. **Scrivere il numero del rilascio con l'albero persistente** (§2) —
   `inizio`/`fine` nel log del prossimo rilascio di sola documentazione.
2. **Solo se non basta: il profilo della suite senza LTO pieno** — stessa
   misura.
3. ~~**`synchronous NORMAL` su `state.db`, se cento eventi in fila lo
   giustificano**~~ — misurato il 05/09 (§4): duecento eventi in fila
   costano dieci millisecondi in più con FULL. Non lo giustificano; resta
   FULL.
4. **`max_attempts` dei passi da mezzo dollaro**, letti dal ledger: quante volte
   il secondo tentativo ha salvato la corsa.
5. **La finestra si profila prima di toccarla.**

## Cosa non si fa

- Non si tocca `opt-level`, `lto` o `codegen-units` del binario in servizio: il
  binario è già piccolo e veloce, e quelle opzioni costano solo a chi compila.
- Non si aggiunge un demone per il battito: il vincolo delle macchine accese
  per anni vale più di un orologio a finestra chiusa.
- Non si «ottimizza» un passo di motore: si sceglie il motore.
