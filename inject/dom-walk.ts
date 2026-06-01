// Inject DOM-walk algorithm (P7 owner). THE single snapshot builder that runs in
// page-context on BOTH WebView2 (Windows) and WebKitGTK (Linux) so ephemeral refs
// `eN` and snapshot text are byte-identical cross-platform.
//
// Algorithm ported from cmux (GPL-3.0), which ported it from
// vercel-labs/agent-browser (Apache-2.0). Modified for weftgrid, 2026-05-31.
//
// Authored in TS, bundled to a self-contained IIFE JS string — the ONE
// user-approved exception to "no hand-written JS" (page-context, not app logic).

/** Element name/role pair plus its resolved CSS path, captured per visible node. */
export interface SnapshotEntry {
  selector: string;
  role: string;
  name: string;
  depth: number;
}

/** Per-ref metadata surfaced in the snapshot payload (`eN` -> {role, name}). */
export interface RefInfo {
  role: string;
  name?: string;
}

/** Full snapshot payload returned to Rust over the IPC round-trip. */
export interface SnapshotResult {
  title: string;
  url: string;
  readyState: string;
  text: string;
  html: string;
  snapshotText: string;
  refs: Record<string, RefInfo>;
  entries: SnapshotEntry[];
}

/**
 * Ephemeral ref table. Refs are session-local and reset on every `snapshot()`
 * call (the re-snapshot-before-act invariant): a ref `eN` maps to the
 * `:nth-of-type` CSS path of the node at snapshot time. `find()` extends the
 * current table without resetting it, so refs stay unique between snapshots.
 */
export interface RefTable {
  map: Record<string, string>;
  counter: number;
}

export function makeRefTable(): RefTable {
  return { map: {}, counter: 0 };
}

export function resetRefs(refs: RefTable): void {
  refs.map = {};
  refs.counter = 0;
}

/** Allocate the next `eN` ref for a CSS path. Returns the bare token (no `@`). */
export function allocRef(refs: RefTable, cssPath: string): string {
  refs.counter += 1;
  const token = `e${refs.counter}`;
  refs.map[token] = cssPath;
  return token;
}

const INTERACTIVE_ROLES = new Set([
  "button", "link", "textbox", "checkbox", "radio", "combobox", "listbox",
  "menuitem", "menuitemcheckbox", "menuitemradio", "option", "searchbox",
  "slider", "spinbutton", "switch", "tab", "treeitem",
]);
const CONTENT_ROLES = new Set([
  "heading", "cell", "gridcell", "columnheader", "rowheader", "listitem",
  "article", "region", "main", "navigation",
]);
const STRUCTURAL_ROLES = new Set([
  "generic", "group", "list", "table", "row", "rowgroup", "grid", "treegrid",
  "menu", "menubar", "toolbar", "tablist", "tree", "directory", "document",
  "application", "presentation", "none",
]);

export function normalize(s: unknown): string {
  return String(s ?? "").replace(/\s+/g, " ").trim();
}

/**
 * Layout-based visibility. Returns false on jsdom (zero layout) — which is WHY
 * golden parity tests must run on a real engine, not jsdom.
 */
export function isElementVisible(el: Element, win: Window): boolean {
  try {
    if (!el) return false;
    const style = win.getComputedStyle(el);
    const rect = el.getBoundingClientRect();
    if (!style || !rect) return false;
    if (rect.width <= 0 || rect.height <= 0) return false;
    if (style.display === "none" || style.visibility === "hidden") return false;
    if (parseFloat(style.opacity || "1") <= 0.01) return false;
    return true;
  } catch {
    return false;
  }
}

export function implicitRole(el: Element): string | null {
  const tag = String(el.tagName || "").toLowerCase();
  if (tag === "button") return "button";
  if (tag === "a" && el.hasAttribute("href")) return "link";
  if (tag === "input") {
    const type = String(el.getAttribute("type") || "text").toLowerCase();
    if (type === "checkbox") return "checkbox";
    if (type === "radio") return "radio";
    if (type === "submit" || type === "button" || type === "reset") return "button";
    return "textbox";
  }
  if (tag === "textarea") return "textbox";
  if (tag === "select") return "combobox";
  if (tag === "summary") return "button";
  if (["h1", "h2", "h3", "h4", "h5", "h6"].includes(tag)) return "heading";
  if (tag === "li") return "listitem";
  return null;
}

export function nameFor(el: Element, doc: Document): string {
  const aria = normalize(el.getAttribute("aria-label") || "");
  if (aria) return aria;
  const labelledBy = normalize(el.getAttribute("aria-labelledby") || "");
  if (labelledBy) {
    const text = labelledBy.split(/\s+/)
      .map((id) => doc.getElementById(id))
      .filter(Boolean)
      .map((n) => normalize((n as Element).textContent || ""))
      .join(" ").trim();
    if (text) return text;
  }
  if (String(el.tagName || "").toLowerCase() === "input") {
    const placeholder = normalize(el.getAttribute("placeholder") || "");
    if (placeholder) return placeholder;
    const value = normalize((el as HTMLInputElement).value || "");
    if (value) return value;
  }
  const title = normalize(el.getAttribute("title") || "");
  if (title) return title;
  const text = normalize((el as HTMLElement).innerText || el.textContent || "");
  if (text) return text.slice(0, 120);
  return "";
}

