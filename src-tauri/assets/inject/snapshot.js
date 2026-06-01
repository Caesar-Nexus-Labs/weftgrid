"use strict";
(() => {
  // inject/dom-walk.ts
  function makeRefTable() {
    return { map: {}, counter: 0 };
  }
  function resetRefs(refs) {
    refs.map = {};
    refs.counter = 0;
  }
  function allocRef(refs, cssPath2) {
    refs.counter += 1;
    const token = `e${refs.counter}`;
    refs.map[token] = cssPath2;
    return token;
  }
  var INTERACTIVE_ROLES = /* @__PURE__ */ new Set([
    "button",
    "link",
    "textbox",
    "checkbox",
    "radio",
    "combobox",
    "listbox",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "searchbox",
    "slider",
    "spinbutton",
    "switch",
    "tab",
    "treeitem"
  ]);
  var CONTENT_ROLES = /* @__PURE__ */ new Set([
    "heading",
    "cell",
    "gridcell",
    "columnheader",
    "rowheader",
    "listitem",
    "article",
    "region",
    "main",
    "navigation"
  ]);
  var STRUCTURAL_ROLES = /* @__PURE__ */ new Set([
    "generic",
    "group",
    "list",
    "table",
    "row",
    "rowgroup",
    "grid",
    "treegrid",
    "menu",
    "menubar",
    "toolbar",
    "tablist",
    "tree",
    "directory",
    "document",
    "application",
    "presentation",
    "none"
  ]);
  function normalize(s) {
    return String(s ?? "").replace(/\s+/g, " ").trim();
  }
  function isElementVisible(el, win) {
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
  function implicitRole(el) {
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
  function nameFor(el, doc) {
    const aria = normalize(el.getAttribute("aria-label") || "");
    if (aria) return aria;
    const labelledBy = normalize(el.getAttribute("aria-labelledby") || "");
    if (labelledBy) {
      const text2 = labelledBy.split(/\s+/).map((id) => doc.getElementById(id)).filter(Boolean).map((n) => normalize(n.textContent || "")).join(" ").trim();
      if (text2) return text2;
    }
    if (String(el.tagName || "").toLowerCase() === "input") {
      const placeholder = normalize(el.getAttribute("placeholder") || "");
      if (placeholder) return placeholder;
      const value = normalize(el.value || "");
      if (value) return value;
    }
    const title = normalize(el.getAttribute("title") || "");
    if (title) return title;
    const text = normalize(el.innerText || el.textContent || "");
    if (text) return text.slice(0, 120);
    return "";
  }
  function cssPath(el, maxParts = 6) {
    if (!el || el.nodeType !== 1) return null;
    if (el.id) return "#" + cssEscape(el.id);
    const parts = [];
    let cur = el;
    while (cur && cur.nodeType === 1) {
      let part = String(cur.tagName || "").toLowerCase();
      if (!part) break;
      if (cur.id) {
        part += "#" + cssEscape(cur.id);
        parts.unshift(part);
        break;
      }
      const tag = part;
      const parent = cur.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children).filter((n) => String(n.tagName || "").toLowerCase() === tag);
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
  function cssEscape(value) {
    const g = globalThis;
    if (g.CSS && typeof g.CSS.escape === "function") return g.CSS.escape(value);
    return value.replace(/[^a-zA-Z0-9_-]/g, (ch) => "\\" + ch);
  }
  function buildSnapshot(env, refs, options = {}) {
    const { doc, win } = env;
    const interactiveOnly = options.interactiveOnly ?? false;
    const compact = options.compact ?? true;
    const maxDepth = options.maxDepth ?? 50;
    const visible = options.isVisible ?? ((el) => isElementVisible(el, win));
    const root = options.scopeSelector ? doc.querySelector(options.scopeSelector) || doc.body || doc.documentElement : doc.body || doc.documentElement;
    const entries = [];
    const seen = /* @__PURE__ */ new Set();
    const appendEntry = (el, depth, forcedRole) => {
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
    const walk = (node, depth) => {
      if (!node || depth > maxDepth || node.nodeType !== 1) return;
      appendEntry(node, depth, null);
      for (const child of Array.from(node.children || [])) {
        walk(child, depth + 1);
      }
    };
    if (root) walk(root, 0);
    const refMap = {};
    const treeLines = [];
    for (const entry of entries) {
      const role = entry.role || "generic";
      const name = entry.name.trim();
      const ref = allocRef(refs, entry.selector);
      const info = { role };
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
    const text = body ? String(body.innerText || body.textContent || "") : "";
    const html = doc.documentElement ? String(doc.documentElement.outerHTML || "") : "";
    const titleForTree = title ? title.replace(/"/g, "'") : "page";
    const lines = [`- document "${titleForTree}"`];
    if (treeLines.length) {
      lines.push(...treeLines);
    } else {
      const excerpt = text.replace(/[\n\t]/g, " ").trim();
      lines.push(excerpt ? `- text "${excerpt.slice(0, 240).replace(/"/g, "'")}"` : "- (empty)");
    }
    return {
      title,
      url: String(win.location?.href || ""),
      readyState: String(doc.readyState || ""),
      text,
      html,
      snapshotText: lines.join("\n"),
      refs: refMap,
      entries
    };
  }

  // inject/dom-actions.ts
  function reactCompatibleSetValue(el, newValue) {
    let nativeSetter = null;
    for (let proto = Object.getPrototypeOf(el); proto; proto = Object.getPrototypeOf(proto)) {
      const desc = Object.getOwnPropertyDescriptor(proto, "value");
      if (desc && desc.set) {
        nativeSetter = desc.set;
        break;
      }
    }
    if (nativeSetter) {
      nativeSetter.call(el, newValue);
    } else {
      el.value = newValue;
    }
  }
  function clickElement(el) {
    if (!el) return { ok: false, error: "not_found" };
    el.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    const htmlEl = el;
    if (typeof htmlEl.click === "function") {
      htmlEl.click();
    } else {
      el.dispatchEvent(new MouseEvent("click", {
        bubbles: true,
        cancelable: true,
        view: window,
        detail: 1
      }));
    }
    return { ok: true };
  }
  function fillElement(el, text) {
    if (!el) return { ok: false, error: "not_found" };
    el.focus?.();
    const newValue = String(text);
    if ("value" in el) {
      reactCompatibleSetValue(el, newValue);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    } else {
      el.textContent = newValue;
    }
    return { ok: true };
  }
  function getFromElement(el, kind, win, attr) {
    if (!el) return { ok: false, error: "not_found" };
    const htmlEl = el;
    switch (kind) {
      case "text":
        return { ok: true, value: String(htmlEl.innerText || el.textContent || "").trim() };
      case "html":
        return { ok: true, value: String(el.outerHTML || "") };
      case "value":
        return { ok: true, value: String(el.value ?? "") };
      case "attr":
        return { ok: true, value: attr ? el.getAttribute(attr) : null };
      case "box": {
        const r = el.getBoundingClientRect();
        return {
          ok: true,
          value: {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
            top: r.top,
            left: r.left,
            right: r.right,
            bottom: r.bottom
          }
        };
      }
      case "styles": {
        const style = win.getComputedStyle(el);
        if (attr) return { ok: true, value: style.getPropertyValue(attr) };
        return {
          ok: true,
          value: {
            display: style.display,
            visibility: style.visibility,
            opacity: style.opacity,
            color: style.color,
            background: style.background,
            width: style.width,
            height: style.height
          }
        };
      }
      default:
        return { ok: false, error: "invalid_params" };
    }
  }
  function isVisible(el, win) {
    if (!el) return { ok: false, error: "not_found" };
    return { ok: true, value: isElementVisible(el, win) };
  }
  function isEnabled(el) {
    if (!el) return { ok: false, error: "not_found" };
    return { ok: true, value: !el.disabled };
  }
  function isChecked(el) {
    if (!el) return { ok: false, error: "not_found" };
    const checked = "checked" in el ? !!el.checked : false;
    return { ok: true, value: checked };
  }
  function countSelector(doc, selector) {
    try {
      return { ok: true, value: doc.querySelectorAll(selector).length };
    } catch {
      return { ok: false, error: "invalid_selector" };
    }
  }
  function findBySelector(doc, selector) {
    let el;
    try {
      el = doc.querySelector(selector);
    } catch {
      return { ok: false, error: "invalid_selector" };
    }
    if (!el) return { ok: false, error: "not_found" };
    const path = cssPath(el, Number.POSITIVE_INFINITY);
    if (!path) return { ok: false, error: "not_found" };
    return {
      ok: true,
      selector: path,
      tag: String(el.tagName || "").toLowerCase(),
      text: normalize(el.textContent || "")
    };
  }
  function evalWaitCondition(env, cond) {
    const { doc, win } = env;
    switch (cond.kind) {
      case "selector":
        try {
          return !!doc.querySelector(cond.selector);
        } catch {
          return false;
        }
      case "urlContains":
        return String(win.location?.href || "").includes(cond.value);
      case "textContains":
        return !!doc.body && String(doc.body.innerText || doc.body.textContent || "").includes(cond.value);
      case "loadState": {
        const state = String(doc.readyState || "").toLowerCase();
        if (cond.value.toLowerCase() === "interactive") {
          return state === "interactive" || state === "complete";
        }
        return state === cond.value.toLowerCase();
      }
      case "function":
        try {
          return !!new Function(`return (${cond.expr});`)();
        } catch {
          return false;
        }
    }
  }

  // inject/snapshot.ts
  var WEFT_INJECT_MARKER = "weftgrid-inject-stub";
  var WEFT_INJECT_VERSION = "p7-domwalk-1";
  (() => {
    const w = window;
    const bridge = w.__weft || {};
    const refs = makeRefTable();
    const resolve = (cmd) => {
      const raw = cmd.ref ?? cmd.selector ?? "";
      const token = raw.startsWith("@") ? raw.slice(1) : raw;
      const mapped = refs.map[token];
      const selector = mapped ?? raw;
      if (!selector) return null;
      try {
        return document.querySelector(selector);
      } catch {
        return null;
      }
    };
    const run = (cmd) => {
      switch (cmd.action) {
        case "snapshot": {
          resetRefs(refs);
          return buildSnapshot({ doc: document, win: window }, refs, cmd.options || {});
        }
        case "click":
          return clickElement(resolve(cmd));
        case "fill":
          return fillElement(resolve(cmd), cmd.text ?? cmd.value ?? "");
        case "get":
          return getFromElement(resolve(cmd), cmd.kind || "text", window, cmd.attr);
        case "count":
          return countSelector(document, cmd.selector ?? "");
        case "isVisible":
          return isVisible(resolve(cmd), window);
        case "isEnabled":
          return isEnabled(resolve(cmd));
        case "isChecked":
          return isChecked(resolve(cmd));
        case "find": {
          const res = findBySelector(document, cmd.selector ?? "");
          if (res.ok && res.selector) {
            refs.counter += 1;
            const ref = `e${refs.counter}`;
            refs.map[ref] = res.selector;
            return { ...res, ref };
          }
          return res;
        }
        case "wait":
          return cmd.wait ? { ok: true, value: evalWaitCondition({ doc: document, win: window }, cmd.wait) } : { ok: false, error: "invalid_params" };
        default:
          return { ok: false, error: "unknown_action" };
      }
    };
    const dispatch = (cmd) => {
      let payload;
      try {
        payload = JSON.stringify({ id: cmd.id, ok: true, result: run(cmd) });
      } catch (err) {
        payload = JSON.stringify({
          id: cmd.id,
          ok: false,
          error: String(err?.message || err)
        });
      }
      try {
        bridge.postMessage?.(payload);
      } catch {
      }
      return payload;
    };
    w.__weft = Object.assign(bridge, {
      version: WEFT_INJECT_VERSION,
      marker: WEFT_INJECT_MARKER,
      refs,
      dispatch
    });
  })();
})();
