# `dispatch-the-work` — il flusso che divide un incarico fra tre motori

Un **innesco** riceve la consegna e la mette a disposizione del grafo. Un nodo
la divide in due incarichi che si reggono da soli. Due motori li eseguono
**senza vedersi**. Un terzo modello legge gli incarichi e le due
risposte e dà un giudizio. Un ultimo nodo trasforma quel giudizio in un esito:
passa, o il flusso è rosso.

## I sei nodi

| nodo | strumento chiesto | dipende da | cosa fa |
|---|---|---|---|
| `trigger` | — | — | attende un segnale e ne mette a disposizione il testo, chi l'ha mandato e da dove |
| `dispatch` | `claude-code` | `trigger` | divide la consegna in due incarichi: `first_engine`, `second_engine` |
| `engine_a` | `codex` | `dispatch` | esegue il primo incarico, sola lettura |
| `engine_b` | `agy` | `dispatch` | esegue il secondo incarico, sola lettura |
| `verify` | `claude-code` | i tre sopra | legge tutto e scrive `verdict` e `why` |
| `verdict` | — (`shell_check`) | `verify` | accetta **solo** l'approvazione: qui il flusso diventa rosso o verde |

`engine_a` e `engine_b` non si vedono: ognuno riceve soltanto
il proprio incarico, e non ha mai visto la consegna intera né la risposta
dell'altro.

## Le quattro regole che questo file rispetta

**1. Nel flusso sta *come* si smista, non *cosa*.** Nessun passo porta un
incarico scritto dentro. Il lavoro entra tutto dall'innesco: per darne un altro
si cambia il testo in `inputs.trigger.text` — o si preme il pulsante nella
finestra, che scrive lo stesso campo — e il grafo non si tocca.

**2. Nessun passo nomina un binario.** Ogni passo che esegue un motore dichiara
`"tool": "<identificativo>"`, lo stesso che il rilevatore degli strumenti
(`crates/toolbox`) restituisce, e chi esegue lo risolve sulla macchina di chi
lancia. Su una macchina dove quello strumento non c'è, il passo si ferma
**prima di spendere qualunque cosa** e dice quale mancava e dove ha cercato.
Prima del 28/08/2026 c'era scritto `"bin": "claude"`, e quel flusso girava solo
dove quel nome era nel percorso di chi eseguiva.

**3. Nessun passo è speciale.** Il verificatore non è un nodo di un tipo suo: è
un passo come gli altri, che riceve un lavoro da verificare. Chi lo esegue si
cambia con una riga (`tool`), e il ruolo è scritto nel prompt.

**4. Ogni passo dichiara la forma della propria risposta.** In `answer_shape`.
Quella forma finisce nel prompt del motore — con un rinvio `{"$json":
"/answer_shape"}`, scritta una volta sola, così le due copie non possono
divergere — e viene fatta rispettare sulla risposta. Al passo dopo passa
**solo** ciò che la forma dichiara: i preamboli, i ragionamenti e i saluti non
attraversano la catena e non si pagano a ogni chiamata a valle.

## Quando questo flusso diventa rosso

Un passo si rompe, e chi dipende da lui non parte, se il motore:

- esce con un codice diverso da zero (`engine_exit_error`);
- non risponde entro il tetto di tempo (`engine_timed_out`);
- non parte (`engine_spawn_failed`);
- non c'è su questa macchina (`tool_unavailable`);
- risponde qualcosa che non è JSON (`answer_not_json`) o che non rispetta la
  forma dichiarata (`answer_off_shape`).

E in fondo, se il verificatore respinge, il nodo `verdict` chiude in rosso.

Un passo può dichiarare che un esito è accettabile — `"accept": ["exit_error"]`
— per chi esegue un controllo apposta per vederlo fallire. Nessun passo di
questo flusso lo fa, e se lo facesse dovrebbe dirlo anche nel proprio schema
d'uscita, dove si vede leggendo il grafo.

## Come si lancia

Dalla radice dei sorgenti di Sailor, qualunque sia sulla tua macchina:

```bash
cargo run -p sailor -- flow run dispatch-the-work
```

La cartella conta: il comando cerca i flussi in `flows/` sotto quella corrente.
Per guardarlo senza eseguirlo: `cargo run -p sailor -- flow check dispatch-the-work`.

## Una cosa che questo flusso sembrava dire e non è vera

Fino al 28/08/2026 questo documento e la descrizione del flusso dicevano che i
due motori girano «insieme». **Non è così, ed è misurato**: l'esecutore percorre
il fronte dei passi pronti **in ordine, uno dopo l'altro**. Due passi da sei
secondi ne impiegano dodici, non sei.

