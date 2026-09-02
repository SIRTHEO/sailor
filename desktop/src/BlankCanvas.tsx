// The canvas with no flows. This is how the product opens the first time, and
// it is the state most often got wrong: an empty window with an empty minimap
// and controls with nothing to control reads as a failure, and the gesture that
// is needed is nowhere. Here the blank canvas says what a flow is and offers it.

// THREE STATES, THREE DIFFERENT SHAPES. Asking, engine not answering, and
// nothing there yet lead to different things: in the second a flow created now
// could not be saved, so offering the gesture would be an unkeepable promise.

import type { FlowPlace } from "./engine";

export type CanvasState = "loading" | "failed" | "empty";

/** Where the engine looked for flows, once asked; `null` while nobody asked. */
export type PlacesAsk = { state: "ready"; places: FlowPlace[] } | { state: "mute"; why: string } | null;

interface BlankCanvasProps {
  state: CanvasState;
  places?: PlacesAsk;
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

export function BlankCanvas({ state, failure, brokenCount, onCreate, places = null }: BlankCanvasProps) {
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
        <p className="blank__waiting">Asking the engine for the flows…</p>
      </div>
    );
  }

  if (state === "failed") {
    return (
      <div className="blank" data-state="failed">
        <div className="blank__card">
          <h2>The engine is not answering.</h2>
          <p>
            The canvas is empty because nobody could say what is on the disk, not because the disk
            is empty. While the engine is silent, a flow created now could not be saved.
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
        <h2>Nothing here yet. Let us begin.</h2>
        <p>
          A flow is a chain of steps: each does one thing and says which others it depends on.
        </p>
        {/* `role="list"` non è un di più: il foglio toglie i pallini con
            `list-style: none`, e con quella riga alcuni lettori di schermo
            smettono di annunciare «elenco di tre». I numeri qui li disegna il
            foglio, quindi senza il ruolo l'ordine dei gesti si perderebbe
            proprio per chi non lo vede. */}
        <ol className="blank__gestures" role="list">
          <li>Create the flow: it is born empty, and stays here until you save it.</li>
          <li>Add steps from the bar that appears at the foot of the canvas.</li>
          <li>Wire them by dragging from one port to another: the wire is the dependency.</li>
        </ol>
        {brokenCount > 0 && (
          <p className="blank__why">
            {brokenCount === 1
              ? "One flow on the disk will not load: it is at the foot of the column, with the reason."
              : `${brokenCount} flows on the disk will not load: they are at the foot of the column, with the reason.`}
          </p>
        )}
        {/* THE PLACES IT LOOKED IN, WITH THEIR REAL PATHS. The window once said
            «no flows» while four sat one folder away, and from inside there
            was no way to know where it had looked. */}
        {places?.state === "ready" && (
          <ul className="blank__places" role="list" aria-label="where the engine looked">
            {places.places.map((place) => (
              <li className="blank__place" key={place.path} data-missing={!place.exists || undefined}>
                <span className="blank__place-origin">{place.origin}</span>
                <span className="blank__place-path">{place.path}</span>
                <span className="blank__place-count">
                  {place.exists ? `${String(place.count)} found` : "no such folder"}
                </span>
              </li>
            ))}
          </ul>
        )}
        {places?.state === "mute" && <p className="blank__why">Where it looked is not known: {places.why}</p>}
        <button type="button" className="is-primary" onClick={onCreate}>
          Create the first flow
        </button>
      </div>
    </div>
  );
}
