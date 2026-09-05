# Dove sta l'MVP e quanto debito porta

**05/09/2026, mattina.** Theo: *«prima sistemiamo tutto il debito tecnico, poi
mi piacerebbe potenziare di gran lunga cosa abbiamo costruito, come da
mandato. Come siamo messi come MVP?»* Questa è la misura presa sull'albero a
`70c25355`, e l'ordine di lavoro che ne segue, confermato da Theo la stessa
mattina. Ogni numero porta il comando che lo ha dato.

## L'MVP contro il mandato

Il mandato (`2026-09-02-sailor-runs-on-every-engine.md`) ha **40 punti
numerati**. **31 hanno una riga «where it landed»** con prova e mutante. Dei
nove senza, uno è nel codice ma non nel resoconto (A.5, il passo privato che
non va a un motore che addestra: `a_private_step_never_goes_where_the_pact_is_not_a_no`).
Gli altri otto sono buchi veri:

| punto | cosa manca |
|---|---|
| A.6 | la regola dei soli gratuiti: decisione di Theo |
| C.2 | `sviluppa-sailor` che corre sotto Sailor su Sailor, con una corsa vera nel ledger |
| C.5 | il muro di tempo e i tre motivi di stop di una corsa che si cura da sola |
| D.4 | il rapporto di ogni corsa scritto una volta e riletto dalla corsa dopo |
| D.5 | competenze, regole e dotazione gestite da Sailor per workspace |
| D.6 | gli specialisti forzati: sessione che coordina senza Edit e Write, gancio che rifiuta |
| E.5 | le sei lacune dei giudici della finestra |
| G.4, G.5 | il token del supervisore per i processi lunghi; `Some(Null)` distinto da `None` |

Funziona da capo a fondo, misurato: tre sedie su un ledger, i flussi di
sistema, la lavagna che scrive un flusso vero, la scelta del motore per
carburante, il rilascio con la suite, le memorie e la ricerca, il battito.
**Per chiamarlo MVP per qualcuno che non siamo noi manca**: una corsa
completa di un flusso di sviluppo con un motore vero e un esito nel ledger
(C.2); i motori diversi dal primo non vedono le memorie (`sailor memory where`
lo dice: nessuno dei tre legge un file che nomina la pagina); e cinque pezzi
della notte non sono mai corsi dal vivo (`for_each`, `consolidate-memories`,
`take-the-next-fault`, il saluto di Codex e Gemini, il guasto scritto dal
battito).

## Il debito, contato

| voce | misura | comando |
|---|---|---|
| avvisi clippy | 344; 180 «non serve `&mut`», 109 «variante troppo grande» | `cargo clippy --workspace --all-targets` |
| `unwrap` / `expect` / `panic!` nei file `src` | 139 / 1 063 / 83, prove in linea comprese | `grep` sui `src/*.rs` fuori da `tests/` |
| righe di commento non inglesi | 7 101; 500 blocchi lunghi; 183 con una data | i semi di `comments_do_not_crowd_out_the_code` |
| file fuori scala | 9 sopra 1 500 righe; due sopra 7 500 (`actions/src/lib.rs`, `sailor/src/flow_cmd.rs`) | `wc -l` |
| guasti aperti | 17 su 93, due dei quali sono la stessa cosa | `guasti-incontrati.md` |
| difetti noti e non riparati | 11 | `da-fare.md` |
| decisioni di Theo in attesa | 10, una barrata | `da-fare.md` |
| chieste da Theo, non iniziate | 10 | `da-fare.md` |
| rilascio | 49 min con la suite in `--release` su 42 commit; rosso se lanciato dentro il perimetro | `rilascio-cli25.log` |
| prove | 1 415 `#[test]` in Rust, 433 in vitest, 3 ignorate | `grep`, `vitest run` |

Il debito **strutturale** non sta nei numeri di clippy: sta nei due file da
7 500 righe, nella memoria che non sa di quale albero è, e nel fatto che
nessun giudice conta i `panic!` del codice di produzione, perché il conteggio
sopra non distingue le prove in linea dal codice.

## L'ordine, confermato

1. **Un giudice per ogni numero che non ne ha**: `unwrap`/`expect`/`panic!`
   fuori dalle prove, avvisi clippy per crate. Semi al valore di oggi, solo in
   discesa. Da lì il debito non risale più.
