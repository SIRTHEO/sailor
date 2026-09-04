import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { wearSavedLook } from "./look";
import "./styles.css";

// BEFORE THE FIRST PAINT, not in an effect: a window that opens on the ground
// it was not left on and corrects itself a frame later is a flash of the other
// look every single time.
wearSavedLook();

const host = document.getElementById("root");
if (!host) throw new Error("no #root: this is not the expected page");

createRoot(host).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
