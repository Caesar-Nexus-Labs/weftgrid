// Golden parity harness scaffold (P7). The CORE assertion logic for cross-OS
// snapshot parity, kept engine-agnostic. A real-engine runner (WebView2 on
// Windows + WebKitGTK on Linux, headless in CI) supplies an `EngineDriver`; this
// module loads a fixture, runs the inject bundle's `snapshot`, and diffs the
// result against the checked-in golden.
//
// IMPORTANT: this harness is NOT wired into the jsdom vitest run on purpose.
// jsdom has zero layout (getBoundingClientRect = 0) so visibility filtering can't
// run — a jsdom "pass" would be meaningless. Real-engine execution is a CI
// follow-up (see README in this dir). Do not import this from src/**/*.test.ts.

import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURES = join(HERE, "fixtures");
const GOLDEN = join(HERE, "golden");

/** Result the inject bundle returns for `{action:'snapshot'}`. */
export interface EngineSnapshot {
  snapshotText: string;
  refs: Record<string, { role: string; name?: string }>;
}

/**
 * Real-engine seam. A platform runner implements this to load a URL/HTML into a
 * live webview, inject the bundle, and evaluate the dispatch call. Both the
 * WebView2 and WebKitGTK CI runners implement the SAME interface so parity is a
 * straight comparison of their outputs against one golden.
 */
export interface EngineDriver {
  /** OS label for diagnostics ("windows-webview2" | "linux-webkitgtk"). */
  readonly engine: string;
  /** Load HTML, inject `INJECT_SNAPSHOT_JS`, run snapshot, return the payload. */
  snapshotFixture(html: string): Promise<EngineSnapshot>;
}

export interface ParityCase {
  name: string;
  fixtureFile: string;
  goldenSnapshotFile: string;
  goldenRefsFile: string;
}

/** The fixtures + goldens this harness asserts. Extend as fixtures grow. */
export const PARITY_CASES: ParityCase[] = [
  {
    name: "account-settings",
    fixtureFile: "account-settings.html",
    goldenSnapshotFile: "account-settings.snapshot.txt",
    goldenRefsFile: "account-settings.refs.json",
  },
];

export function loadFixture(file: string): string {
  return readFileSync(join(FIXTURES, file), "utf8");
}

export function loadGoldenSnapshot(file: string): string {
  return readFileSync(join(GOLDEN, file), "utf8").trimEnd();
}

export function loadGoldenRefs(file: string): Record<string, { role: string; name?: string }> {
  return JSON.parse(readFileSync(join(GOLDEN, file), "utf8"));
}

export interface ParityDiff {
  matches: boolean;
  snapshotMismatch?: { expected: string; actual: string };
  refsMismatch?: { expectedKeys: string[]; actualKeys: string[] };
}

/** Compare one engine snapshot against the golden (byte-exact text + ref keys). */
export function diffAgainstGolden(actual: EngineSnapshot, c: ParityCase): ParityDiff {
  const expectedText = loadGoldenSnapshot(c.goldenSnapshotFile);
  const expectedRefs = loadGoldenRefs(c.goldenRefsFile);
  const diff: ParityDiff = { matches: true };
  if (actual.snapshotText.trimEnd() !== expectedText) {
    diff.matches = false;
    diff.snapshotMismatch = { expected: expectedText, actual: actual.snapshotText };
  }
  const expectedKeys = Object.keys(expectedRefs);
  const actualKeys = Object.keys(actual.refs);
  if (JSON.stringify(expectedKeys) !== JSON.stringify(actualKeys)) {
    diff.matches = false;
    diff.refsMismatch = { expectedKeys, actualKeys };
  }
  return diff;
}

/**
 * Cross-OS parity: run every case on TWO drivers and assert their outputs are
 * byte-identical to each other AND to the golden. The CI workflow calls this with
 * the WebView2 driver and the WebKitGTK driver.
 */
export async function assertCrossEngineParity(
  a: EngineDriver,
  b: EngineDriver,
): Promise<void> {
  for (const c of PARITY_CASES) {
    const html = loadFixture(c.fixtureFile);
    const sa = await a.snapshotFixture(html);
    const sb = await b.snapshotFixture(html);
    if (sa.snapshotText !== sb.snapshotText) {
      throw new Error(
        `parity[${c.name}]: ${a.engine} vs ${b.engine} snapshot text differ`,
      );
    }
    const da = diffAgainstGolden(sa, c);
    if (!da.matches) {
      throw new Error(`parity[${c.name}]: ${a.engine} diverged from golden`);
    }
    const db = diffAgainstGolden(sb, c);
    if (!db.matches) {
      throw new Error(`parity[${c.name}]: ${b.engine} diverged from golden`);
    }
  }
}
