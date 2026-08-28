import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";

const host = document.getElementById("root");
if (!host) throw new Error("manca #root: la pagina non è quella attesa");

createRoot(host).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
