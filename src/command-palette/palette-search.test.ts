// Palette-search tests (P16) — history boost computation + Rust command call
// shape + usage persistence. The Rust nucleo command is mocked at the invoke
// boundary (ranking itself is covered by the Rust unit tests).

import { describe, it, expect, vi } from "vitest";
import {
  PaletteSearch,
  boostForRecord,
  boostMap,
  type InvokeFn,
  type UsageStore,
  type UsageMap,
  type PaletteCandidate,
} from "./palette-search";

function memoryStore(initial: UsageMap = {}): UsageStore & { saved: UsageMap[] } {
  let state: UsageMap = { ...initial };
  const saved: UsageMap[] = [];
  return {
    saved,
    async load() {
      return state;
    },
    async save(usage) {
      state = { ...usage };
      saved.push({ ...usage });
    },
  };
}

describe("boost computation", () => {
  it("a more recent record outranks an older one with equal count", () => {
    const now = 1_000_000_000_000;
    const recent = boostForRecord({ count: 1, lastUsed: now }, now);
    const old = boostForRecord({ count: 1, lastUsed: now - 1000 * 60 * 60 * 24 * 30 }, now);
    expect(recent).toBeGreaterThan(old);
  });

  it("a higher count outranks a lower count at equal recency", () => {
    const now = 1_000_000_000_000;
    const frequent = boostForRecord({ count: 20, lastUsed: now }, now);
    const rare = boostForRecord({ count: 1, lastUsed: now }, now);
    expect(frequent).toBeGreaterThan(rare);
  });

  it("boostMap converts every usage record to a positive boost", () => {
    const now = 1_000_000_000_000;
    const map = boostMap({ a: { count: 3, lastUsed: now } }, now);
    expect(map.a).toBeGreaterThan(0);
  });
});

describe("PaletteSearch", () => {
  const corpus: PaletteCandidate[] = [{ id: "split.right", text: "Split Right", rank: 0 }];

  it("search forwards query + corpus + boosts to the Rust palette_search command", async () => {
    const invoke: InvokeFn = vi.fn(async () => []) as InvokeFn;
    const store = memoryStore({ "split.right": { count: 2, lastUsed: Date.now() } });
    const search = new PaletteSearch(invoke, store);
    await search.init();

    await search.search("split", corpus);

    const [cmd, args] = (invoke as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(cmd).toBe("palette_search");
    expect(args.query).toBe("split");
    expect(args.corpus).toEqual(corpus);
    expect((args.boosts as Record<string, number>)["split.right"]).toBeGreaterThan(0);
  });

  it("recordUsage increments count + persists via the store", async () => {
    const invoke: InvokeFn = vi.fn(async () => []) as InvokeFn;
    const store = memoryStore();
    const search = new PaletteSearch(invoke, store);
    await search.init();

    const now = 1_700_000_000_000;
    await search.recordUsage("split.right", now);
    await search.recordUsage("split.right", now + 1000);

    expect(search.getUsage()["split.right"]).toEqual({ count: 2, lastUsed: now + 1000 });
    expect(store.saved).toHaveLength(2);
    expect(store.saved[1]["split.right"].count).toBe(2);
  });

  it("returns the ranked matches the Rust command produced", async () => {
    const matches = [{ id: "split.right", score: 100, indices: [0, 1] }];
    const invoke: InvokeFn = vi.fn(async () => matches) as InvokeFn;
    const search = new PaletteSearch(invoke, memoryStore());
    await search.init();

    const out = await search.search("split", corpus);
    expect(out).toEqual(matches);
  });
});
