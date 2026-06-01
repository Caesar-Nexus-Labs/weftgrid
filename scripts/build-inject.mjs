// Build the page-context inject script (TS → IIFE JS string) for Rust to embed
// via include_str! (P2 Keystone 2, pipeline #2).
//
// Type-checks with tsconfig.inject.json first, then bundles to a self-contained
// IIFE (no ESM imports, no app deps) at src-tauri/assets/inject/snapshot.js.
import { build } from "esbuild";
import { execSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";

const OUT = "src-tauri/assets/inject/snapshot.js";

// 1. strict type-check (separate from app tsconfig)
execSync("npx tsc -p tsconfig.inject.json", { stdio: "inherit" });

// 2. bundle to IIFE string asset
mkdirSync(dirname(OUT), { recursive: true });
await build({
  entryPoints: ["inject/snapshot.ts"],
  outfile: OUT,
  bundle: true,
  format: "iife",
  platform: "browser",
  target: "es2020",
  legalComments: "none",
});

console.log(`[build-inject] wrote ${OUT}`);
