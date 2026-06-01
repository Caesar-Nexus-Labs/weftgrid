// @vitest-environment jsdom
//
// jsdom STRUCTURE smoke test (P7) — verifies the DOM-walk emits well-formed
// `:nth-of-type` ref paths and a `[ref=eN]` snapshot text. This is NOT a parity
// gate: jsdom has no layout engine (getBoundingClientRect = 0), so real
// visibility filtering can't run here. We inject an `isVisible` override to
// exercise the tree-walk structure deterministically. Cross-OS byte-parity is
// proven by the real-engine golden harness (tests/automation-parity/), which
// runs on WebView2 + WebKitGTK in CI — see that dir's README.

import { describe, it, expect } from "vitest";
import {
  buildSnapshot, cssPath, makeRefTable, allocRef, resetRefs, normalize,
} from "../../inject/dom-walk";
import {
  findBySelector, fillElement, reactCompatibleSetValue, getFromElement, isChecked,
} from "../../inject/dom-actions";

/** jsdom has zero layout — force everything visible to test structure only. */
const ALWAYS_VISIBLE = () => true;

function mount(html: string): { doc: Document; win: Window } {
  document.body.innerHTML = html;
  return { doc: document, win: window };
}

describe("cssPath (:nth-of-type structure)", () => {
  it("prefers an element id over a positional path", () => {
    mount(`<div><button id="go">Go</button></div>`);
    const el = document.getElementById("go")!;
    expect(cssPath(el)).toBe("#go");
  });

  it("emits :nth-of-type only when siblings share the tag", () => {
    mount(`<ul><li>a</li><li>b</li><li>c</li></ul>`);
    const items = document.querySelectorAll("li");
    expect(cssPath(items[1])).toContain("li:nth-of-type(2)");
    // a tag that is unique among its siblings gets no nth-of-type qualifier,
    // but the path still carries the ancestor chain (cmux walks up to maxParts).
    const ulPath = cssPath(document.querySelector("ul")!);
    expect(ulPath).not.toContain(":nth-of-type");
    expect(ulPath?.endsWith("ul")).toBe(true);
  });

  it("fully-qualifies the path up to the root when maxParts is unbounded (find)", () => {
    mount(`<section><div><p>x</p><p>y</p></div></section>`);
    const target = document.querySelectorAll("p")[1];
    const path = cssPath(target, Number.POSITIVE_INFINITY);
    expect(path).toContain("p:nth-of-type(2)");
    // walk reaches the document root; jsdom roots at <html>
    expect(path?.startsWith("html")).toBe(true);
  });
});

describe("ref table (ephemeral eN allocation)", () => {
  it("allocates monotonically and maps eN -> css path", () => {
    const refs = makeRefTable();
    expect(allocRef(refs, "#a")).toBe("e1");
    expect(allocRef(refs, "div > button")).toBe("e2");
    expect(refs.map.e2).toBe("div > button");
  });

  it("resets on re-snapshot (re-snapshot-before-act invariant)", () => {
    const refs = makeRefTable();
    allocRef(refs, "#a");
    resetRefs(refs);
    expect(refs.counter).toBe(0);
    expect(allocRef(refs, "#b")).toBe("e1");
  });
});

describe("buildSnapshot (structure, forced-visible)", () => {
  it("produces document header + [ref=eN] tokens for interactive nodes", () => {
    mount(`<main><button>Save</button><a href="/x">Home</a></main>`);
    const refs = makeRefTable();
    const snap = buildSnapshot({ doc: document, win: window }, refs, {
      isVisible: ALWAYS_VISIBLE,
    });
    expect(snap.snapshotText.startsWith("- document")).toBe(true);
    expect(snap.snapshotText).toContain("[ref=e1]");
    // refs map carries role/name for each emitted entry
    const roles = Object.values(snap.refs).map((r) => r.role);
    expect(roles).toContain("button");
    expect(roles).toContain("link");
  });

  it("is deterministic: same DOM -> identical text + ref ids", () => {
    const html = `<div><button>One</button><button>Two</button></div>`;
    mount(html);
    const a = buildSnapshot({ doc: document, win: window }, makeRefTable(), {
      isVisible: ALWAYS_VISIBLE,
    });
    mount(html);
    const b = buildSnapshot({ doc: document, win: window }, makeRefTable(), {
      isVisible: ALWAYS_VISIBLE,
    });
    expect(a.snapshotText).toBe(b.snapshotText);
    expect(Object.keys(a.refs)).toEqual(Object.keys(b.refs));
  });

  it("falls back to text excerpt when no roles match", () => {
    mount(`<div>just some prose</div>`);
    const snap = buildSnapshot({ doc: document, win: window }, makeRefTable(), {
      isVisible: ALWAYS_VISIBLE,
    });
    expect(snap.snapshotText).toContain("just some prose");
  });
});

describe("action bodies (structure)", () => {
  it("find resolves a selector to a fully-qualified nth-of-type path", () => {
    mount(`<ul><li>a</li><li><a href="/y">link</a></li></ul>`);
    const res = findBySelector(document, "li:nth-of-type(2) a");
    expect(res.ok).toBe(true);
    expect(res.selector).toContain("a");
    expect(res.tag).toBe("a");
  });

  it("fill uses the React-compatible native setter and fires input", () => {
    mount(`<input id="name" />`);
    const input = document.getElementById("name") as HTMLInputElement;
    let inputFired = false;
    input.addEventListener("input", () => (inputFired = true));
    const res = fillElement(input, "hello");
    expect(res.ok).toBe(true);
    expect(input.value).toBe("hello");
    expect(inputFired).toBe(true);
  });

  it("reactCompatibleSetValue walks the prototype chain", () => {
    mount(`<input id="x" />`);
    const input = document.getElementById("x") as HTMLInputElement;
    reactCompatibleSetValue(input, "via-setter");
    expect(input.value).toBe("via-setter");
  });

  it("get value/attr reads element state", () => {
    mount(`<input id="x" value="v" data-test="t" />`);
    const input = document.getElementById("x")!;
    expect(getFromElement(input, "value", window).value).toBe("v");
    expect(getFromElement(input, "attr", window, "data-test").value).toBe("t");
  });

  it("isChecked reflects checkbox state", () => {
    mount(`<input id="c" type="checkbox" checked />`);
    const box = document.getElementById("c")!;
    expect(isChecked(box).value).toBe(true);
  });
});

describe("normalize", () => {
  it("collapses whitespace and trims", () => {
    expect(normalize("  a\n\t b   c ")).toBe("a b c");
    expect(normalize(null)).toBe("");
  });
});
