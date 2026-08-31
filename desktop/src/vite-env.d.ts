/**
 * Il foglio di stile letto come testo.
 *
 * I controlli dei divieti devono leggere `styles.css` così com'è scritto. La
 * via ovvia — `node:fs` — vorrebbe `@types/node`, cioè una dipendenza in più su
 * un progetto che ne tiene nove in tutto; questa passa dal bundler che c'è già.
 */
declare module "*.css?raw" {
  const source: string;
  export default source;
}
