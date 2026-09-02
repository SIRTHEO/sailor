import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";

const host = document.getElementById("root");
if (!host) throw new Error("no #root: this is not the expected page");

createRoot(host).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
