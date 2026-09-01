import type { ReactElement } from "react";
import { Panel } from "@xyflow/react";

import { KIND_LABEL } from "./StepNode";
import { DEFAULT_ACTION_FOR_KIND, type StepKind } from "./flow";

/**
 * **LA CASSETTA STA DENTRO LA TELA, NON NELLA COLONNA ACCANTO.**
 *
 * Prima era una griglia di bottoni in fondo a `.rail`: chi componeva un flusso
 * usciva dalla tela col puntatore, premeva, e rientrava. Il gesto attraversava
 * il confine fra ciò che si guarda e ciò che si comanda, e lo attraversava a
 * ogni passo.
 *
 * **Perché in basso, e perché dentro `Panel`.** La tela è pan/zoom infinito:
 * una barra disegnata nella tela scorrerebbe via con essa alla prima
 * trascinata. `Panel` di React Flow disegna fuori da `.react-flow__viewport` —
 * l'elemento che porta la `transform` — quindi la barra sta *sopra* la tela e
 * non *nella* tela. Non è una scelta di stile: è l'unico posto in cui una barra
 * può stare dentro il riquadro senza scappare. Una prova lo interroga
 * (`Toolbar.test.tsx`), perché a occhio le due cose sono identiche finché
 * qualcuno non trascina.
 *
 * **`bottom-left`, non `bottom-center`, e non è estetica.** La fascia bassa ha
 * già due inquilini: i comandi di zoom a sinistra (41px dal fianco) e la
 * minimappa a destra (215px). Una barra centrata sulla tela può essere larga
 * al massimo il doppio del lato più stretto — e sotto i 1418px di finestra
 * quella misura scende sotto i 468px che gli attrezzi occupano, cioè la barra
 * copre la minimappa. È successo due volte in questa lavorazione, con due
 * larghezze diverse, e tutte e due le volte il rimedio è stato spostare un
 * numero. Ancorata al fianco, la barra parte dopo i comandi e il suo tetto
 * finisce prima della minimappa: il corridoio è dichiarato in `styles.css`
 * (`--controls-reserve`, `--minimap-reserve`) e la prova rifà il conto senza
 * misurare un pixel, perché il conto non dipende dalla larghezza.
 *
 * Una striscia verticale sul fianco sinistro sarebbe entrata dove cominciano
 * le corsie, e — peggio — avrebbe riletto come una seconda colonna: cioè come
 * la cosa da cui questa barra se ne va.
 */

/**
 * Le famiglie, raggruppate per **chi fa il lavoro**, che è la sola distinzione
 * che cambia cosa succede quando il passo gira.
 *
 * Sette bottoni in fila sono un muro. Sette in tre gruppi — uno, tre, tre — si
 * leggono. Il taglio non è estetico: un passo `engine`, `human` o `subflow`
 * consegna il lavoro a qualcosa che sta fuori (una riga di comando, un agente
 * già vivo, un altro flusso), mentre `check`, `gesture` e `deposit` li esegue
 * Sailor stesso senza chiedere niente a nessuno. Chi compone sceglie prima fra
 * queste tre cose, e solo dopo fra i tre attrezzi del gruppo.
 *
 * **I nomi dei gruppi non sono disegnati, e stanno in `aria-label`.** Tre righe
 * di didascalia costavano un terzo dell'altezza della barra per battezzare
 * gruppi da uno e da tre elementi: il filo verticale e la forma dei segni li
 * dicono già. Chi legge con la voce li sente comunque.
 *
 * **Sette famiglie, e il taglio da nove è ereditato, non fatto qui.** La
 * cassetta di prima leggeva già `Object.keys(DEFAULT_ACTION_FOR_KIND)` e
 * mostrava già sette bottoni: `wait` e `branch` erano già fuori perché non
 * hanno un'azione, e un attrezzo senza azione crea un nodo che poi non si
 * salva. Questa barra continua a leggere la stessa mappa — nessun elenco suo —
 * e ci aggiunge due cose: il **raggruppamento**, e una prova che tiene i gruppi
 * incollati alla mappa **nei due versi** (né un attrezzo senza azione, né una
 * famiglia con azione lasciata fuori). Prima nessuno interrogava quel legame:
 * si reggeva sull'`Object.keys`, e un elenco scritto a mano lo avrebbe rotto in
 * silenzio.
 */
