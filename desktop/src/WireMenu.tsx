/**
 * The menu that opens where a wire was let go, or where a node's «+» was
 * pressed. Pick a family and the step is born already depending on the one the
 * wire came from — creating and connecting in one gesture, which is the whole
 * reason this exists.
 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { KIND_LABEL, KindIcon } from "./StepNode";
import { TOOL_GROUPS } from "./Toolbar";
import type { StepKind } from "./flow";
import { placeMenu } from "./menuplace";

export interface WireMenuProps {
  /** Where it opens, in window coordinates. */
  at: { x: number; y: number };
  /** The step the new one will depend on. */
  from: string;
  onPick: (kind: StepKind) => void;
  onClose: () => void;
}

export function WireMenu({ at, from, onPick, onClose }: WireMenuProps) {
  const box = useRef<HTMLDivElement>(null);
  const [where, setWhere] = useState(at);

  // Measured after the first paint, because where it goes depends on how tall
  // it turned out: opened low on the canvas it would hang off the bottom, and
  // its items could not be clicked at all.
  useLayoutEffect(() => {
    const rect = box.current?.getBoundingClientRect();
    if (!rect) return;
    const { left, top } = placeMenu(at, { width: rect.width, height: rect.height }, {
      width: window.innerWidth,
      height: window.innerHeight,
    });
    setWhere({ x: left, y: top });
  }, [at]);

  // Escape closes it, and so does a press anywhere else: a menu that can only
  // be dismissed by choosing something makes the wrong choice the cheap one.
  useEffect(() => {
    const key = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    const away = (event: MouseEvent) => {
      if (!box.current?.contains(event.target as Node)) onClose();
    };
    window.addEventListener("keydown", key);
    window.addEventListener("mousedown", away);
    box.current?.querySelector("button")?.focus();
    return () => {
      window.removeEventListener("keydown", key);
      window.removeEventListener("mousedown", away);
    };
  }, [onClose]);

  return (
    <div
      ref={box}
      className="wiremenu"
      style={{ left: where.x, top: where.y }}
      role="menu"
      aria-label={`what follows «${from}»`}
    >
      <p className="wiremenu__head">after «{from}»</p>
      {/* The families are read from the toolbar, never written again here: two
          hand-kept lists of the same thing drift, and one of them silently
          stops offering what the other does. */}
      {TOOL_GROUPS.map((group) => (
        <div className="wiremenu__group" key={group.label} role="group" aria-label={group.label}>
          {group.kinds.map((kind) => (
            <button
              key={kind}
              type="button"
              role="menuitem"
              className="wiremenu__item"
              onClick={() => onPick(kind)}
            >
              <KindIcon kind={kind} className="wiremenu__mark" />
              {KIND_LABEL[kind]}
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}
