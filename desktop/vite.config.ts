import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// La porta è fissa perché il guscio nativo la apre per nome, non per scoperta.
// Diversa da quella di `sailor ui` (47831), che serve la pagina in sola lettura:
// finché le due esistono insieme non devono litigare.
export default defineConfig({
  plugins: [react()],
  server: { port: 5183, strictPort: true },
  build: { outDir: "dist", emptyOutDir: true },
});