interface ToolGroup {
  /** Ciò che accomuna il gruppo, per chi legge con la voce. */
  label: string;
  kinds: StepKind[];
}

export const TOOL_GROUPS: ToolGroup[] = [
  { label: "Where it starts", kinds: ["trigger"] },
  { label: "Who does the work instead of Sailor", kinds: ["engine", "human", "subflow"] },
  { label: "What Sailor does itself", kinds: ["check", "gesture", "deposit"] },
];

/** Le famiglie che la barra offre, nell'ordine in cui si vedono. */
export const TOOLBAR_KINDS: StepKind[] = TOOL_GROUPS.flatMap((group) => group.kinds);

/**
 * Il segno di una famiglia: **la forma disegna il gesto**, non decora.
 *
 * L'etichetta resta sotto e non sparisce — è il divieto 5 applicato alla forma:
 * il segno da solo non porta niente, come non lo porta il colore da solo. E
 * infatti qui il colore non c'è: `currentColor` e basta, perché la tinta in
 * questo prodotto è riservata allo stato di una macchina, e un attrezzo fermo
 * in una cassetta non ha stato.
 *
 * Tratto squadrato e non arrotondato: è il segno di un disegno tecnico, la
 * stessa mano dei quattro tagli d'angolo sul nodo scelto.
 */
function KindMark({ kind }: { kind: StepKind }) {
  const shape = MARK[kind];
  return (
    <svg
      className="toolbar__mark"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="square"
      strokeLinejoin="miter"
    >
      {shape}
    </svg>
  );
}

/**
 * Sette segni, ognuno con due o tre primitive.
 *
 * La freccia che esce vuol dire «il lavoro se ne va da qui»: chi la porta è un
 * passo che consegna, e ciò che la freccia raggiunge dice a chi — un altro
 * programma (il riquadro), una persona (la testa e le spalle), un altro flusso
 * (i suoi passi impilati). Chi non la porta lavora sul posto.
 */
const MARK: Record<StepKind, ReactElement> = {
  // La barra da cui parte tutto, e il segnale che ne esce.
  trigger: (
    <>
      <path d="M2.7 2.5v11" />
      <path d="M5.5 4l8 4-8 4z" fill="currentColor" stroke="none" />
    </>
  ),
  // Il lavoro esce ed entra in un altro programma.
  engine: (
    <>
      <path d="M1.5 8h4.5" />
      <path d="M4 5.5L6.5 8 4 10.5" />
      <rect x="9" y="3.2" width="5.5" height="9.6" />
    </>
  ),
  // Il lavoro esce e lo prende una persona.
  human: (
    <>
      <path d="M1.5 8h4.5" />
      <path d="M4 5.5L6.5 8 4 10.5" />
      <circle cx="11.6" cy="5.4" r="2.1" />
      <path d="M8.4 13.2c0-2 1.5-3.4 3.2-3.4s3.2 1.4 3.2 3.4" />
    </>
  ),
  // Il lavoro esce e lo prende un altro flusso, coi suoi passi.
  subflow: (
    <>
      <path d="M1.5 8h4.5" />
      <path d="M4 5.5L6.5 8 4 10.5" />
      <path d="M9.2 4h5.3M9.2 8h5.3M9.2 12h5.3" />
    </>
  ),
  // Il richiamo di una riga di comando: il segno d'invito di una shell.
  check: (
    <>
      <path d="M2.2 3.6L6.4 8l-4.2 4.4" />
      <path d="M7.6 12.4h6.2" />
    </>
  ),
  // Due estremi che si toccano: la domanda a un servizio collegato.
  gesture: (
    <>
      <circle cx="3.9" cy="8" r="2.1" />
      <circle cx="12.1" cy="8" r="2.1" />
      <path d="M6 8h4" />
    </>
  ),
  // Il tamburo del deposito: ciò che resta scritto.
  deposit: (
    <>
      <path d="M2.6 4.2v7.6c0 1.1 2.4 2 5.4 2s5.4-.9 5.4-2V4.2" />
      <ellipse cx="8" cy="4.2" rx="5.4" ry="2" />
    </>
  ),
  // Le due famiglie senza azione: nessuna azione del motore vi si risolve,
  // quindi la barra non le offre e questi segni non si disegnano mai. Stanno
  // qui perché la mappa sia totale e il compilatore se ne accorga se un giorno
  // un'azione arriva.
  wait: <path d="M8 2.5v5.5l3.5 2.5" />,
  branch: (
    <>
      <path d="M2.5 8h4" />
      <path d="M6.5 8L12 3.5M6.5 8L12 12.5" />
    </>
  ),
};

