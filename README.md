# weftgrid

A cross-platform terminal for AI agents. Windows-first, Linux next.

weftgrid gives you a terminal, an in-app browser, notifications, workspaces,
splits, tabs, and a CLI to control all of it — a primitive for driving coding
agents, not an opinionated workflow.

Built with Tauri 2 + Rust + TypeScript (Svelte 5).

## Features

- **Terminal core** — GPU-accelerated xterm.js with a portable-pty backend (ConPTY on Windows), flow-control backpressure, and in-terminal find.
- **Panes** — recursive split/tab layout in a single webview, with multi-surface stacking and keyboard-driven directional focus.
- **In-app browser** — overlay browser panes with per-profile isolation, scriptable through a single cross-platform DOM-walk automation backend.
- **Agent control** — a `weft` CLI talks to the running app over an authenticated local socket, so an agent in a shell pane can drive browser automation.
- **Workspaces** — a vertical-tab sidebar where each row is a split-tree, plus a fuzzy command palette with custom `weft.json` commands.
- **Notifications** — OSC 9/99/777 desktop notifications with a per-pane unread ring.
- **Remote** — SSH workspaces with a SOCKS5h broker so a remote browser pane resolves remote hostnames.
- **Browser import** — import cookies and history from local browsers (consent-gated, local-only).

## Status

Early development, Windows-first. The terminal core, pane/split/tab UI, browser
pane, scriptable automation, workspace sidebar, command palette, notifications,
SSH transport, browser import, and the `weft` CLI are implemented and unit-tested;
live-desktop integration and packaging are in progress.

## Development

```bash
npm install
npm run build:inject   # build the page-context inject bundle (required before cargo build/test)
npm run tauri dev      # run the app
```

Tests:

```bash
npm run check                      # type-check the Svelte/TS app (allowJs:false)
cd src-tauri && cargo test         # Rust core tests
```

## License

weftgrid is free software under **GPL-3.0-or-later** (see `LICENSE`).

It is a cross-platform rewrite based on [cmux](https://github.com/manaflow-ai/cmux)
(GPL-3.0-or-later, © Manaflow Inc.), and its browser automation is ported from
[agent-browser](https://github.com/vercel-labs/agent-browser) (Apache-2.0). Full
attributions are in `NOTICE`.
