/**
 * The one setting the window has of its own. Every other row of this ground
 * reports what the machine holds; this one changes it, here, with no restart.
 */
import { useState } from "react";
import { LOOKS, savedLook, wear, whatTheMachineSays, type Look } from "./look";

/** What each answer means, in the words of what it does to the window. */
const WHAT: Record<Look, string> = {
  "the machine's": "follows this mac, and changes when it does",
  night: "dark ground, always",
  day: "light ground, always",
};

export function LookScreen() {
  const [look, setLook] = useState<Look>(() => savedLook());
  const machine = whatTheMachineSays();

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Appearance</h2>
      </header>
      <p className="now__mute">
        The ground this window is drawn on. It is kept on this mac and nowhere else: no flow
        reads it, and nothing you run changes because of it.
      </p>

      {LOOKS.map((one) => (
        <button
          type="button"
          key={one}
          className="rail__all"
          data-active={one === look || undefined}
          onClick={() => {
            wear(one);
            setLook(one);
          }}
        >
          {one}
          <span className="rail__note">
            {WHAT[one]}
            {one === "the machine's" && ` — it says ${machine}`}
          </span>
        </button>
      ))}
    </div>
  );
}
