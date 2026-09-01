# La lettura di un valore

Un passo che esegue un comando oggi consegna a valle una cosa sola: se è andato
bene. Ciò che il comando ha *detto* muore dentro il passo. Questo documento
misura quanto manca davvero per farglielo consegnare — e la risposta è: molto
meno di quanto sembri, perché il meccanismo esiste già, provato, e sta a due
funzioni di distanza.

## Il fatto

`crates/actions/src/lib.rs:2437` — l'intera uscita di `shell_check`:

```rust
Ok(ActionOutcome::Went(json!({ "status": status })))
```

`crates/actions/src/lib.rs:2053` e `:2140` — l'uscita di `external_engine`
quando il passo ha dichiarato una forma:

```rust
shaped_answer(shape, &stdout).map(|answer| {
    Asked::Answered(ActionOutcome::Went(json!({"status": "ok", "answer": answer})))
})
```

Stesso crate, stesso file, novanta righe di distanza. Uno legge, l'altro butta.

## Sailor non è un sistema di soli esiti

Delle sedici azioni registrate, la maggior parte consegna già un valore:

| azione | consegna |
|---|---|
| `store_read` | `count`, `last_key`, `entries` con dentro i valori |
| `store_list`, `store_write` | chiavi e conferme |
| `work_survey` | chi sta lavorando su cosa |
| `detect_tools` | quali strumenti ci sono |
| `history_ask`, `mcp_ask` | la risposta, in un involucro |
| `external_engine` | `answer`, potata sulla forma dichiarata |
| **`shell_check`** | **solo `status`** |

Quindi la domanda non è «sailor sa restituire valori». Sa. La domanda è perché
l'unica azione che **legge il mondo attraverso un comando** è anche l'unica
lettrice che getta via ciò che ha letto.

## Il macchinario è già generico

Tre pezzi, e nessuno dei tre è legato al concetto di motore:

- **`ValueSchema`** — `crates/flow/src/schema.rs:9`. Vive nel crate `flow`, non
  in `actions`. È già infrastruttura condivisa.
- **`shaped_answer(shape, said)`** — `lib.rs:1240`. Funzione libera, non un
  metodo di `EngineSpec`. Prende una forma e del testo, restituisce un valore
  validato o uno di due errori: `answer_not_json`, `answer_off_shape`.
- **`pruned(shape, value)`** — `lib.rs:1218`. Funzione libera. Taglia dal valore
  tutto ciò che la forma non ha promesso: a valle passa solo il dichiarato.

Un solo pezzo è legato al motore: **`shape_was_asked_for`** (`lib.rs:951`), che
prende `&EngineSpec`. E quello, come si vede più sotto, per un comando non ha
senso di esistere.

## Due decisioni già prese, che non vanno ridiscusse

Chi ha scritto `external_engine` ha già risolto due problemi che si
ripresenterebbero identici. Le sue risposte si ereditano, non si riaprono.

**Uno: misurare non cambia ciò che si misura.** `lib.rs:2043-2049`. Se il
descrittore ha chiesto un involucro per farsi dire il consumo di token, la
risposta si tira fuori dall'involucro e `stdout` torna quello di prima — «un
flusso a valle che dichiara la forma della propria risposta diventerebbe rosso
per una misura che non ha chiesto».

**Due: la tolleranza riguarda il codice d'uscita, non la risposta.**
`lib.rs:2130-2140`. Anche nel ramo `exit_error`, se il passo ha dichiarato una
forma, la forma si applica lo stesso. Perdonare l'uscita non è perdonare una
risposta malfatta.

## L'asimmetria che non si può copiare

`shape_was_asked_for` esiste per un motivo scritto nel suo stesso messaggio
d'errore (`lib.rs:974`): se il passo pretende una risposta in una forma
dichiarata ma **quella forma non compare in ciò che manda al motore**, il passo
si ferma *prima di spendere*. Chiedere senza verificare e verificare senza
chiedere sono lo stesso difetto.

Per un comando questo controllo non ha analogo, e fingerlo sarebbe peggio che
non averlo. `git rev-parse` non riceve la tua forma e non può conformarsi. Il
che apre l'unica domanda di progetto vera di tutta la faccenda:

> Se il comando non sa quale forma vuoi, chi trasforma la sua uscita in un
> valore?

## Le decisioni, prese (01/09/2026)

**1 — Da dove esce il valore.** Solo comandi che **emettono già JSON**.
`shaped_answer` chiama `json_body`: se l'uscita non è JSON, il passo va rosso con
`answer_not_json`, e chi scrive il flusso aggiunge `--json`, `--format=json` o
`| jq`. Scartata l'interpretazione del testo a righe: è un pavimento che cede in
silenzio il giorno che il comando cambia formato.

