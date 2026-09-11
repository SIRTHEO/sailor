import type { FlowMapReading, FlowNode } from "./flowmapread";

/**
 * **THE MAP OF THE FLOWS: WHICH CALLS WHICH.** Every flow on the machine, laid
 * out by how deep it sits under the ones that call it, each plate naming its
 * callers and its calls so the answer needs no hover. It reads: the only
 * gesture is opening a flow, through [`FlowMapProps.onOpen`].
 */

export type FlowMapAsk =
  | { state: "reading" }
  | { state: "unreadable"; why: string }
  | { state: "read"; reading: FlowMapReading };

export interface FlowMapProps {
  /** The one thing this surface takes: whoever wires it does the fetching. */
  ask: FlowMapAsk;
  onOpen?: (flow: string) => void;
}

const PLATE = "flex w-56 flex-col gap-[var(--space-1)] rounded-[var(--radius)] border border-solid border-[var(--border)] bg-[var(--surface-2)] p-[var(--space-2)] text-left";
const NOTE = "text-[length:var(--text-small)] text-[color:var(--text-2)]";
const PILL = "inline-block rounded-[var(--radius-pill)] px-[var(--space-2)] py-px text-[length:var(--text-small)]";
const HEADING = "text-[length:var(--text-small)] font-semibold tracking-widest text-[color:var(--text-2)] uppercase";

function alone(node: FlowNode): boolean {
  return node.calls.length === 0 && node.calledBy.length === 0;
}

/** Where Sailor's own flows end and the person's begin: a shape, not a tint. */
function Origin({ of: origin }: { of: FlowNode["origin"] }) {
  const worn = origin === "shipped"
    ? "border border-solid border-[var(--border)] text-[color:var(--text-2)]"
    : "bg-[var(--surface-1)] font-semibold text-[color:var(--text-1)]";
  return <span className={`${PILL} ${worn}`}>{origin === "shipped" ? "ships with Sailor" : "your own"}</span>;
}

function Names({ lead, of: names }: { lead: string; of: { to: string; missing: boolean }[] }) {
  if (names.length === 0) return null;
  return (
    <p className={`${NOTE} font-mono`}>
      <span className="text-[color:var(--text-2)]">{lead} </span>
      {names.map((name, at) => (
        <span key={name.to}>
          {at > 0 && ", "}
          <span className={name.missing ? "font-semibold text-[color:var(--failed)]" : "text-[color:var(--text-1)]"}>
            {name.to}
          </span>
          {name.missing && <span className="text-[color:var(--failed)]"> (not there)</span>}
        </span>
      ))}
    </p>
  );
}

/**
 * The loudest line on the surface, and it says something either way: a call
 * naming a file nobody wrote is the one defect a map of flows exists to find,
 * and «there is none» is an answer, where silence is not.
 */
function Verdict({ reading }: { reading: FlowMapReading }) {
  if (reading.broken.length === 0) {
    return (
      <p className="rounded-[var(--radius)] border border-solid border-[var(--border)] p-[var(--space-3)] text-[length:var(--text-mid)] text-[color:var(--text-1)]">
        No flow calls a flow that is not there. Every name a step hands to «subflow» or «for_each» has a file.
      </p>
    );
  }
  return (
    <section
      className="rounded-[var(--radius)] border-2 border-solid border-[var(--failed)] p-[var(--space-3)]"
      data-broken={String(reading.broken.length)}
    >
      <p className="text-[length:var(--text-large)] font-semibold text-[color:var(--failed)]">
        {reading.broken.length === 1
          ? "One step calls a flow that is not there."
          : `${String(reading.broken.length)} steps call a flow that is not there.`}
      </p>
      <ul className={`${NOTE} mt-[var(--space-2)] font-mono`}>
        {reading.broken.map((call) => (
          <li key={`${call.from}/${call.step}`} className="text-[color:var(--text-1)]">
            <span className="font-semibold">{call.from}</span>
            {` — step «${call.step}» (${call.via}) wants `}
            <span className="font-semibold text-[color:var(--failed)]">{call.to}</span>
            {", and no file answers to that name."}
          </li>
        ))}
      </ul>
    </section>
  );
}

function Tally({ reading }: { reading: FlowMapReading }) {
  const web = reading.nodes.filter((node) => !alone(node)).length;
  const said = [
    `${String(reading.nodes.length)} flows`,
    `${String(reading.shipped)} ship with Sailor`,
    `${String(reading.own)} your own`,
    `${String(web)} in the web, ${String(reading.nodes.length - web)} on their own`,
  ];
  return <p className={`${NOTE} font-mono`}>{said.join("  ·  ")}</p>;
}

/** A ring drawn as a ring: the round trip written out and closed on itself. */
function Rings({ reading }: { reading: FlowMapReading }) {
  if (reading.cycles.length === 0) return null;
  return (
    <section className="flex flex-col gap-[var(--space-2)]">
      <h3 className={HEADING}>Rings: these flows call each other round</h3>
      {reading.cycles.map((ring) => (
        <p
          key={ring.join("/")}
          className="rounded-[var(--radius)] border border-solid border-[var(--needs-human)] p-[var(--space-2)] font-mono text-[length:var(--text-small)] text-[color:var(--text-1)]"
        >
          {[...ring, ring[0]].join("  →  ")}
        </p>
      ))}
    </section>
  );
}

