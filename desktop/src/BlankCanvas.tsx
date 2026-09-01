// The canvas with no flows. This is how the product opens the first time, and
// it is the state most often got wrong: an empty window with an empty minimap
// and controls with nothing to control reads as a failure, and the gesture that
// is needed is nowhere. Here the blank canvas says what a flow is and offers it.

// THREE STATES, THREE DIFFERENT SHAPES. Asking, engine not answering, and
// nothing there yet lead to different things: in the second a flow created now
// could not be saved, so offering the gesture would be an unkeepable promise.

export type CanvasState = "loading" | "failed" | "empty";

interface BlankCanvasProps {
  state: CanvasState;
  /** Why the engine is not answering, word for word: shown, not summarised. */
  failure?: string | null;
  brokenCount: number;
  onCreate: () => void;
}

/**
 * The skeleton's lanes, and how many steps each pretends to have. Two lanes of
 * different length, not two equal ones: a symmetric skeleton reads as a
 * decorative pattern, a lopsided one as content on its way. The numbers promise
 * nothing, which is why the shape carries no words.
 */
const SKELETON_LANES = [3, 4];

export function BlankCanvas({ state, failure, brokenCount, onCreate }: BlankCanvasProps) {
  /**
   * **LOADING IS A SHAPE, NOT A SPINNER.** A spinner says "wait"; a skeleton
   * says WHAT IS COMING, so the screen reads before the data lands. It does not
   * move: the sheet keeps its two durations for motion that confirms a gesture,
   * and there is none here. The word under it says loading, as rule 5 asks.
   */
  if (state === "loading") {
    return (
      <div className="blank" data-state="loading">
        <div className="blank__skeleton" aria-hidden="true">
          {SKELETON_LANES.map((steps, lane) => (
            <div className="blank__lane" key={lane}>
              <div className="blank__head">
                <div className="blank__plate blank__plate--name" />
                <div className="blank__plate blank__plate--desc" />
              </div>
              <div className="blank__row">
                {Array.from({ length: steps }, (_, step) => (
                  <div className="blank__plate blank__plate--step" key={step} />
                ))}
              </div>
            </div>
          ))}
        </div>
        <p className="blank__waiting">Chiedo i flussi al motore…</p>
      </div>
    );
  }

  if (state === "failed") {
    return (
      <div className="blank" data-state="failed">
        <div className="blank__card">
          <h2>Il motore non risponde.</h2>
          <p>
            La tela è vuota perché nessuno ha potuto dire cosa c'è sul disco, non perché il disco
            sia vuoto. Finché il motore tace, un flusso creato adesso non si potrebbe salvare.
          </p>
          {failure && <p className="blank__why">{failure}</p>}
        </div>
      </div>
    );
  }

  /**
   * **THE EMPTY STATE TEACHES THE FIRST GESTURE**, and to teach it, it must
   * name places that exist. A false instruction costs more than silence,
   * because whoever follows it concludes the product is broken.
   */
  return (
    <div className="blank" data-state="empty">
      <div className="blank__card">
        <h2>Qui non c'è ancora niente. Cominciamo.</h2>
        <p>
          Un flusso è una catena di passi: ognuno fa una cosa sola e dice da quali altri dipende.
        </p>
        {/* `role="list"` non è un di più: il foglio toglie i pallini con
            `list-style: none`, e con quella riga alcuni lettori di schermo
            smettono di annunciare «elenco di tre». I numeri qui li disegna il
            foglio, quindi senza il ruolo l'ordine dei gesti si perderebbe
            proprio per chi non lo vede. */}
        <ol className="blank__gestures" role="list">
          <li>Crea il flusso: nasce vuoto, e resta qui finché non lo salvi.</li>
          <li>Aggiungi i passi dalla barra che compare in fondo alla tela.</li>
          <li>Collegali trascinando da una porta all'altra: il filo è la dipendenza.</li>
        </ol>
        {brokenCount > 0 && (
          <p className="blank__why">
            {brokenCount === 1
              ? "Un flusso sul disco non si carica: è in fondo alla colonna, col motivo."
              : `${brokenCount} flussi sul disco non si caricano: sono in fondo alla colonna, col motivo.`}
          </p>
        )}
        <button type="button" className="is-primary" onClick={onCreate}>
          Crea il primo flusso
        </button>
      </div>
    </div>
  );
}