**2 — Fallimento tollerato (`accept: ["failed"]`).** Nessun valore. La forma è
pretesa solo con `status: "ok"`; su fallimento tollerato `answer` è assente e chi
sta a valle ramifica sullo stato — cosa che già deve fare, altrimenti non avrebbe
scritto `accept`. Il motore fa diversamente perché un motore che fallisce ha
comunque parlato; un comando fallito non ha prodotto la lettura richiesta, e
lasciar passare un valore lì dentro vorrebbe dire leggere da uno strumento rotto.

**3 — Un'azione che impara.** `shell_check` impara `answer_shape`. Nessuna
`shell_read` separata: `CheckSpec` (`lib.rs:2333`) ha già `command`, `env`,
`accept`, `timeout_secs`, `workdir`, e una seconda azione li duplicherebbe tutti
— due copie, col tempo, divergono. La presenza di `answer_shape` è ciò che
dichiara «questo passo legge».

**4 — Il valore si pota.** `pruned` è già scritto e già usato dal motore: si
applica anche qui. A valle passa solo ciò che la forma ha dichiarato, così
nessuno più a valle può appoggiarsi a un campo che nessuno aveva promesso.

**5 — Il tetto sul volume: rosso, non troncamento.** Sopra la soglia il passo si
ferma e lo dice. Soglia proposta: **~1 MB** (un libro di seicento pagine) — larga
per un uso vero, stretta abbastanza da prendere gli incidenti. Troncare è stato
scartato: un valore mozzato sembra intero, e chi legge dopo non ha modo di
saperlo.

> **Da sapere prima di scriverlo**: cercato in tutto `crates/` — sailor **non ha
> alcun limite sul volume di testo**, né nelle azioni né nel deposito né nel
> registro. L'unico tetto esistente è sul *tempo* (`run_with_timeout`, provato da
> `a_slow_command_is_killed_at_the_limit`): un comando lento viene ucciso, un
> comando logorroico no. Questo sarebbe il primo limite di volume del progetto,
> quindi non c'è una convenzione da seguire — c'è da fondarla.

**6 — La regola di sicurezza va in codice nello stesso lavoro.** Non prima, non
dopo: insieme. La porta nuova nasce già chiusa. Il perché è qui sotto.

## Il pericolo che cresce, e che oggi nessuno ferma

`lib.rs:2359`, nella documentazione di `shell_check`:

> **Un rinvio a ciò che ha detto un motore va in `env`, mai in `command`.** Il
> comando è testo di shell e viene eseguito; una risposta di modello incollata lì
> dentro è un comando scritto da chi ha risposto.

**È solo un commento.** Cercato in tutto `crates/`: nessun codice lo applica,
nessuna prova lo copre. Oggi il rischio è contenuto perché le uscite che possono
finire in un `command` vengono da un motore, e chi scrive flussi lo sa.

Nel momento in cui un comando restituisce valori, la stessa strada si apre a
partire dall'uscita di un comando — e quella sembra innocua proprio perché non
viene da un modello. Un `git log` su un ramo il cui nome è stato scelto da
qualcun altro è testo di qualcun altro.

La regola giusta non è più «ciò che ha detto un motore», ma:

> **Ciò che viene da fuori — motore o comando — va in `env`, mai in `command`.**

E finché resta un commento, non è una regola: è un augurio. Se si aggiunge la
lettura, questa è la cosa da mettere in codice **nello stesso lavoro**, non dopo.

## La prova che va rossa per prima

Prima di scrivere una riga di implementazione, la prova che dice cosa si sta
costruendo:

> Un passo che esegue un comando e dichiara la forma della propria lettura
> consegna a valle `answer` validata e potata; se il comando non emette JSON il
> passo va rosso con `answer_not_json`; se emette JSON fuori forma, con
> `answer_off_shape`; e `stdout` grezzo non compare mai nell'uscita del passo.

L'ultima clausola non è un dettaglio. La prova
`an_engine_step_declares_what_it_can_return_and_what_it_hands_on`
(`crates/sailor/tests/dispatch_the_work.rs:229-291`) già pretende che un passo
motore non inoltri `stdout`: consegna `answer`, o niente. Una lettura da comando
che facesse passare il testo grezzo accanto al valore romperebbe quella scelta
invece di ereditarla — e sarebbe la scorciatoia che rende inutile tutto il resto.

E la seconda, che nasce dalla decisione 6 — oggi non esiste nemmeno per il motore:

> Un passo che monta ciò che un altro passo ha letto **dentro `command`** non
> parte: va rosso prima di eseguire. In `env` passa; in `command` no. Vale per
> ciò che ha detto un motore e per ciò che ha stampato un comando, senza
> distinzione — la seconda sembra più innocua ed è esattamente per questo che va
> trattata uguale.