function Plate({ node, onOpen }: { node: FlowNode; onOpen?: (flow: string) => void }) {
  return (
    <button type="button" className={PLATE} onClick={() => onOpen?.(node.id)} data-flow={node.id}>
      <span className="font-mono text-[length:var(--text-body)] font-semibold text-[color:var(--text-1)]">{node.id}</span>
      <span className="flex flex-wrap gap-[var(--space-1)]">
        <Origin of={node.origin} />
        {node.inCycle && <span className={`${PILL} border border-solid border-[var(--needs-human)] text-[color:var(--needs-human)]`}>in a ring</span>}
        {node.calls.some((call) => call.missing) && (
          <span className={`${PILL} bg-[var(--danger-soft)] font-semibold text-[color:var(--failed)]`}>calls nothing there</span>
        )}
        {node.calledBy.length === 0 && <span className={`${PILL} border border-solid border-[var(--border)] text-[color:var(--text-2)]`}>nothing calls it</span>}
      </span>
      <Names lead="calls →" of={node.calls} />
      <Names lead="← called by" of={node.calledBy.map((from) => ({ to: from, missing: false }))} />
    </button>
  );
}

/** The layers, deepest caller leftmost: a flow always sits right of its caller. */
function Web({ reading, onOpen }: { reading: FlowMapReading; onOpen?: (flow: string) => void }) {
  const joined = reading.nodes.filter((node) => !alone(node));
  if (joined.length === 0) return null;
  const deepest = Math.max(...joined.map((node) => node.layer));
  const columns = Array.from({ length: deepest + 1 }, (_, layer) => joined.filter((node) => node.layer === layer));
  return (
    <section className="flex flex-col gap-[var(--space-2)]">
      <h3 className={HEADING}>The web: what calls what</h3>
      <div className="flex gap-[var(--space-4)] overflow-x-auto">
        {columns.map((column, layer) => (
          <div key={layer} className="flex shrink-0 flex-col gap-[var(--space-2)]">
            <span className={`${NOTE} font-mono`}>{layer === 0 ? "entered here" : `${String(layer)} deep`}</span>
            {column.map((node) => <Plate key={node.id} node={node} onOpen={onOpen} />)}
          </div>
        ))}
      </div>
    </section>
  );
}

function OnTheirOwn({ reading, onOpen }: { reading: FlowMapReading; onOpen?: (flow: string) => void }) {
  const lone = reading.nodes.filter(alone);
  if (lone.length === 0) return null;
  return (
    <section className="flex flex-col gap-[var(--space-2)]">
      <h3 className={HEADING}>On their own: nothing calls them and they call nothing</h3>
      <div className="flex flex-wrap gap-[var(--space-2)]">
        {lone.map((node) => <Plate key={node.id} node={node} onOpen={onOpen} />)}
      </div>
    </section>
  );
}

function Troubles({ reading }: { reading: FlowMapReading }) {
  if (reading.unread.length === 0 && reading.shadowed.length === 0) return null;
  return (
    <section className={`${NOTE} flex flex-col gap-[var(--space-1)]`}>
      {reading.unread.length > 0 && (
        <p className="text-[color:var(--failed)]">
          {`Not read, and so not on the map: ${reading.unread.map((file) => `${file.name} (${file.why})`).join("; ")}`}
        </p>
      )}
      {reading.shadowed.length > 0 && (
        <p>{`A flow of your own answers to these names instead of the one Sailor ships: ${reading.shadowed.join(", ")}.`}</p>
      )}
    </section>
  );
}

export function FlowMap({ ask, onOpen }: FlowMapProps) {
  const frame = "flex flex-col gap-[var(--space-4)] bg-[var(--surface-0)] p-[var(--space-4)]";
  if (ask.state === "reading") {
    return <div className={frame}><p className={NOTE}>Reading the flow files…</p></div>;
  }
  if (ask.state === "unreadable") {
    return (
      <div className={frame}>
        <p className="text-[length:var(--text-mid)] text-[color:var(--failed)]">The flows could not be read, so this map is empty and not true.</p>
        <p className={`${NOTE} font-mono`}>{ask.why}</p>
      </div>
    );
  }
  const { reading } = ask;
  if (reading.nodes.length === 0 && reading.unread.length === 0) {
    return (
      <div className={frame}>
        <p className="text-[length:var(--text-mid)] text-[color:var(--text-1)]">No flow files here yet.</p>
        <p className={NOTE}>A flow is a file named «something.flow.json». Write one and it appears on this map.</p>
      </div>
    );
  }
  return (
    <div className={frame}>
      <Verdict reading={reading} />
      <Tally reading={reading} />
      <Troubles reading={reading} />
      <Rings reading={reading} />
      <Web reading={reading} onOpen={onOpen} />
      <OnTheirOwn reading={reading} onOpen={onOpen} />
    </div>
  );
}