2. **Clippy a zero** sui due gruppi grossi, un agente per crate.
3. **Spaccare `actions/src/lib.rs` e `flow_cmd.rs`** per responsabilità, sotto
   le 2 000 righe di `files_do_not_grow_out_of_scale`.
4. **La memoria con l'albero**, pagina per progetto più le globali.
5. **I 17 guasti aperti e gli 11 difetti noti**, uno per agente, ognuno con la
   prova rossa prima della cura. I primi: 52 (il conto dei binari di prova e
   dei flussi che non scende in silenzio), 67 (ogni documento che dichiara un
   controllo nomina una prova che esiste), G.4 (il token del supervisore),
   G.5 / guasto 33 (`Some(Null)` e `None`).
6. **Le corse dal vivo** dei pezzi mai eseguiti, con Theo presente perché
   costano.

I commenti non inglesi restano al cricchetto: riscriverne 7 101 con un motore è
stato il guasto 58, il riassunto spacciato per traduzione.

Dopo il debito, il potenziamento è il mandato: C.2 → D.4 → D.6 → C.5 →
G.4/G.5 → D.5 → E.5, perché C.2 è la condizione di fine e tutto il resto lo
alimenta.

## Come si lavora, misurato la notte prima

Nove agenti in worktree in parallelo, ognuno con il suo `target/`; il tronco
fonde con `git merge --no-ff`, rimisura con `sailor ratchet`, prova la suite
intera fuori dal perimetro e rilascia da solo. Le regole che gli agenti devono
avere nel mandato, imparate la notte: partire dalla punta di `sorgenti`, mai
aspettare una notifica in sfondo, mai `git stash` (è condiviso), non toccare i
documenti, un commit per pezzo, ogni prova con il suo mutante. Undici agenti
la notte prima hanno consegnato undici pezzi; quattro difetti sono stati
trovati rileggendo il codice fuso, non eseguendolo (guasti 90–93).

## Cosa è entrato con la prima ondata, misurato a `5d9e98a4`

Nove agenti in worktree, lanciati alle 12:20, tutti fusi e verdi alle 14:40.
Suite intera verde fuori dal perimetro, vitest 433/433, quaranta giudici.

| voce | prima | dopo |
|---|---|---|
| avvisi clippy nel workspace | 344 | **0**, e un giudice per crate (`clippy_only_ever_gets_quieter`) |
| `unwrap`/`expect`/`panic!` nel codice di produzione | non contati | **44**, contati per crate (`production_code_does_not_panic_on_purpose`); 19 nel crate dei terminali |
| file oltre le 2 000 righe | 6 | **4**: `actions/src/lib.rs` è dodici moduli (il più grande 1 757), `flow_cmd.rs` una cartella di nove (il più grande 1 187) |
| binari di prova, funzioni `#[test]`, flussi | non contati | 111 / 1 506 / 10, e un numero che scende senza dichiararlo è rosso (`the_battery_does_not_shrink_in_silence`) |
| percorsi `flows/…` nominati nel codice | non verificati | ognuno deve esistere (`every_flow_path_the_code_names_exists`) |
| sezioni dei documenti che dichiarano un controllo | 37 in sola prosa, nessuno lo sapeva | 37 contate, solo in discesa; ogni nome di prova nominato deve esistere |
| memoria | senza albero | `Memory.tree`; pagina per albero più le globali; `sailor remember --global`; la ricerca dice l'albero |
| guasti aperti | 17 | **15** (52 e 67 chiusi dai loro giudici; 33 e G.5: `output` è un campo solo con il suo serializzatore) |
| G.4 | assente | `supervisor::StartToken`, due `compile_fail` |

Due cose imparate. **La misura di stamattina era sbagliata per difetto** sul
conto dei `panic!`: contava 139/1 063/83 con le prove in linea; il giudice, che
le esclude, ne trova 44. **Un giudice nuovo sposta i vecchi**: la scissione di
`flow_cmd.rs` ha scoperto 26 frasi che il giudice del catalogo non leggeva,
perché si fermava al primo `#[cfg(test)]` del file; ora hanno la loro voce.

La seconda ondata (guasti 74, 37, 17, i quattro difetti di un motore, il
testo del passo comando, guasti 10 e 5) è partita alle 14:35 e **caduta sul
limite di sessione** senza un commit; ripresa tre agenti alla volta.
