// Palette search (P16) — bridges the registry corpus to the Rust nucleo command
// and layers history-boost ranking on top.
//
// Fuzzy ranking itself lives in Rust (`palette_search`, in-process nucleo). This
// module: (1) computes a per-id boost from usage history (recency + count) and
// passes it to Rust so recently/frequently-run commands float up; (2) records a
// usage event when a command runs and persists it via the injected store. No JS
// fuzzy fallback — the Rust matcher is in-process and reliable (YAGNI).

import type { CommandContext } from "./command-registry";

/** One searchable entry sent to the Rust matcher (mirror of Rust `PaletteCandidate`). */
export interface PaletteCandidate {
  id: string;
  text: string;
  keywords?: string;
  rank?: number;
}

/** A ranked hit from Rust (mirror of Rust `PaletteMatch`). */
export interface PaletteMatch {
  id: string;
  score: number;
  /** Char offsets into `text` that matched, for highlight. */
  indices: number[];
}

/** Per-id usage record persisted across sessions. */
export interface UsageRecord {
  count: number;
  /** Epoch ms of the most recent run. */
  lastUsed: number;
}

export type UsageMap = Record<string, UsageRecord>;

/** Tauri `invoke` boundary, injected so the module is testable in jsdom/node. */
export type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

/** Loads/saves the usage map (P12 config store seam — host injects the real one). */
export interface UsageStore {
  load(): Promise<UsageMap>;
  save(usage: UsageMap): Promise<void>;
}

// History-boost weights. `count` rewards frequency; recency decays so a command
// used seconds ago beats one used last week. Both are well under nucleo's exact-
// match band so a strong text match still wins over pure history.
const COUNT_WEIGHT = 60;
const RECENCY_MAX = 600;
const RECENCY_HALF_LIFE_MS = 1000 * 60 * 60 * 24 * 3; // 3 days

/** Boost for one id from its usage: frequency term + exponentially-decayed recency. */
export function boostForRecord(record: UsageRecord, now: number): number {
  const frequency = Math.log2(record.count + 1) * COUNT_WEIGHT;
  const age = Math.max(0, now - record.lastUsed);
  const recency = RECENCY_MAX * Math.pow(0.5, age / RECENCY_HALF_LIFE_MS);
  return frequency + recency;
}

/** Build the id→boost map the Rust command consumes. */
export function boostMap(usage: UsageMap, now: number = Date.now()): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [id, record] of Object.entries(usage)) {
    out[id] = boostForRecord(record, now);
  }
  return out;
}

/** Drives the Rust matcher with history boost + persists usage on run. */
export class PaletteSearch {
  private usage: UsageMap = {};

  constructor(
    private readonly invoke: InvokeFn,
    private readonly store: UsageStore,
  ) {}

  /** Load persisted usage (call once when the overlay mounts). */
  async init(): Promise<void> {
    this.usage = await this.store.load();
  }

  /** Rank `corpus` against `query`; boosts come from loaded usage. */
  async search(query: string, corpus: PaletteCandidate[]): Promise<PaletteMatch[]> {
    return this.invoke<PaletteMatch[]>("palette_search", {
      query,
      corpus,
      boosts: boostMap(this.usage),
    });
  }

  /** Record a run of `id` (bumps count + recency) and persist it. */
  async recordUsage(id: string, now: number = Date.now()): Promise<void> {
    const prev = this.usage[id];
    this.usage[id] = {
      count: (prev?.count ?? 0) + 1,
      lastUsed: now,
    };
    await this.store.save(this.usage);
  }

  /** Current in-memory usage (test/inspection helper). */
  getUsage(): UsageMap {
    return this.usage;
  }
}

// `CommandContext` is re-exported for hosts wiring the search to the registry.
export type { CommandContext };
