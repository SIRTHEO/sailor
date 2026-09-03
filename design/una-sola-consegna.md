# Una sola consegna — la finestra di Sailor

Questo file è **un prompt**, non una relazione. Si dà intero a un motore che non
ha mai visto questo albero, e basta da solo: dentro ci sono la carta, la forma,
la misura e il modo di chiudere. Chi lo esegue non deve fare domande.

Sta qui e non in un flusso perché un prompt che vive in un solo posto si
correggge in un solo posto. `porta-una-superficie-alla-carta` lo legge dal
deposito `design-charter`; una persona lo legge da qui; sono lo stesso testo.

---

## 1. Cosa consegni

Una passata sulla finestra di `desktop/`, che alza la superficie di un gradino
e lascia l'albero verde. Non un rifacimento: **una voce per corsa**. Alla fine:

- l'albero passa `npm test` in `desktop/` e `cargo test` alla radice;
- `npm run screenshots` e `npm run check:canvas` girano e le loro uscite sono
  aggiornate in `desktop/design/screenshots/`;
- un commit per pezzo che sta in piedi da solo, con scritto **perché**;
- il verdetto in fondo a questo file, compilato.

Se una parte è bloccata, si fa tutto il resto e si dice cosa è rimasto fuori.
Ridurre l'ambito da soli non è una decisione di chi esegue.

## 2. Che cos'è questo prodotto

Sailor guarda macchine che lavorano. Un **flusso** è un grafo di passi; un
**passo** finisce in uno di sei modi; una **corsa** è un flusso eseguito, con
quello che è costato. La finestra sta aperta tutto il giorno accanto a un
terminale.

Da qui discende tutto il resto: *bello* qui vuol dire **leggibile**. La cosa
che deve brillare sullo schermo è il lavoro in corso, non la cornice.

## 3. La carta

Il fondo è la notte. `:root` è lo schema scuro; `@media (prefers-color-scheme:
light)` porta il gemello diurno. Non esiste una terza tavolozza: ogni ruolo che
un componente di libreria chiede è un **alias** di un ruolo del foglio.

I dodici divieti stanno scritti in testa a `desktop/src/styles.css` e non si
riassumono qui: si leggono là, dove valgono. Quelli che si sbagliano più spesso:

1. **Tre ruoli di carattere, due facce.** Niente `system-ui` per omissione.
2. **Due raggi e una pillola.** La pillola è delle targhe di stato, di nient'altro.
3. **Una sola ombra**, e solo per ciò che galleggia davvero.
4. **Il colore è riservato allo stato della macchina**, più *un* accento che
   significa «l'azione». Un secondo accento è un secondo significato.
5. Il colore non porta mai uno stato da solo: la parola gli sta accanto.
6. **Nessuna coppia testo/fondo sotto 4,5:1.** Misurata, non stimata.
7. **Due livelli di testo, non tre.** L'enfasi è peso, maiuscoletto, spaziatura.
9. Niente sfumature, vetro smerigliato, sfocature.
11. **Una colonna rigida dichiara come si comporta da stretta.**
12. Il fondo non è mai nero puro.

## 4. La forma

Una colonna sola a sinistra, come Orca. La colonna **è il mondo**; l'area
principale mostra una cosa per volta, a tutta larghezza.

```
⌘K  cerca o lancia un comando
──────────────────────────────
▮ Terminals   ▤ Ledger   ◷ Runs   ⚓ Sailor      ← ciò che vale ovunque
──────────────────────────────
WORKSPACES
▾ un-progetto
   ▾ un-albero  ●                                ← l'albero in cui stai
        ◈ Board                    31
        ⌁ un-flusso              7 passi
        + New flow
     un-altro-albero                             ← stesso progetto, altro ramo
──────────────────────────────
FLOWS EVERYWHERE
   yours    ⌁ …
   built in ⌁ …                                  ← i flussi di sistema
──────────────────────────────
OUTSIDE EVERY WORKSPACE
   ▮ un-terminale                                ← fuori è un posto, non un'assenza
```

Quattro requisiti, e sono del committente:

1. **I workspace ci sono**, e i flussi di un workspace stanno con la sua lavagna.
   Un workspace ha **più alberi** (un checkout per ramo): il nome raggruppa, il
   percorso è l'identità.
2. **I flussi di sistema hanno un posto.** Sepolti fra quelli di un checkout si
   leggono come di quel checkout.
3. **Esiste una vista di tutti i flussi globali collegati**: nodi = flussi,
   archi = chiamate `subflow`. Questa è l'unica cosa dell'elenco che ancora non
   esiste.
4. **I terminali ci sono, come in Orca**, e un terminale **può stare dentro un
   workspace o fuori**. Fuori è un posto con una testata sua.

## 5. Che cosa vuol dire «forte»