interface ToolbarProps {
  /** Il flusso che riceve il passo, o `null` se nessuno è a fuoco. */
  flowName: string | null;
  onAdd: (kind: StepKind) => void;
  onNewFlow: () => void;
}

/**
 * **SENZA UN FLUSSO A FUOCO LA BARRA NON SI SPEGNE: CAMBIA MESTIERE.**
 *
 * Prima erano sette bottoni `disabled` con un `title` che spiegava. Un `title`
 * si vede dopo un secondo di attesa, col puntatore fermo sopra la cosa che non
 * funziona: chi non sa che c'è non lo cerca. E sette bottoni spenti occupano
 * lo spazio di sette gesti offrendone zero.
 *
 * Qui la barra si accorcia a una riga sola che dice cosa manca e porta il gesto
 * che lo risolve. È lo stesso principio della tela vuota, che insegna il primo
 * gesto invece di dire «nessun dato».
 */
export function Toolbar({ flowName, onAdd, onNewFlow }: ToolbarProps) {
  if (flowName === null) {
    return (
      <Panel position="bottom-left" className="toolbar">
        <p className="toolbar__prompt">
          Scegli un flusso nella colonna per aggiungere passi.
          <button type="button" className="toolbar__new" onClick={onNewFlow}>
            + Nuovo flusso
          </button>
        </p>
      </Panel>
    );
  }

  return (
    <Panel position="bottom-left" className="toolbar">
      {/* DOVE VA A FINIRE IL PASSO, scritto prima di premere. La tela mostra
          tutti i flussi insieme: senza questa riga il passo nuovo comparirebbe
          in una corsia qualunque delle tante, e capire quale è un indovinello
          che si risolve dopo. */}
      <div className="toolbar__target">
        Aggiungi a <span className="toolbar__target-name">«{flowName}»</span>
      </div>
      <div className="toolbar__row">
        {TOOL_GROUPS.map((group) => (
          <div className="toolbar__group" key={group.label} role="group" aria-label={group.label}>
            {group.kinds.map((kind) => (
              <button
                key={kind}
                type="button"
                className="toolbar__tool"
                data-kind={kind}
                onClick={() => onAdd(kind)}
              >
                <KindMark kind={kind} />
                <span className="toolbar__label">{KIND_LABEL[kind]}</span>
              </button>
            ))}
          </div>
        ))}
      </div>
    </Panel>
  );
}

/**
 * Le famiglie che la mappa delle azioni predefinite conosce, per la prova che
 * confronta i gruppi con essa. Sta qui e non nella prova perché è la stessa
 * lettura che fa la barra: una prova che ricopiasse l'elenco proverebbe la
 * propria copia.
 */
export const KINDS_WITH_ACTION = Object.keys(DEFAULT_ACTION_FOR_KIND) as StepKind[];
