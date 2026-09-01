// The inspector for the selected step: what it is called, what it does, who
// runs it, how many attempts it gets, what it depends on, and with which
// parameters.
//
// THE TOOL PICKER KNOWS NO TOOL. The list comes from the engine
// (`discover_tools`); this file only knows how to draw it. When discovery is
// mute the panel says so and lets the identifier be typed by hand, so nobody
// faces an empty list without knowing whether the machine is bare or the
// engine is silent.

import { useRef, useState } from "react";
import { kindOf, type Condition, type Step, type ValueSchema } from "./flow";
import {
  chainIn,
  groupByKind,
  joinToolParams,
  modelSuggestions,
  optionsPreview,
  rivalBinary,
  schemaRejectsToolKeys,
  splitToolParams,
  toolKindLabel,
  type OptionSpec,
  type OptionValue,
  type Tool,
  type ToolChoice,
  type ToolDiscovery,
} from "./tools";
import { ToolMark } from "./ToolMark";

export interface StepEditorProps {
  flowName: string;
  color: string;
  step: Step;
  siblingIds: string[];
  tools: Tool[];
  discovery: ToolDiscovery;
  /** Models already written in the other steps: suggestions taken from the real thing. */
  usedModels: string[];
  onRename: (newId: string) => void;
  onField: (patch: Partial<Step>) => void;
  onToggleDep: (depId: string, on: boolean) => void;
  onDelete: () => void;
}

/** The families a step can belong to, as a word a reader knows. */
const KIND_LABEL: Record<string, string> = {
  trigger: "trigger",
  engine: "engine",
  check: "check",
  wait: "wait",
  branch: "branch",
  deposit: "store",
  gesture: "gesture",
  human: "person",
  subflow: "subflow",
};

/**
 * Mounts once per selected step (the key is `selectedNode` in `App`), so the
 * local drafts (id, JSON) start clean on every change of selection without a
 * dedicated effect to reset them.
 */