Il codice non lo nasconde — `crates/flow/src/executor.rs` lo dichiara nel punto
esatto: «questo esecutore lo percorre in ordine: l'esecutore di processi potrà
avviarlo in parallelo». Erano il flusso e questo documento a dire un'altra cosa.

Resta vero che i due motori **non si vedono**: nessuno dei due riceve la
risposta dell'altro, e questo è ciò che rende il verdetto di `verify` un
giudizio su due lavori indipendenti. È «insieme» che descriveva un parallelismo
che non c'è, e il costo è il tempo: due motori di intelligenza artificiale in
fila fanno aspettare la somma, non il massimo.

## La prima corsa vera, 28/08/2026 — e cosa ha insegnato

Esito: **rosso**, e per la ragione giusta. La catena ha girato tutta, quindi il
motore, lo smistamento e il passaggio dei valori fra i passi **funzionavano**.
A cadere era il contenuto: i due motori erano usciti in errore con l'uscita
vuota, e il verificatore aveva scritto che non c'era niente da verificare.

Ma il difetto vero era un altro, e valeva più dei due errori d'uso: **i due
passi falliti erano stati registrati come andati a buon fine**, con
`status: exit_error` dentro il risultato. Il flusso era diventato rosso solo
perché *l'ultimo* nodo guardava anche lo stato dei motori — una rete che
qualcuno poteva togliere senza accorgersene.

Adesso non è più così, e il nodo finale non guarda più gli stati altrui: non
può nemmeno vederli, perché un passo rotto non arriva a lui.

### Cosa è stato misurato dopo, riga di comando per riga di comando

- **`agy`**: il prompt va in un **argomento**, non sull'ingresso, e `--mode` va
  **prima** di `--print`. `agy --mode plan --print '<prompt>'` risponde ed esce
  0 (provato il 28/08/2026). La forma vecchia era `--print --mode plan`, dove
  `--print` prendeva `--mode` come proprio prompt.
- **`codex exec`**: legge il prompt da stdin quando non è un argomento
  (`codex exec --help`, misurato). Il fallimento della prima corsa non è ancora
  spiegato: da riprovare a mano prima della prossima corsa.
- **`claude`**: invariato, `-p --model <nome>` con il prompt sull'ingresso.

## L'innesco: cosa è vero e cosa no

Il nodo `trigger` è il vero ingresso del flusso, e le sorgenti di segnale sono
un **elenco di descrittori** (`crates/trigger/descriptors/default.json`), non
codice: si aggiungono scrivendo una riga di JSON in `~/.config/sailor/triggers.d/`.

- **`manual`** — vero e funzionante. Qualcuno preme e parte, portando un testo.
  È la sorgente che il pulsante di lancio della finestra userà.
- **`sailor-terminal`**, **`orca-terminal`** — dichiarati e **non ascoltati**.
  Un passo che li usa si rompe con un messaggio che dice cosa manca. Non c'è
  nessun ascolto simulato: un segnale finto farebbe partire i motori a valle, e
  costa chiamate vere.

Perché l'ascolto di un terminale non è realizzabile onestamente oggi:

1. **Nessun processo di Sailor resta in piedi.** `sailor flow run` esegue il
   grafo una volta e finisce: non c'è nessuno che aspetti un segnale e faccia
   partire una corsa quando arriva. Questo manca **prima** di qualunque lettore.
2. **Il terminale di Sailor non esiste ancora**: nessuno scrive il file che il
   descrittore dichiara. Il percorso lì scritto è la forma che avrà, non una
   misura.
3. **Il registro dei pannelli di Orca non è una sorgente onesta** (misurato il
   28/08/2026): `terminal-history/*/output.log` è un formato binario a frame con
   dentro byte di terminale ANSI — ridisegni di schermo, non messaggi — svuotato
   sul posto oltre i 5 MB e scritto a lotti ogni ~5 secondi. Un lettore in coda
   perde contenuto senza accorgersene, e ricostruire il testo vorrebbe dire
   riscrivere un emulatore di terminale. L'unica via supportata è
   `orca terminal read --json --cursor N`, che restituisce testo: è la forma
   dichiarata nel descrittore, e serve un lettore che conservi il cursore fra
   una corsa e l'altra.

## Il perimetro, dichiarato e non applicato

I due motori sono invocati in sola lettura — `--sandbox read-only` per il primo,
`--mode plan` per il secondo, nessuna opzione che allarghi i permessi ai due
passi che usano Claude. È una dichiarazione negli argomenti, **non un limite che
qualcuno faccia rispettare**: il campo che dovrebbe dire dove un flusso può
scrivere esiste, ma nessuno lo legge. Voce aperta:
`2026-08-28-il-perimetro-di-un-flusso-non-limita-niente.md`.