Il pavimento lo misurano le prove: un carattere fuori dai ruoli, un'accoppiata
sotto soglia, una classe che nessuna regola veste. **Nessuno misura il
soffitto**, e un motore che lavora contro una batteria produce il minimo che
passa — corretto e piatto.

Quindi si guarda lo schermo e si giudica su **quattro assi separati**, mai su
uno solo (a un asse solo il giudizio concorda con l'occhio umano il 48,5% delle
volte; a quattro il 69,5%):

| asse | la domanda |
|---|---|
| `visual_task` | la scena fa vedere ciò per cui esiste? |
| `aesthetic` | sembra decisa da qualcuno? |
| `code_task` | il codice fa ciò che la scena promette? |
| `code_quality` | e lo fa in un modo che si rilegge? |

Ogni scena porta anche `lowest_because`: **una frase su cosa l'ha tenuta bassa**.
Un punteggio senza quella frase non è un giudizio, è un numero.

### I segni del minimo che passa

Se ne trovi uno, è lavoro, non gusto:

- una gerarchia che non c'è: tre pesi che valgono uguale;
- ritmo verticale a caso — spazi che non stanno su una griglia;
- lo stesso nome scritto due volte nella stessa barra;
- un'**azione distruttiva in evidenza** dove va un'azione qualunque;
- un bordo attorno a tutto: il riquadro usato al posto della spaziatura;
- una colonna che tronca invece di dare la sua larghezza a chi le serve;
- il vuoto, il caricamento e il guasto non disegnati: sono gli stati che
  nessuno guarda, ed è lì che il default sopravvive più a lungo;
- una libreria di componenti installata e mai usata.

## 6. Il ciclo

Cinque passate, e **si tiene la migliore, non l'ultima**. Riscrivere senza
riguardare lo schermo vale +1,5%; **riguardarlo a ogni passata vale +17,8%**, e
metà di quel guadagno sta nel trattenere la passata migliore, perché la
traiettoria non sale sempre.

Su ogni passata, in quest'ordine:

1. **ridisegna** — `npm run screenshots`;
2. **guarda** — prima l'albero (`*.aria.txt`, poche decine di kB per tutte le
   scene), poi solo le immagini delle scene che l'albero non spiega;
3. **segna i quattro assi** e la frase del più basso;
4. **cambia una cosa sola** e torna al punto 1.

Mentre l'aspetto migliora del 26,3% la qualità del codice cala del 3,2%: per
questo **le prove chiudono sempre**, e girano **due volte** — prima che si
tocchi qualcosa, per sapere cos'era già rosso, e dopo, dove non si tollera
niente.

## 7. Le misure, e chi le fa

| comando | dove | cosa risponde |
|---|---|---|
| `npm test` | `desktop/` | il pavimento della finestra |
| `npm run screenshots` | `desktop/` | dodici scene, immagine + albero |
| `npm run check:canvas` | `desktop/` | la tela esiste a ogni larghezza? |
| `cargo test` | radice | tutto il resto, cricchetti compresi |

`npm run screenshots` esce 1 solo se una scena è mancata **senza** che il
prodotto dichiari il varco. Un varco dichiarato (`product gap: …`) è un fatto
messo a verbale, non un fallimento della cattura: si lascia, e si nomina.

## 8. I divieti di processo

- **Il cancello prima della riparazione.** Se una passata prevede un controllo,
  si scrive *prima* di riparare il difetto che deve intercettare, e **lo si
  guarda fallire**. Un cancello scritto dopo nasce verde e non ha mai dimostrato
  di poter vedere niente.
- **Un rosso che c'era già non è tuo.** Non si ripara: non è il lavoro scelto.
  Ma si dice che c'era.
- **Un cricchetto scende soltanto.** Se una misura è calata, si riscrive col
  numero misurato; non si alza mai per far passare qualcosa.
- **Non si cancella un controllo perché dà fastidio: lo si adatta.** Se un
  cancello va rosso dopo un cambiamento, la prima ipotesi è che abbia ragione.
- **Niente colore letterale in un componente**: un esadecimale dentro un `.tsx`
  non risponde a nessuno schema.
- **Un commit per pezzo che sta in piedi da solo**, e il messaggio dice il
  perché, non il cosa. Il cosa sta nel diff.

## 9. Come si chiude

```json
{
  "passate": 5,
  "punteggi_per_passata": [
    {"passata": 1, "visual_task": 0, "aesthetic": 0, "overall": 0, "cosa_e_cambiato": ""}
  ],
  "tenuta": 4,
  "perche_tenuta": "",
  "ho_ridisegnato_ogni_passata": true,
  "toccati": ["percorso/di/un/file"],
  "cancello_scritto": "il nome della prova, e cosa ha visto fallire",
  "rossi_gia_presenti": [],
  "rimasto_fuori": []
}
```