export function StepEditor({
  flowName,
  color,
  step,
  siblingIds,
  tools,
  discovery,
  usedModels,
  onRename,
  onField,
  onToggleDep,
  onDelete,
}: StepEditorProps) {
  const [idDraft, setIdDraft] = useState(step.id);
  const idTaken = idDraft !== step.id && siblingIds.includes(idDraft);

  const { choice, rest } = splitToolParams(step.with);
  // The chain the panel cannot compose and must not delete: it stays among the
  // other parameters, and is read from there to be told to whoever is looking.
  const chain = chainIn(rest);
  // An `engine` step uses a tool by definition; and a step that already names
  // one shows it whatever action it carries — hiding it would hide a datum that
  // is in the file. A chain counts: a step naming three engines uses one.
  const usesTool = kindOf(step.action) === "engine" || choice.tool !== "" || chain.length > 0;

  // The JSON box shows only the parameters the panel does not handle: the
  // dedicated fields already have theirs, and seeing them in two places that
  // overwrite each other is the fastest way to lose the one written last.
  const [withDraft, setWithDraft] = useState(() => {
    const shown = usesTool ? rest : (step.with ?? {});
    return Object.keys(shown).length === 0 ? "" : JSON.stringify(shown, null, 2);
  });
  const [withError, setWithError] = useState<string | null>(null);

  const chosen = tools.find((tool) => tool.id === choice.tool);
  const unknownChoice = choice.tool !== "" && chosen === undefined;
  const models = modelSuggestions(chosen, usedModels);
  const rival = rivalBinary(rest);
  const rejectedBySchema = schemaRejectsToolKeys(step.input_schema, choice);
  const kind = kindOf(step.action);

  function commitId() {
    const trimmed = idDraft.trim();
    if (!trimmed || trimmed === step.id || siblingIds.includes(trimmed)) {
      setIdDraft(step.id);
      return;
    }
    onRename(trimmed);
  }

  function setChoice(patch: Partial<ToolChoice>) {
    onField({ with: joinToolParams(rest, { ...choice, ...patch }) });
  }

  function commitWith() {
    const text = withDraft.trim();
    if (text === "") {
      setWithError(null);
      onField({ with: usesTool ? joinToolParams({}, choice) : null });
      return;
    }
    try {
      const parsed = JSON.parse(text) as Record<string, unknown>;
      setWithError(null);
      onField({ with: usesTool ? joinToolParams(parsed, choice) : parsed });
    } catch (error) {
      setWithError(String(error));
    }
  }

  return (
    <div className="inspector">
      <header className="inspector__head">
        <div className="inspector__flow" style={{ color }}>
          {flowName}
        </div>
        <div className="inspector__eyebrow">Selected step</div>
        {/* A STEP HAS NO NAME. `flow::Step` carries an id and nothing else to
            read it by, so the heading is that id spelled out — the same datum,
            not a second one — and the field below is where it is edited. */}
        <h2 className="inspector__title">{spellOut(step.id)}</h2>
        <input
          className="inspector__id"
          aria-label="Step id"
          value={idDraft}
          onChange={(event) => setIdDraft(event.target.value)}
          onBlur={commitId}
        />
        {idTaken && <p className="inspector__error">another step of this flow is already called that</p>}
      </header>

      <div className="inspector__body">
        <section className="inspector__block">
          <div className="inspector__label">
            Action
            <span className="inspector__kind">{KIND_LABEL[kind] ?? kind}</span>
          </div>
          <input
            className="inspector__input inspector__input--data"
            list="known-actions"
            aria-label="Action"
            value={step.action}
            onChange={(event) => onField({ action: event.target.value })}
          />
        </section>

        <Species action={step.action} />

        {usesTool && (
          <section className="inspector__block">
            <div className="inspector__label">Engine</div>

            {/* A CHAIN IS SAID HERE TOO. A picker reading "— none —" above a
                step that names three engines is the same lie the node used to
                tell, moved one window over. */}
            {chain.length > 0 && (
              <p className="inspector__note">
                this step declares a chain of engines — {chain.join(" › ")} — in order of
                preference. The panel cannot compose it yet: it stays as it is, and can be read
                among the parameters below. Choosing a tool here replaces it.
              </p>
            )}

            {tools.length > 0 ? (
              // The mark sits beside the choice, not inside: an `<option>`
              // admits no drawing, and no trick that puts one there survives a
              // long list or someone navigating by keyboard.
              <div className="inspector__pick">
                {choice.tool !== "" && (
                  <ToolMark id={choice.tool} size={20} off={chosen ? !chosen.available : true} />
                )}
                <select
                  className="inspector__select"
                  aria-label="Engine"
                  value={choice.tool}
                  onChange={(event) => setChoice({ tool: event.target.value })}
                >
                  <option value="">— none —</option>
                  {groupByKind(tools).map((group) => (
                    <optgroup key={group.kind} label={toolKindLabel(group.kind)}>
                      {group.tools.map((tool) => (
                        <option key={tool.id} value={tool.id}>
                          {tool.name}
                          {tool.available ? "" : " (unavailable)"}
                        </option>
                      ))}
                    </optgroup>
                  ))}
                  {/* A tool chosen elsewhere and missing here is not deleted in
                      secret: it stays chosen, and one can see it is not here. */}
                  {unknownChoice && (
                    <option value={choice.tool}>{choice.tool} — not detected on this machine</option>
                  )}
                </select>
              </div>
            ) : (
              <>
                <p className="inspector__note">
                  {discovery.state === "asking"
                    ? "asking the engine which tools are here…"
                    : "no tool detected"}
                </p>
                {discovery.state === "mute" && <p className="inspector__why">{discovery.why}</p>}
                {/* Without a list the identifier is typed by hand: the node
                    stays composable, and discovery will find it again. */}
                <input
                  className="inspector__input inspector__input--data"
                  aria-label="Engine identifier"
                  value={choice.tool}
                  placeholder="the identifier the engine will declare"
                  onChange={(event) => setChoice({ tool: event.target.value })}
                />
              </>
            )}

            <datalist id="known-models">
              {models.map((model) => (
                <option key={model} value={model} />
              ))}
            </datalist>
            {/* A closed list here would be a lie: no descriptor declares its
                models, and writing ten from memory would look like the machine
                had found them. The suggestions are only the models that really
                exist — the ones written in the other steps. */}
            <input
              className="inspector__input inspector__input--data"
              list="known-models"
              aria-label="Model"
              value={choice.model}
              placeholder="the model name, as the tool writes it"
              onChange={(event) => setChoice({ model: event.target.value })}
            />
            {models.length === 0 && chosen && (
              <p className="inspector__why">
                no model to suggest: the descriptor of «{chosen.id}» declares none, and inventing
                some would be worse than leaving the field free.
              </p>
            )}

            {chosen && (
              <div className="inspector__detail" data-off={chosen.available ? undefined : true}>
                <p>
                  <span className="inspector__detail-kind">{toolKindLabel(chosen.kind)}</span>
                  {chosen.version !== "" && <span> · {chosen.version}</span>}
                </p>
                {chosen.path !== "" && <p className="inspector__detail-path">{chosen.path}</p>}
                {/* THE REASON IS ALWAYS SHOWN, not only when the tool is
                    missing: when it is there it says from where — the only way
                    to notice one is about to use the wrong binary of two. */}
                {chosen.reason !== "" && (
                  <p>
                    {chosen.available ? "" : "unavailable: "}
                    {chosen.reason}
                  </p>
                )}
                {chosen.descriptor !== "" && (
                  <p>recognised by the descriptor «{chosen.descriptor}»</p>
                )}
              </div>
            )}
            {unknownChoice && tools.length > 0 && (
              <p className="inspector__detail" data-off>
                this tool is not among the ones detected here: the flow stays valid, but on this
                machine it would not start.
              </p>
            )}
            {/* Two answers to the same question: whoever runs it would not know
                which to believe, and the panel cannot choose for the writer. */}
            {rival !== "" && choice.tool !== "" && (
              <p className="inspector__warn">
                this step also declares a binary of its own («bin»: {rival}): two truths about who
                runs it.
              </p>
            )}
            {rejectedBySchema && (
              <p className="inspector__warn">
                the input schema of this step admits no extra fields: with these the engine would
                refuse the flow at load time.
              </p>
            )}

            <ToolOptions
              tool={chosen}
              options={choice.options}
              onChange={(options) => setChoice({ options })}
            />

            <div className="inspector__label">Prompt</div>
            <textarea
              className="inspector__textarea"
              rows={6}
              aria-label="Prompt"
              value={choice.prompt}
              placeholder="what it has to do, said in full"
              onChange={(event) => setChoice({ prompt: event.target.value })}
            />
          </section>
        )}

        <section className="inspector__pair">
          <div>
            <div className="inspector__label">Max attempts</div>
            <input
              className="inspector__input inspector__input--data"
              type="number"
              min={1}
              aria-label="Max attempts"
              value={step.max_attempts}
              onChange={(event) => onField({ max_attempts: Math.max(1, Number(event.target.value) || 1) })}
            />
          </div>
          <div>
            {/* Shown and not editable: composing a condition is a control this
                panel does not have, and leaving `when` out would make a
                conditional step look unconditional. */}
            <div className="inspector__label">Runs when</div>
            <div className="inspector__readonly" title={whenTitle(step.when)}>
              {whenSummary(step.when)}
            </div>
          </div>
        </section>

        <section className="inspector__block">
          <div className="inspector__label">Input and output</div>
          <div className="inspector__io">
            {[
              ...schemaLines(step.input_schema, "in"),
              ...schemaLines(step.output_schema, "out"),
            ].map((line, index) => (
              <div key={index} className="inspector__io-line">
                <span className="inspector__io-side">{line.side}</span>
                <span>{line.name === "" ? "" : `${line.name}:`}</span>
                <span className="inspector__io-type">{line.type}</span>
              </div>
            ))}
          </div>
        </section>

        <section className="inspector__block">
          <div className="inspector__label">Depends on</div>
          {siblingIds.length === 0 ? (
            <p className="inspector__why">no other step in this flow</p>
          ) : (
            <div className="inspector__deps">
              {siblingIds.map((id) => (
                <label key={id} className="inspector__dep">
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
        </section>

        <section className="inspector__block">
          <div className="inspector__label">{usesTool ? "Other parameters (JSON)" : "Parameters (JSON)"}</div>
          <textarea
            className="inspector__textarea inspector__textarea--data"
            rows={5}
            aria-label="Parameters (JSON)"
            value={withDraft}
            onChange={(event) => setWithDraft(event.target.value)}
            onBlur={commitWith}
            placeholder="none"
          />
          {withError && <p className="inspector__error">invalid JSON: {withError}</p>}
        </section>
      </div>

      <footer className="inspector__foot">
        <button type="button" className="inspector__delete" onClick={onDelete}>
          Delete step
        </button>
      </footer>
    </div>
  );
}

// ── the species ──────────────────────────────────────────────────────────

/**
 * What happens when a step falls halfway.
 *
 * THE SPECIES IS NOT IN THE FILE. `flow::StepSpecies` exists and the executor
 * branches on it, but it is declared by the action in Rust (`Action::species`)
 * and never by the graph: there is no field here to write, and no command that
 * tells the window which species an action declares. So this says what is true
 * instead of offering a choice that would go nowhere.
 */
function Species({ action }: { action: string }) {
  return (
    <section className="inspector__block">
      <div className="inspector__label">
        Species
        <span className="inspector__kind">from the action</span>
      </div>
      <p className="inspector__readonly inspector__readonly--wrap">«{action}» declares it in the engine</p>
      <p className="inspector__why">
        Tells Sailor what to do if the step falls halfway and its effect stays unknown. The flow
        file cannot set it, and the window cannot yet ask which one was declared.
      </p>
    </section>
  );
}

// ── reading a step out loud ──────────────────────────────────────────────

/** The id as a heading: the same datum, spaced and capitalised. */
function spellOut(id: string): string {
  const words = id.replace(/[-_.]+/g, " ").trim();
  return words === "" ? id : words.charAt(0).toUpperCase() + words.slice(1);
}

function typeName(schema: ValueSchema): string {
  switch (schema.type) {
    case "array":
      return `${typeName(schema.items)}[]`;
    case "one_of":
      return schema.values.map((value) => JSON.stringify(value)).join(" | ");
    default:
      return schema.type;
  }
}

interface IoLine {
  side: string;
  name: string;
  type: string;
}

/**
 * One line per field of an object schema, one line for anything else. The side
 * word is written once, so a long list reads as one block and not as a column
 * of repeated `in`.
 */
function schemaLines(schema: ValueSchema, side: string): IoLine[] {
  if (schema.type !== "object") return [{ side, name: "", type: typeName(schema) }];
  const names = Object.keys(schema.properties);
  if (names.length === 0) return [{ side, name: "", type: typeName(schema) }];
  return names.map((name, index) => ({
    side: index === 0 ? side : "",
    name: schema.required.includes(name) ? name : `${name}?`,
    type: typeName(schema.properties[name]!),
  }));
}

function whenSummary(when: Condition | null): string {
  if (when === null) return "always";
  switch (when.kind) {
    case "equals":
      return `input = ${short(when.value)}`;
    case "pointer_equals":
      return `${when.pointer} = ${short(when.value)}`;
    case "pointer_exists":
      return `${when.pointer} exists`;
  }
}

/** The condition in full, for the pointer that does not fit the box. */
function whenTitle(when: Condition | null): string {
  return when === null ? "this step always runs" : JSON.stringify(when);
}

function short(value: unknown): string {
  const text = JSON.stringify(value) ?? "null";
  return text.length > 24 ? `${text.slice(0, 23)}…` : text;
}

// ── the options ──────────────────────────────────────────────────────────

interface ToolOptionsProps {
  /** The chosen tool, if it is among the detected ones. */
  tool: Tool | undefined;
  options: Record<string, OptionValue>;
  onChange: (options: Record<string, OptionValue>) => void;
}

/** A row being composed. The `id` never leaves this file: it keeps React from
 *  remounting the row while its name is typed — without it the field loses
 *  focus at every keystroke. */
interface OptionRow {
  id: number;
  name: string;
  value: OptionValue;
}

/**
 * The options of a step: a choice, not a command line to be typed.
 *
 * TWO WAYS, AND THE SECOND IS TODAY'S. If the tool declares its own options
 * (`options` in the descriptor) each becomes a control with its own shape. No
 * descriptor declares them yet, so pairs are written by hand — and the panel
 * SAYS it is a fallback instead of implying that those few rows are everything
 * the tool accepts. What it never does is guess.
 */
function ToolOptions({ tool, options, onChange }: ToolOptionsProps) {
  const declared: OptionSpec[] = tool?.options ?? [];
  const nextId = useRef(0);
  // Mounts once per selected step — `StepEditor` has `key={selectedNode}` in
  // `App` — so the draft starts clean on every change of selection.
  const [rows, setRows] = useState<OptionRow[]>(() =>
    Object.entries(options)
      .filter(([name]) => !declared.some((spec) => spec.key === name))
      .map(([name, value]) => ({ id: nextId.current++, name, value })),
  );

  /** The free rows plus the declared ones: what ends up in the step. */
  function push(freeRows: OptionRow[], declaredValues: Record<string, OptionValue>) {
    const next: Record<string, OptionValue> = { ...declaredValues };
    for (const row of freeRows) {
      if (row.name.trim() !== "") next[row.name] = row.value;
    }
    onChange(next);
  }

  const declaredValues: Record<string, OptionValue> = {};
  for (const spec of declared) {
    if (spec.key in options) declaredValues[spec.key] = options[spec.key]!;
  }

  function editRows(update: (rows: OptionRow[]) => OptionRow[]) {
    setRows((current) => {
      const next = update(current);
      push(next, declaredValues);
      return next;
    });
  }

  function setDeclared(key: string, value: OptionValue | undefined) {
    const values = { ...declaredValues };
    if (value === undefined) delete values[key];
    else values[key] = value;
    push(rows, values);
  }

  // Two rows with the same name cannot live in one JSON object: only one
  // survives on disk. Better said while typing than found on reopening.
  const named = rows.map((row) => row.name.trim()).filter((name) => name !== "");
  const duplicated = named.some((name, index) => named.indexOf(name) !== index);

  const preview = optionsPreview({
    tool: "",
    model: "",
    prompt: "",
    options: (() => {
      const shown: Record<string, OptionValue> = { ...declaredValues };
      for (const row of rows) if (row.name.trim() !== "") shown[row.name] = row.value;
      return shown;
    })(),
  });

  return (
    <div className="inspector__options">
      <div className="inspector__label">Options</div>

      {declared.length > 0 ? (
        declared.map((spec) => (
          <DeclaredOption
            key={spec.key}
            spec={spec}
            value={declaredValues[spec.key]}
            onSet={(value) => setDeclared(spec.key, value)}
            onClear={() => setDeclared(spec.key, undefined)}
          />
        ))
      ) : (
        <p className="inspector__why">
          {tool
            ? `the descriptor of «${tool.id}» does not declare which options it accepts: they are written by hand here, and become a guided choice as soon as the descriptor carries them.`
            : "choose a tool to know which options it accepts."}
        </p>
      )}

      {/* Hand-written options survive even when the descriptor declares others:
          one it does not know is not necessarily wrong, and it is the runner
          who finds that out, not this panel. */}
      {rows.map((row) => (
        <FreeOption
          key={row.id}
          name={row.name}
          value={row.value}
          onRename={(to) =>
            editRows((current) => current.map((item) => (item.id === row.id ? { ...item, name: to } : item)))
          }
          onSet={(next) =>
            editRows((current) => current.map((item) => (item.id === row.id ? { ...item, value: next } : item)))
          }
          onDrop={() => editRows((current) => current.filter((item) => item.id !== row.id))}
        />
      ))}

      {duplicated && (
        <p className="inspector__warn">two options share a name: only one will be left in the file.</p>
      )}

      <button
        type="button"
        className="inspector__quiet"
        onClick={() => editRows((current) => [...current, { id: nextId.current++, name: "", value: "" }])}
      >
        add an option
      </button>

      {preview !== "" && (
        <p className="inspector__preview" title="what is being composed, not what ends up in the file">
          <span>gives:</span> <code>{preview}</code>
        </p>
      )}
    </div>
  );
}

/** An option the descriptor declares: its shape decides the control. */
function DeclaredOption({
  spec,
  value,
  onSet,
  onClear,
}: {
  spec: OptionSpec;
  value: OptionValue | undefined;
  onSet: (value: OptionValue) => void;
  onClear: () => void;
}) {
  const chosen = value !== undefined;
  const label = spec.label ?? spec.key;
  return (
    <div className="inspector__option" data-declared>
      <div className="inspector__label" title={spec.help}>
        {label}
      </div>
      {spec.kind === "flag" ? (
        <label className="inspector__dep">
          <input
            type="checkbox"
            checked={value === true}
            onChange={(event) => (event.target.checked ? onSet(true) : onClear())}
          />
          {label}
        </label>
      ) : spec.kind === "choice" ? (
        <select
          className="inspector__select"
          aria-label={label}
          value={chosen ? String(value) : ""}
          onChange={(event) => (event.target.value === "" ? onClear() : onSet(event.target.value))}
        >
          <option value="">— not chosen —</option>
          {(spec.choices ?? []).map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      ) : (
        <input
          className="inspector__input inspector__input--data"
          type={spec.kind === "number" ? "number" : "text"}
          aria-label={label}
          value={chosen ? String(value) : ""}
          onChange={(event) => {
            const text = event.target.value;
            if (text === "") onClear();
            else onSet(spec.kind === "number" ? Number(text) : text);
          }}
        />
      )}
      {spec.help && <p className="inspector__why">{spec.help}</p>}
    </div>
  );
}

/** An option written by hand: a name, a value, and a way to drop it. */
function FreeOption({
  name,
  value,
  onRename,
  onSet,
  onDrop,
}: {
  name: string;
  value: OptionValue;
  onRename: (to: string) => void;
  onSet: (value: OptionValue) => void;
  onDrop: () => void;
}) {
  return (
    <div className="inspector__option">
      <input
        className="inspector__input inspector__input--data"
        aria-label="Option name"
        value={name}
        placeholder="--option"
        onChange={(event) => onRename(event.target.value)}
      />
      {/* A switch is an option without a value: it is declared with `true`, and
          writing it so instead of leaving the value empty is the difference
          between `--verbose` and `--verbose ""`. */}
      {value === true ? (
        <span className="inspector__readonly">no value</span>
      ) : (
        <input
          className="inspector__input inspector__input--data"
          aria-label="Option value"
          value={String(value)}
          placeholder="value"
          onChange={(event) => onSet(event.target.value)}
        />
      )}
      <div className="inspector__option-foot">
        <label className="inspector__dep" title="an option that wants no value">
          <input
            type="checkbox"
            checked={value === true}
            onChange={(event) => onSet(event.target.checked ? true : "")}
          />
          alone
        </label>
        <button type="button" className="inspector__quiet" onClick={onDrop} title="drop this option">
          drop
        </button>
      </div>
    </div>
  );
}
