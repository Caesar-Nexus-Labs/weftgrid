import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

// Vitest config kept separate from vite.config.js so the SvelteKit plugin (which
// expects a full app build) doesn't run for unit tests. `$lib` is aliased to
// match SvelteKit's default so store/client tests resolve model types.
export default defineConfig({
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL("./src/lib", import.meta.url)),
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
