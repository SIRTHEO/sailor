/// <reference types="vite/client" />

/**
 * The stylesheet read as text. The bans' checks must read `styles.css` exactly
 * as written, and the obvious way — `node:fs` — would want `@types/node`, one
 * more dependency on a project that keeps nine in all. This one goes through
 * the bundler that is already there.
 */
declare module "*.css?raw" {
  const source: string;
  export default source;
}
