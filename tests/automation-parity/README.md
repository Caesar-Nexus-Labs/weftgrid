# Automation golden parity harness (P7)

Cross-OS byte-parity gate for the single inject-JS DOM-walk
(`inject/snapshot.ts` → `src-tauri/assets/inject/snapshot.js`). One algorithm
runs on **both** WebView2 (Windows) and WebKitGTK (Linux); this harness proves
the snapshot text and ephemeral refs `eN` are byte-identical across OS, so agent
scripts written on one platform target the same elements on the other.

## Why NOT jsdom

jsdom has no layout engine: `getBoundingClientRect()` returns all zeros, so the
visibility filter (`isElementVisible`) can't run. Parity bugs live exactly in
layout/visibility. A jsdom "pass" would be meaningless and could hide a real
divergence. Therefore:

- **jsdom is used only for STRUCTURE smoke** (`src/automation/snapshot-script.test.ts`):
  ref→`:nth-of-type` format, deterministic text, action bodies — with an
  `isVisible` override. It is NOT a parity gate.
- **Parity runs on real engines in CI** via this harness.

## Status: real-engine execution DEFERRED to CI

This directory ships the pieces that DON'T need a live webview:

- `fixtures/account-settings.html` — fixed, layout-stable page.
- `golden/account-settings.snapshot.txt` — expected snapshot text.
- `golden/account-settings.refs.json` — expected `eN` → {role, name}.
- `parity-harness.ts` — engine-agnostic compare logic + `EngineDriver` seam +
  `assertCrossEngineParity(a, b)`.

What's still TODO (CI follow-up, cannot run in this dev environment):

1. A **WebView2 driver** (Windows) implementing `EngineDriver`.
2. A **WebKitGTK driver** (Linux, headless) implementing `EngineDriver`.
3. A CI workflow that builds the inject bundle (`npm run build:inject`), runs
   both drivers over `PARITY_CASES`, and calls `assertCrossEngineParity`.
4. **Confirm the goldens** against the first real-engine run. The checked-in
   golden files are derived from the deterministic algorithm trace; the first CI
   run is the source of truth and may require a one-time golden refresh (e.g. the
   exact `innerText`/whitespace a real engine emits for a checkbox name).

Until then the goldens are a documented reference, not an enforced gate. Do not
wire this harness into the jsdom vitest run.

## Adding a fixture

1. Add `fixtures/<name>.html` (inline styles only — no external CSS/fonts/async).
2. Add `golden/<name>.snapshot.txt` + `golden/<name>.refs.json`.
3. Append a `ParityCase` to `PARITY_CASES` in `parity-harness.ts`.
