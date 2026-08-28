import { useMemo, useState } from "react";
import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  type Node,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";

import { StepNode, type StepNodeData } from "./StepNode";
import { toEdges, toNodes } from "./layout";
import { SAMPLE, SAMPLE_RUN } from "./sample";
import type { FlowEntry } from "./flow";

const nodeTypes = { step: StepNode };

function flowName(entry: FlowEntry): string {
  return entry.state === "loaded" ? entry.flow.id : entry.broken.name;
}

export default function App() {
  const [openName, setOpenName] = useState(flowName(SAMPLE[0]));
  const [selected, setSelected] = useState<string | null>(null);

  const open = SAMPLE.find((entry) => flowName(entry) === openName);

  const { nodes, edges } = useMemo(() => {
    if (!open || open.state !== "loaded") return { nodes: [], edges: [] };
    return {
      nodes: toNodes(open.flow.graph, SAMPLE_RUN),
      edges: toEdges(open.flow.graph),
    };
  }, [open]);

  const chosen = nodes.find((node) => node.id === selected);
  const chosenData = chosen?.data as StepNodeData | undefined;

  return (
    <div className="app">
      <header className="bar">
        <span className="bar__name">Sailor</span>
        <span className="bar__sep" />
        <span className="bar__flow">{openName}</span>
        <div className="bar__spacer" />
        <button type="button" className="is-primary">
          Esegui
        </button>
      </header>

      <div className="body">
        <aside className="rail">
          <div className="rail__title">Flussi registrati</div>
          {SAMPLE.map((entry) => {
            const name = flowName(entry);
            const broken = entry.state === "broken";
            return (
              <button
                type="button"
                key={name}
                className="rail__item"
                data-open={name === openName || undefined}
                data-broken={broken || undefined}
                onClick={() => {
                  setOpenName(name);
                  setSelected(null);
                }}
              >
                <span className="rail__label">{name}</span>
                {/* Un flusso che non si carica resta nell'elenco: nasconderlo
                    farebbe vedere un elenco corto senza dire che è corto. */}
                <span className="rail__note">
                  {broken
                    ? entry.broken.reason
                    : `${entry.flow.graph.steps.length} passi`}
                </span>
              </button>
            );
          })}
        </aside>

        <main className="canvas">
          {open?.state === "broken" ? (
            <div className="broken">
              <h2>Questo flusso non si carica</h2>
              <p>{open.broken.reason}</p>
              <p className="broken__why">
                Sailor non lo esegue e non lo nasconde: finché il file non è
                valido, il flusso esiste solo come segnalazione.
              </p>
            </div>
          ) : (
            <ReactFlow
              nodes={nodes}
              edges={edges}
              nodeTypes={nodeTypes}
              onNodeClick={(_event, node: Node) => setSelected(node.id)}
              onPaneClick={() => setSelected(null)}
              fitView
              proOptions={{ hideAttribution: false }}
            >
              <Background gap={20} />
              <Controls />
              <MiniMap pannable zoomable />
            </ReactFlow>
          )}
        </main>

        <aside className="panel">
          {chosenData ? (
            <>
              <div className="panel__title">Passo scelto</div>
              <div className="panel__id">{chosenData.step.id}</div>
              <dl>
                <dt>Azione</dt>
                <dd>{chosenData.step.action}</dd>
                <dt>Tetto tentativi</dt>
                <dd>{chosenData.step.max_attempts}</dd>
                <dt>Dipende da</dt>
                <dd>{chosenData.step.deps.join(", ") || "nessuno"}</dd>
                <dt>Parametri</dt>
                <dd>
                  {chosenData.step.with
                    ? JSON.stringify(chosenData.step.with)
                    : "nessuno"}
                </dd>
              </dl>
            </>
          ) : (
            <div className="panel__empty">
              Scegli un passo per vederne i parametri.
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}
