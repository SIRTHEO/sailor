import { useEffect, useMemo, useRef, useState } from "react";
import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type Edge,
  type EdgeChange,
  type Node,
  type NodeChange,
  type ReactFlowInstance,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";

import { FlowBandNode, KIND_LABEL, StepNode, type StepNodeData } from "./StepNode";
import { buildUnifiedLayout, nodeId, splitNodeId, wouldCycle } from "./layout";
import { SAMPLE, SAMPLE_RUN } from "./sample";
import { deleteFlow, insideTheWindow, loadFlows, saveFlow } from "./engine";
import {
  DEFAULT_ACTION_FOR_KIND,
  KNOWN_ACTIONS,
  type BrokenFlow,
  type FlowEntry,
  type FlowFile,
  type Step,
  type StepKind,
} from "./flow";

const nodeTypes = { step: StepNode, flowBand: FlowBandNode };
const CASSETTE_KINDS = Object.keys(DEFAULT_ACTION_FOR_KIND) as StepKind[];

/** Da dove vengono i flussi che si stanno guardando. */
type Source = "sample" | "engine" | "failed";

/** Un flusso in modifica: quello che si vede e quello che è già sul disco. */
interface WorkingFlow {
  flow: FlowFile;
  saved: FlowFile;
}

function isDirty(working: WorkingFlow): boolean {
  return JSON.stringify(working.flow) !== JSON.stringify(working.saved);
}

/** Divide gli ingressi in flussi modificabili (mappa per nome) e flussi rotti (lista). */
function splitEntries(entries: FlowEntry[]): { flows: Map<string, WorkingFlow>; broken: BrokenFlow[] } {
  const flows = new Map<string, WorkingFlow>();
  const broken: BrokenFlow[] = [];
  for (const entry of entries) {
    if (entry.state === "loaded") {
      flows.set(entry.flow.id, { flow: entry.flow, saved: entry.flow });
    } else {
      broken.push(entry.broken);
    }
  }
  return { flows, broken };
}

