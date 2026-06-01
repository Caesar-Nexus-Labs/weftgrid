<div align="center">

# weftgrid

### A cross-platform terminal for AI agents

Terminal · in-app browser · scriptable automation · workspaces · splits · tabs · CLI —
a primitive for driving coding agents, not an opinionated workflow.

<br />

[![License: GPL v3](https://img.shields.io/badge/License-GPL%20v3-3DA639?style=for-the-badge&logo=gnu&logoColor=white)](LICENSE)
[![Platform](https://img.shields.io/badge/Windows%20first%20%C2%B7%20Linux%20next-0078D6?style=for-the-badge&logo=windows&logoColor=white)](#status)
[![Status](https://img.shields.io/badge/status-early%20development-FFB000?style=for-the-badge)](#status)

<br />

[![Tauri](https://img.shields.io/badge/Tauri-2.11-24C8DB?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-2021-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?style=flat-square&logo=svelte&logoColor=white)](https://svelte.dev)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.6-3178C6?style=flat-square&logo=typescript&logoColor=white)](https://www.typescriptlang.org)
[![Vite](https://img.shields.io/badge/Vite-6-646CFF?style=flat-square&logo=vite&logoColor=white)](https://vite.dev)
[![xterm.js](https://img.shields.io/badge/xterm.js-6.0-3A3A3A?style=flat-square&logo=gnometerminal&logoColor=white)](https://xtermjs.org)

</div>

<br />

## Overview

**weftgrid** gives an AI agent the same surface a developer uses: a real terminal, an
in-app browser it can script, desktop notifications, and a workspace model with splits
and tabs — all controllable from a single `weft` CLI. It is built as a cross-platform
desktop app on **Tauri 2 + Rust + TypeScript (Svelte 5)**, Windows-first with Linux next.

<br />

## Features

<table>
  <tr>
    <td width="50%" valign="top">

#### Terminal core
GPU-accelerated **xterm.js** with a **portable-pty** backend (ConPTY on Windows),
coalesced binary streaming, flow-control backpressure, and in-terminal find.

#### Panes
Recursive **split / tab** layout in a single webview, multi-surface stacking,
keyboard-driven directional focus, and an accessible (ARIA + keyboard) UI.

#### In-app browser
Overlay browser panes with per-profile isolation and physical-coordinate bounds
math (multi-monitor, mixed-DPI correct).

#### Scriptable automation
One cross-platform inject-JS **DOM-walk** backend — snapshot with ephemeral refs,
then `click` / `fill` / `eval` / `wait` / `get` / `find`.

</td>
<td width="50%" valign="top">

#### Agent control
A unified **`weft` CLI** talks to the running app over an authenticated local
socket, so an agent in a shell pane can drive browser automation.

#### Workspaces
A vertical-tab sidebar where each row is a split-tree, plus a fuzzy **command
palette** (in-process Nucleo) with custom `weft.json` commands behind a trust gate.

#### Notifications
**OSC 9 / 99 / 777** desktop notifications with a per-pane unread ring.

#### Remote &amp; import
**SSH** workspaces with a SOCKS5h broker (remote DNS), plus consent-gated import
of cookies &amp; history from local browsers — local-only, no exfiltration.

</td>
  </tr>
</table>

<br />

## Status

Early development, **Windows-first**. The terminal core, pane/split/tab UI, browser
pane, scriptable automation, workspace sidebar, command palette, notifications, SSH
transport, browser import, reliability layer, and the `weft` CLI are implemented and
unit-tested. Live-desktop integration, real-engine automation parity, and packaging are
in progress.

<br />

## Getting started

```bash
npm install
npm run build:inject     # build the page-context inject bundle (required before cargo build/test)
npm run tauri dev        # run the app
```

#### Tests

```bash
npm run check                  # type-check the Svelte/TS app
npm test                       # TS unit tests (vitest)
cd src-tauri && cargo test     # Rust core tests
```

<br />

## License

<table>
  <tr>
    <td valign="top"><strong>GPL-3.0-or-later</strong></td>
    <td>

weftgrid is free software under the **GNU General Public License v3.0 or later** — see
[`LICENSE`](LICENSE).

It is a cross-platform rewrite based on
[cmux](https://github.com/manaflow-ai/cmux) (GPL-3.0-or-later, © Manaflow Inc.), and its
browser-automation algorithm is ported from
[agent-browser](https://github.com/vercel-labs/agent-browser) (Apache-2.0). Full
attributions are in [`NOTICE`](NOTICE).

</td>
  </tr>
</table>

<div align="center">
<br />
<sub>Built with Tauri 2 · Rust · TypeScript · Svelte 5</sub>
</div>
