# Direzioni di prodotto — segnate, non decise

Theo, 29/08/2026: «per ora segnamolo, poi prenderemo decisioni; concentriamoci
sul core e sulla risoluzione di avere la benzina».

Niente di qui dentro è deciso. Sta scritto perché una direzione dimenticata
diventa una scelta presa per omissione — che è come si sono accumulate le altre
cose scritte in questo repo.

## 1. La finestra: «più simile a n8n, ma con una UI/UX migliore»

Oggi non ci siamo per niente: c'è la tela dei flussi, l'editor di un passo, la
console di una corsa. Nessuna pagina per profili, modelli, fornitori. I nodi non
mostrano il modello che useranno né cosa è entrato dentro di loro.

Il metro resta quello già scritto fra i vincoli permanenti: **un'interfaccia che
nasconde cosa succede è il contrario del prodotto.** «Meglio di n8n» va misurato
lì, non sull'estetica.

## 2. Il processo di sviluppo come flusso

Non solo *cosa* costruiamo, ma **come**: un flusso che integri ciò che si
sviluppa. Da indagare seriamente, e le due domande sono queste:

- **il riutilizzo dei flussi** — un flusso scritto una volta dev'essere usabile
  da un altro senza copiarlo (è `subflow`, che non esiste);
- **come si progetta bene un flusso** — cosa è un nodo, quando si spezza, cosa
  si ripete. Legato alla tassonomia dei nodi: finché il tipo di un nodo non è un
  dato, «progettare bene» non ha un metro.

## 3. Integrare invece di costruire dentro

**Un nodo connesso, non codice nostro.** Vale per la parte di finestra e per
l'orchestrazione: se esiste un progetto vivo che fa quel pezzo, si collega.

**Il progetto è Orca.** Theo, 29/08/2026. Misurato lo stesso giorno:

| | |
|---|---|
| repo | `stablyai/orca`, **pubblico, licenza MIT** |
| peso | 56.742 stelle, ultimo aggiornamento **lo stesso giorno** |
| com'è fatta | Electron + **React**, `xterm` per i terminali, `monaco` per l'editor |
| cosa ha già | schede, terminali, worktree, e un **browser dentro** (`agent-browser`) |
| punti d'estensione | **nessuno trovato** nel pacchetto: sarebbe un fork, non un innesto |

**Il fatto che decide**: la finestra di Sailor è Tauri + React + `@xyflow/react`,
Orca è Electron + React. **Il livello che disegna è lo stesso** — la tela dei
flussi si sposta quasi com'è. A cambiare è il guscio.

E la parte più grossa del piano sparirebbe: i terminali non si costruiscono,
ci sono già — e con essi il browser che Theo voleva «in futuro». Il descrittore
`orca-terminal`, oggi dichiarato e non ascoltato, smetterebbe di essere una
promessa.

**Le due strade, e la scelta è di Theo:**

1. **La tela dentro Orca.** Sailor resta il motore — un binario che Orca chiama —
   e la finestra Tauri sparisce. Si ottiene tutto subito: schede, terminali,
   browser, la rifinitura di un progetto con 56.000 stelle. Il prezzo è che
   Sailor diventa un pannello dentro l'applicazione di qualcun altro, e che un
   fork di un progetto vivo si porta dietro le fusioni per sempre — a meno di
   contribuire a monte, che è la variante seria della stessa strada.
2. **Orca come nodo connesso.** Non si fonde niente: Sailor la guida dalla sua
   riga di comando, che è la via già dichiarata nei descrittori d'innesco. Costa
   poco e non dà né il browser né la finestra migliore.

La prima contraddice «Orca andrà a morire» del 28/08 — e va bene, perché quella
frase nasceva dal non sapere che i sorgenti fossero aperti. **Nessuna delle due
si decide stanotte**: prima la benzina.

## 4. Un browser dentro, come fa Orca

Quando ci saranno i terminali: chi guarda un flusso vede anche il browser che si
apre. È la stessa idea della tracciabilità — vedere cosa è successo, non che sia
successo — applicata a ciò che un flusso fa fuori dalla macchina.

## Cosa viene prima di tutto questo

**La benzina.** Misurare il consumo, riconoscere una quota finita, e sfruttare
le fasce gratuite che i fornitori dichiarano. Senza, ogni corsa dipende da un
solo abbonamento e si ferma quando finisce — come il 29/08.