export default function App() {
  // SI PARTE DALL'ESEMPIO, E LO SI DICE. La tela nasce con i dati di esempio
  // perché in un browser (`npm run dev`) il motore non c'è; dentro la finestra
  // vengono sostituiti al primo giro. Quello che non si fa è lasciare credere
  // che l'esempio sia il mondo: la barra dichiara sempre quale dei due si sta
  // guardando.
  const [flows, setFlows] = useState<Map<string, WorkingFlow>>(() => splitEntries(SAMPLE).flows);
  const [broken, setBroken] = useState<BrokenFlow[]>(() => splitEntries(SAMPLE).broken);
  const [source, setSource] = useState<Source>("sample");
  const [failure, setFailure] = useState<string | null>(null);

  // Il fuoco è del ramo, non della tela: la colonna indica un percorso dentro
  // il grafo unico, non sceglie più quale grafo mostrare.
  const [focusName, setFocusName] = useState<string | null>(null);
  const [selectedNode, setSelectedNode] = useState<string | null>(null);

  const [saving, setSaving] = useState<Set<string>>(new Set());
  const [saveErrors, setSaveErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!insideTheWindow()) return;
    let dropped = false;
    loadFlows()
      .then((loaded) => {
        if (dropped) return;
        const split = splitEntries(loaded);
        setFlows(split.flows);
        setBroken(split.broken);
        setSource("engine");
        setFocusName(null);
        setSelectedNode(null);
      })
      .catch((error: unknown) => {
        if (dropped) return;
        // Un motore muto non si maschera con l'esempio: chi guarda vedrebbe
        // flussi che sul disco non ci sono.
        setSource("failed");
        setFailure(String(error));
      });
    return () => {
      dropped = true;
    };
  }, []);

  const anyDirty = useMemo(() => Array.from(flows.values()).some(isDirty), [flows]);

  // Chi chiude la finestra non deve scoprire dopo di aver perso il lavoro.
  // `beforeunload` è un ripiego onesto: nel guscio nativo la chiusura è un
  // evento della finestra, non una navigazione di pagina, e non è detto che
  // questo listener venga interpellato — non l'ho potuto provare su una
  // chiusura vera. La difesa che regge comunque è il bollino «non salvato»,
  // sempre visibile, nella barra e accanto a ogni flusso.
  useEffect(() => {
    function handleBeforeUnload(event: BeforeUnloadEvent) {
      if (!anyDirty) return;
      event.preventDefault();
      event.returnValue = "";
    }
    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => window.removeEventListener("beforeunload", handleBeforeUnload);
  }, [anyDirty]);

  const flowList = useMemo(
    () => Array.from(flows.entries()).map(([name, working]) => ({ name, flow: working.flow })),
    [flows],
  );

  // GLI STATI D'ESEMPIO RESTANO ALL'ESEMPIO. Sui flussi veri la corsa non è
  // ancora letta dal deposito: passare qui `SAMPLE_RUN` colorerebbe di
  // «andato» e «rotto» dei passi che nessuno ha mai eseguito, ed è la specie
  // di bugia che una tela racconta senza che nessuno la smentisca.
  const runs = source === "sample" ? SAMPLE_RUN : new Map();

  const layout = useMemo(() => buildUnifiedLayout(flowList, runs, focusName), [flowList, runs, focusName]);

  const [nodes, setNodes] = useState<Node[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  useEffect(() => {
    setNodes(layout.nodes);
    setEdges(layout.edges);
  }, [layout]);

  // Il fuoco muove la vista, non ricrea la disposizione: leggo l'ultimo
  // riquadro noto da un ref, così un'ingresso non ri-centra la tela mentre si
  // sta modificando un campo altrove.
  const bandsRef = useRef(layout.bands);
  bandsRef.current = layout.bands;
  const flowInstance = useRef<ReactFlowInstance | null>(null);

  useEffect(() => {
    const instance = flowInstance.current;
    if (!instance) return;
    if (focusName === null) {
      instance.fitView({ padding: 0.15, duration: 300 });
      return;
    }
    const band = bandsRef.current.get(focusName);
    if (!band) return;
    void instance.fitBounds(
      { x: band.x, y: band.y, width: band.width, height: band.height },
      { padding: 0.2, duration: 300 },
    );
  }, [focusName]);

  useEffect(() => {
    flowInstance.current?.fitView({ padding: 0.15 });
  }, [source]);

  function updateFlow(name: string, updater: (flow: FlowFile) => FlowFile) {
    setFlows((prev) => {
      const current = prev.get(name);
      if (!current) return prev;
      const next = new Map(prev);
      next.set(name, { ...current, flow: updater(current.flow) });
      return next;
    });
  }

  function addStep(flowName: string, kind: StepKind) {
    const action = DEFAULT_ACTION_FOR_KIND[kind];
    if (!action) return;
    const working = flows.get(flowName);
    if (!working) return;
    const ids = new Set(working.flow.graph.steps.map((step) => step.id));
    let n = 1;
    let id = `${kind}-${n}`;
    while (ids.has(id)) {
      n += 1;
      id = `${kind}-${n}`;
    }
    const step: Step = {
      id,
      deps: [],
      input_schema: { type: "any" },
      output_schema: { type: "any" },
      with: null,
      when: null,
      action,
      max_attempts: 1,
    };
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: { ...flow.graph, steps: [...flow.graph.steps, step] },
    }));
    setSelectedNode(nodeId(flowName, id));
  }

  function deleteStep(flowName: string, stepId: string) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps
          .filter((step) => step.id !== stepId)
          .map((step) =>
            step.deps.includes(stepId) ? { ...step, deps: step.deps.filter((dep) => dep !== stepId) } : step,
          ),
        skippable_dependencies: (flow.graph.skippable_dependencies ?? []).filter(
          (edge) => edge.step !== stepId && edge.dependency !== stepId,
        ),
      },
    }));
    setSelectedNode((current) => (current === nodeId(flowName, stepId) ? null : current));
  }

  function renameStep(flowName: string, oldId: string, newId: string) {
    updateFlow(flowName, (flow) => {
      if (flow.graph.steps.some((step) => step.id === newId)) return flow;
      return {
        ...flow,
        graph: {
          steps: flow.graph.steps.map((step) => {
            const deps = step.deps.includes(oldId) ? step.deps.map((dep) => (dep === oldId ? newId : dep)) : step.deps;
            if (step.id === oldId) return { ...step, id: newId, deps };
            return deps === step.deps ? step : { ...step, deps };
          }),
          skippable_dependencies: (flow.graph.skippable_dependencies ?? []).map((edge) => ({
            step: edge.step === oldId ? newId : edge.step,
            dependency: edge.dependency === oldId ? newId : edge.dependency,
          })),
        },
      };
    });
    setSelectedNode(nodeId(flowName, newId));
  }

  function updateStepField(flowName: string, stepId: string, patch: Partial<Step>) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) => (step.id === stepId ? { ...step, ...patch } : step)),
      },
    }));
  }

  // Collegare e scollegare due passi: lo stesso gesto nasca da un trascino
  // sulla tela o da una casella nel pannello, passa sempre di qui.
  function connectSteps(flowName: string, from: string, to: string) {
    if (from === to) return;
    const working = flows.get(flowName);
    if (!working) return;
    if (wouldCycle(working.flow.graph, from, to)) return;
    const target = working.flow.graph.steps.find((step) => step.id === to);
    if (!target || target.deps.includes(from)) return;
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) => (step.id === to ? { ...step, deps: [...step.deps, from] } : step)),
      },
    }));
  }

  function disconnectSteps(flowName: string, from: string, to: string) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) =>
          step.id === to ? { ...step, deps: step.deps.filter((dep) => dep !== from) } : step,
        ),
      },
    }));
  }

  async function handleSave(name: string) {
    const working = flows.get(name);
    if (!working) return;
    setSaving((prev) => new Set(prev).add(name));
    setSaveErrors((prev) => {
      if (!(name in prev)) return prev;
      const next = { ...prev };
      delete next[name];
      return next;
    });
    try {
      await saveFlow(working.flow);
      setFlows((prev) => {
        const current = prev.get(name);
        if (!current) return prev;
        const next = new Map(prev);
        next.set(name, { flow: current.flow, saved: current.flow });
        return next;
      });
    } catch (error) {
      setSaveErrors((prev) => ({ ...prev, [name]: String(error) }));
    } finally {
      setSaving((prev) => {
        const next = new Set(prev);
        next.delete(name);
        return next;
      });
    }
  }

  async function handleDeleteFlow(name: string) {
    if (!window.confirm(`Cancellare il flusso "${name}"? Non si torna indietro da qui.`)) return;
    setSaving((prev) => new Set(prev).add(name));
    try {
      await deleteFlow(name);
      setFlows((prev) => {
        const next = new Map(prev);
        next.delete(name);
        return next;
      });
      setFocusName((current) => (current === name ? null : current));
      setSelectedNode((current) => (current && splitNodeId(current).flowName === name ? null : current));
    } catch (error) {
      setSaveErrors((prev) => ({ ...prev, [name]: String(error) }));
    } finally {
      setSaving((prev) => {
        const next = new Set(prev);
        next.delete(name);
        return next;
      });
    }
  }

  function onNodesChange(changes: NodeChange[]) {
    setNodes((current) => applyNodeChanges(changes, current));
  }

  function onEdgesChange(changes: EdgeChange[]) {
    setEdges((current) => applyEdgeChanges(changes, current));
  }

  function onConnect(connection: Connection) {
    if (!connection.source || !connection.target) return;
    const from = splitNodeId(connection.source);
    const to = splitNodeId(connection.target);
    if (from.flowName !== to.flowName) return;
    connectSteps(from.flowName, from.stepId, to.stepId);
  }

  function isValidConnection(connection: Connection | Edge): boolean {
    const { source, target } = connection;
    if (!source || !target) return false;
    const from = splitNodeId(source);
    const to = splitNodeId(target);
    return from.flowName === to.flowName && from.stepId !== to.stepId;
  }

  function onNodesDelete(deleted: Node[]) {
    for (const node of deleted) {
      if (node.type !== "step") continue;
      const { flowName, stepId } = splitNodeId(node.id);
      deleteStep(flowName, stepId);
    }
  }

  function onEdgesDelete(deleted: Edge[]) {
    for (const edge of deleted) {
      const from = splitNodeId(edge.source);
      const to = splitNodeId(edge.target);
      if (from.flowName !== to.flowName) continue;
      disconnectSteps(to.flowName, from.stepId, to.stepId);
    }
  }

  function onNodeClick(_event: unknown, node: Node) {
    if (node.type !== "step") return;
    setSelectedNode(node.id);
    setFocusName(splitNodeId(node.id).flowName);
  }

  const selected = selectedNode ? nodes.find((node) => node.id === selectedNode) : undefined;
  const selectedData = selected?.data as StepNodeData | undefined;
  const selectedFlow = selectedData ? flows.get(selectedData.flowName) : undefined;

  const focusedBand = focusName ? layout.bands.get(focusName) : undefined;
  const focusedWorking = focusName ? flows.get(focusName) : undefined;
  const focusedDirty = focusedWorking ? isDirty(focusedWorking) : false;

  return (
    <div className="app">
      <header className="bar">
        <span className="bar__name">Sailor</span>
        <span className="bar__sep" />
        <span className="bar__flow">{flows.size} flussi, un sistema solo</span>
        {/* Chi guarda deve sapere se sta vedendo il disco o un esempio, senza
            chiedere e senza aprire il codice. */}
        <span className="bar__source" data-source={source}>
          {source === "engine"
            ? `${flows.size + broken.length} flussi dal disco`
            : source === "failed"
              ? `il motore non risponde: ${failure}`
              : "dati di esempio"}
        </span>
        {anyDirty && <span className="bar__dirty">modifiche non salvate</span>}
        <div className="bar__spacer" />
        <button type="button" className="is-primary">
          Esegui
        </button>
      </header>

      {focusName && focusedWorking && focusedBand && (
        <div className="focusbar">
          <span className="focusbar__dot" style={{ background: focusedBand.color }} />
          <span className="focusbar__name">{focusName}</span>
          <span className="focusbar__desc">{focusedWorking.flow.description}</span>
          {focusedDirty && <span className="focusbar__dirty">non salvato</span>}
          <div className="focusbar__spacer" />
          {saveErrors[focusName] && <span className="focusbar__error">{saveErrors[focusName]}</span>}
          <button
            type="button"
            onClick={() => void handleSave(focusName)}
            disabled={!focusedDirty || saving.has(focusName)}
          >
            {saving.has(focusName) ? "salvo…" : "Salva flusso"}
          </button>
          <button
            type="button"
            className="is-danger"
            onClick={() => void handleDeleteFlow(focusName)}
            disabled={saving.has(focusName)}
          >
            Elimina flusso
          </button>
        </div>
      )}

      <div className="body">
        <aside className="rail">
          <div className="rail__title">Flussi registrati</div>
          <button
            type="button"
            className="rail__all"
            data-active={focusName === null || undefined}
            onClick={() => setFocusName(null)}
          >
            Tutti i flussi
          </button>
          {flowList.map(({ name, flow }) => {
            const working = flows.get(name);
            const dirty = working ? isDirty(working) : false;
            const color = layout.bands.get(name)?.color;
            return (
              <button
                type="button"
                key={name}
                className="rail__item"
                data-open={name === focusName || undefined}
                onClick={() => {
                  setFocusName((current) => (current === name ? null : name));
                  setSelectedNode(null);
                }}
              >
                <span className="rail__dot" style={{ background: color }} />
                <span className="rail__label">
                  {name}
                  {dirty && <span className="rail__dirty-dot" title="non salvato" />}
                </span>
                <span className="rail__note">{flow.graph.steps.length} passi</span>
              </button>
            );
          })}
          {broken.map((entry) => (
            // Un flusso rotto non sparisce dall'elenco: si vede, marcato, col
            // motivo. Non entra nella tela perché non ha un grafo da disegnare.
            <div className="rail__item" key={entry.name} data-broken>
              <span className="rail__label">{entry.name}</span>
              <span className="rail__note">{entry.reason}</span>
            </div>
          ))}

          <div className="rail__cassette">
            <div className="rail__title">Cassetta dei passi</div>
            {focusName === null && <p className="rail__hint">scegli un flusso per aggiungere un passo</p>}
            <div className="cassette__grid">
              {CASSETTE_KINDS.map((kind) => (
                <button
                  key={kind}
                  type="button"
                  className="cassette__item"
                  disabled={focusName === null}
                  title={focusName ? `aggiungi un passo di tipo «${KIND_LABEL[kind]}»` : "scegli prima un flusso"}
                  onClick={() => focusName && addStep(focusName, kind)}
                >
                  {KIND_LABEL[kind]}
                </button>
              ))}
            </div>
          </div>
        </aside>

        <main className="canvas">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            isValidConnection={isValidConnection}
            onNodesDelete={onNodesDelete}
            onEdgesDelete={onEdgesDelete}
            deleteKeyCode={["Backspace", "Delete"]}
            onNodeClick={onNodeClick}
            onPaneClick={() => setSelectedNode(null)}
            onInit={(instance) => {
              flowInstance.current = instance;
            }}
            fitView
            proOptions={{ hideAttribution: false }}
          >
            <Background gap={20} />
            <Controls />
            <MiniMap pannable zoomable />
          </ReactFlow>
        </main>

        <aside className="panel">
          <datalist id="known-actions">
            {KNOWN_ACTIONS.map((action) => (
              <option key={action} value={action} />
            ))}
          </datalist>

          {selectedData && selectedFlow ? (
            <StepEditor
              key={selectedNode}
              flowName={selectedData.flowName}
              color={selectedData.color}
              step={selectedData.step}
              siblingIds={selectedFlow.flow.graph.steps.map((step) => step.id).filter((id) => id !== selectedData.step.id)}
              onRename={(newId) => renameStep(selectedData.flowName, selectedData.step.id, newId)}
              onField={(patch) => updateStepField(selectedData.flowName, selectedData.step.id, patch)}
              onToggleDep={(depId, on) =>
                on
                  ? connectSteps(selectedData.flowName, depId, selectedData.step.id)
                  : disconnectSteps(selectedData.flowName, depId, selectedData.step.id)
              }
              onDelete={() => deleteStep(selectedData.flowName, selectedData.step.id)}
            />
          ) : (
            <div className="panel__empty">
              Scegli un passo per vederne e modificarne i parametri, o un flusso a
              sinistra per metterlo a fuoco.
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}

interface StepEditorProps {
  flowName: string;
  color: string;
  step: Step;
  siblingIds: string[];
  onRename: (newId: string) => void;
  onField: (patch: Partial<Step>) => void;
  onToggleDep: (depId: string, on: boolean) => void;
  onDelete: () => void;
}

/**
 * Il pannello di un passo scelto: id, azione, tentativi, dipendenze, parametri.
 * Monta una volta per passo selezionato (la chiave è `selectedNode` in
 * `App`), così le bozze locali (id, JSON) ripartono pulite a ogni cambio di
 * selezione senza un effetto dedicato a resettarle.
 */
function StepEditor({ flowName, color, step, siblingIds, onRename, onField, onToggleDep, onDelete }: StepEditorProps) {
  const [idDraft, setIdDraft] = useState(step.id);
  const [withDraft, setWithDraft] = useState(step.with ? JSON.stringify(step.with, null, 2) : "");
  const [withError, setWithError] = useState<string | null>(null);
  const idTaken = idDraft !== step.id && siblingIds.includes(idDraft);

  function commitId() {
    const trimmed = idDraft.trim();
    if (!trimmed || trimmed === step.id || siblingIds.includes(trimmed)) {
      setIdDraft(step.id);
      return;
    }
    onRename(trimmed);
  }

  function commitWith() {
    const text = withDraft.trim();
    if (text === "") {
      setWithError(null);
      onField({ with: null });
      return;
    }
    try {
      const parsed = JSON.parse(text) as Record<string, unknown>;
      setWithError(null);
      onField({ with: parsed });
    } catch (error) {
      setWithError(String(error));
    }
  }

  return (
    <>
      <div className="panel__flow" style={{ color }}>
        {flowName}
      </div>
      <div className="panel__title">Passo</div>
      <input
        className="panel__id-input"
        value={idDraft}
        onChange={(event) => setIdDraft(event.target.value)}
        onBlur={commitId}
      />
      {idTaken && <p className="panel__error">un altro passo del flusso si chiama già così</p>}

      <label className="panel__field">
        <span>Azione</span>
        <input list="known-actions" value={step.action} onChange={(event) => onField({ action: event.target.value })} />
      </label>

      <label className="panel__field">
        <span>Tetto tentativi</span>
        <input
          type="number"
          min={1}
          value={step.max_attempts}
          onChange={(event) => onField({ max_attempts: Math.max(1, Number(event.target.value) || 1) })}
        />
      </label>

      <div className="panel__field">
        <span>Dipende da</span>
        {siblingIds.length === 0 ? (
          <p className="panel__empty">nessun altro passo in questo flusso</p>
        ) : (
          <div className="panel__deps">
            {siblingIds.map((id) => (
              <label key={id} className="panel__dep">
                <input
                  type="checkbox"
                  checked={step.deps.includes(id)}
                  onChange={(event) => onToggleDep(id, event.target.checked)}
                />
                {id}
              </label>
            ))}
          </div>
        )}
      </div>

      <label className="panel__field">
        <span>Parametri (JSON)</span>
        <textarea
          className="panel__with"
          rows={5}
          value={withDraft}
          onChange={(event) => setWithDraft(event.target.value)}
          onBlur={commitWith}
          placeholder="nessuno"
        />
      </label>
      {withError && <p className="panel__error">JSON non valido: {withError}</p>}

      <button type="button" className="panel__delete" onClick={onDelete}>
        Elimina passo
      </button>
    </>
  );
}
