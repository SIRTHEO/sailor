import type { ChainMark } from "./flowchain";
import { useState } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { FlowFile } from "./flow";
import type { BarFlow } from "./boardhead";

interface FocusBarProps {
  name: string;
  color: string;
  flow: FlowFile;
  bar: BarFlow;
  neverSaved: boolean;
  /** What this flow replaces and the chain, or why the chain is unread. */
  mark?: ChainMark | null;
  error?: string;
  onRename: (next: string) => void;
  onDescription: (text: string) => void;
  onDelete: () => void;
  onWatch?: () => void;
  onSave: () => void;
  onRun: () => void;
}

/**
 * The focused flow's bar: everything that has this flow for a subject, and
 * nothing that does not. One Save, beside the thing it saves, which is all the
 * two-Save objection ever asked. The name is editable only until the first save,
 * since it is the filename; drafts settle on `blur`, or a rename fires per letter.
 */
export function FocusBar({
  name,
  color,
  flow,
  bar,
  neverSaved,
  mark,
  error,
  onRename,
  onDescription,
  onDelete,
  onWatch,
  onSave,
  onRun,
}: FocusBarProps) {
  const [nameDraft, setNameDraft] = useState(name);
  const [descDraft, setDescDraft] = useState(flow.description);
  const statusBody = (
    <>
      <span className="focusbar__live" data-idle={bar.status.live ? undefined : true} />
      <span className="focusbar__status-word">{bar.status.word}</span>
    </>
  );

  return (
    <div className="focusbar">
      <span className="focusbar__dot" style={{ background: color }} />
      {neverSaved ? (
        <input
          className="focusbar__name-input"
          value={nameDraft}
          aria-label="name of the flow"
          onChange={(event) => setNameDraft(event.target.value)}
          onBlur={() => onRename(nameDraft)}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
      ) : (
        <span className="focusbar__name" title="the name is the file's: it is chosen before saving">
          {name}
        </span>
      )}
      <span className="focusbar__steps">{bar.steps} steps</span>
      {mark && (
        <span className="focusbar__chain" title={mark.title}>
          {mark.text}
        </span>
      )}
      <input
        className="focusbar__desc-input"
        value={descDraft}
        aria-label="description of the flow"
        placeholder="what this flow is for"
        onChange={(event) => setDescDraft(event.target.value)}
        onBlur={() => onDescription(descDraft)}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
      />
      <div className="focusbar__spacer" />
      {onWatch ? (
        <button type="button" className="focusbar__status" onClick={onWatch}>
          {statusBody}
        </button>
      ) : (
        <span className="focusbar__status">{statusBody}</span>
      )}
      {bar.dirty && (
        <span className="focusbar__dirty">
          <span className="focusbar__dirty-dot" />
          unsaved changes
        </span>
      )}
      {error && <span className="focusbar__error">{error}</span>}
      {/* DELETING IS NOT WHAT THIS BAR IS FOR. A red button beside the name of
          the thing it destroys is the loudest object on the screen, and it is
          the one gesture nobody comes here to make. It keeps its place — one
          click away, under the mark that always means «what else can I do». */}
      <DropdownMenu>
        <Tooltip>
          <TooltipTrigger asChild>
            <DropdownMenuTrigger asChild disabled={bar.busy}>
              <button type="button" className="focusbar__more" aria-label="more for this flow">
                ⋯
              </button>
            </DropdownMenuTrigger>
          </TooltipTrigger>
          {/* The mark has no word beside it, so it needs one the instant the
              pointer arrives: ban 5 does not stop at colour. */}
          <TooltipContent side="bottom">More for this flow</TooltipContent>
        </Tooltip>
        <DropdownMenuContent align="end">
          <DropdownMenuItem variant="destructive" onSelect={onDelete}>
            {neverSaved ? "Discard flow" : "Delete flow"}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {/* THE TWO GESTURES STAY TOGETHER AND DO NOT WRAP: the words beside them
          give ground first, so Run keeps one place at every width. */}
      <div className="focusbar__actions">
        <button type="button" className="focusbar__save" onClick={onSave} disabled={!bar.dirty || bar.busy}>
          {bar.busy ? "Saving…" : "Save"}
        </button>
        {/* THE ACCENT MEANS «THE ACTION», and this is the action. Not a green:
            green is a step that went well, and prohibition 4 keeps the state
            colours for states. */}
        <button type="button" className="focusbar__run is-primary" onClick={onRun} disabled={bar.starting}>
          <span className="focusbar__glyph" aria-hidden="true">
            ▶
          </span>
          {bar.starting ? "Starting…" : "Run"}
        </button>
      </div>
    </div>
  );
}