/**
 * Stable `:nth-of-type` CSS path. An `id` short-circuits to `#id`. `maxParts`
 * caps the chain (snapshot caps at 6 to keep refs compact; `find()` passes
 * Infinity for a fully-qualified path). This is the ONE ref-path scheme used on
 * both OS — never mixes backend node ids.
 */
export function cssPath(el: Element | null, maxParts = 6): string | null {
  if (!el || el.nodeType !== 1) return null;
  if ((el as HTMLElement).id) return "#" + cssEscape((el as HTMLElement).id);
  const parts: string[] = [];
  let cur: Element | null = el;
  while (cur && cur.nodeType === 1) {
    let part = String(cur.tagName || "").toLowerCase();
    if (!part) break;
    if ((cur as HTMLElement).id) {
      part += "#" + cssEscape((cur as HTMLElement).id);
      parts.unshift(part);
      break;
    }
    const tag = part;
    const parent: Element | null = cur.parentElement;
    if (parent) {
      const siblings = Array.from(parent.children)
        .filter((n) => String(n.tagName || "").toLowerCase() === tag);
      if (siblings.length > 1) {
        const index = siblings.indexOf(cur) + 1;
        part += `:nth-of-type(${index})`;
      }
    }
    parts.unshift(part);
    cur = cur.parentElement;
    if (parts.length >= maxParts) break;
  }
  return parts.join(" > ");
}

/** CSS.escape with a manual fallback (jsdom/older engines may lack it). */
function cssEscape(value: string): string {
  const g = globalThis as { CSS?: { escape?: (v: string) => string } };
  if (g.CSS && typeof g.CSS.escape === "function") return g.CSS.escape(value);
  return value.replace(/[^a-zA-Z0-9_-]/g, (ch) => "\\" + ch);
}

export interface SnapshotOptions {
  interactiveOnly?: boolean;
  compact?: boolean;
  maxDepth?: number;
  scopeSelector?: string | null;
  /** Override for tests (jsdom has zero layout). Defaults to layout-based. */
  isVisible?: (el: Element) => boolean;
}

export interface DomEnv {
  doc: Document;
  win: Window;
}

/**
 * Walk the AX-relevant tree from the scope root, emitting one entry per visible
 * element with a meaningful role, and render the deterministic snapshot text +
 * ref map. Identical input DOM -> identical output on any engine.
 */
export function buildSnapshot(
  env: DomEnv,
  refs: RefTable,
  options: SnapshotOptions = {},
): SnapshotResult {
  const { doc, win } = env;
  const interactiveOnly = options.interactiveOnly ?? false;
  const compact = options.compact ?? true;
  const maxDepth = options.maxDepth ?? 50;
  const visible = options.isVisible ?? ((el: Element) => isElementVisible(el, win));

  const root = options.scopeSelector
    ? doc.querySelector(options.scopeSelector) || doc.body || doc.documentElement
    : doc.body || doc.documentElement;

  const entries: SnapshotEntry[] = [];
  const seen = new Set<string>();

  const appendEntry = (el: Element, depth: number, forcedRole: string | null): void => {
    if (!visible(el)) return;
    const explicitRole = normalize(el.getAttribute("role") || "").toLowerCase();
    const role = forcedRole || explicitRole || implicitRole(el) || "";
    if (!role) return;
    if (interactiveOnly && !INTERACTIVE_ROLES.has(role)) return;
    if (!interactiveOnly) {
      const includeRole = INTERACTIVE_ROLES.has(role) || CONTENT_ROLES.has(role);
      if (!includeRole) return;
      if (compact && STRUCTURAL_ROLES.has(role) && !nameFor(el, doc)) return;
    }
    const selector = cssPath(el, 6);
    if (!selector || seen.has(selector)) return;
    seen.add(selector);
    entries.push({ selector, role, name: nameFor(el, doc), depth });
  };

  const walk = (node: Element | null, depth: number): void => {
    if (!node || depth > maxDepth || node.nodeType !== 1) return;
    appendEntry(node, depth, null);
    for (const child of Array.from(node.children || [])) {
      walk(child, depth + 1);
    }
  };
  if (root) walk(root as Element, 0);

  const refMap: Record<string, RefInfo> = {};
  const treeLines: string[] = [];
  for (const entry of entries) {
    const role = entry.role || "generic";
    const name = entry.name.trim();
    const ref = allocRef(refs, entry.selector);
    const info: RefInfo = { role };
    if (name) info.name = name;
    refMap[ref] = info;
    const indent = "  ".repeat(Math.max(0, entry.depth));
    let line = `${indent}- ${role}`;
    if (name) line += ` "${name.replace(/"/g, "'")}"`;
    line += ` [ref=${ref}]`;
    treeLines.push(line);
  }

  const title = normalize(doc.title || "");
  const body = doc.body;
  const text = body ? String((body as HTMLElement).innerText || body.textContent || "") : "";
  const html = doc.documentElement ? String(doc.documentElement.outerHTML || "") : "";

  const titleForTree = title ? title.replace(/"/g, "'") : "page";
  const lines = [`- document "${titleForTree}"`];
  if (treeLines.length) {
    lines.push(...treeLines);
  } else {
    const excerpt = text.replace(/[\n\t]/g, " ").trim();
    lines.push(excerpt
      ? `- text "${excerpt.slice(0, 240).replace(/"/g, "'")}"`
      : "- (empty)");
  }

  return {
    title,
    url: String(win.location?.href || ""),
    readyState: String(doc.readyState || ""),
    text,
    html,
    snapshotText: lines.join("\n"),
    refs: refMap,
    entries,
  };
}
