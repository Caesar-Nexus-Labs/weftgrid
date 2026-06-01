// Metadata badges (P15b) — render enrichment badges for a workspace row, but
// ONLY for fields that are present on the snapshot.
//
// The enrichment fields (git branch, PRs, listening ports, status/progress,
// latest notification, latest conversation message) are filled by producers
// wired in Wave-3: most are PUSHED from the `weft` CLI / shell-integration /
// agent over the P13 socket; two are app-driven scans (ports, git poll) behind
// default-off toggles. Until a field arrives it is undefined/empty on the DTO, so
// a badge for it is simply NOT rendered. This keeps the basic sidebar (P15a) free
// of any metadata cost and makes "show only when data exists" a structural
// property, not a runtime check scattered across the row builder.
//
// Pure value-in / DOM-out (snapshot boundary): reads the immutable snapshot,
// holds no reference to it. A receiver-driven change re-renders via
// `updateMetadataBadges` with a fresh snapshot value (never a live store ref).

import type { WorkspaceSnapshot } from "$lib/model";

/**
 * Build the badge container for a workspace snapshot. Returns an element holding
 * one child per PRESENT metadata field (none → an empty container). The host
 * appends it to the row; rebuilt from a fresh snapshot on change.
 */
export function buildMetadataBadges(snapshot: Readonly<WorkspaceSnapshot>): HTMLElement {
  const container = document.createElement("div");
  container.className = "sidebar-badges";
  container.setAttribute("role", "group");

  if (snapshot.gitBranchSummary) {
    container.appendChild(
      badge("git", `⎇ ${snapshot.gitBranchSummary}`, snapshot.gitBranchSummary),
    );
  }

  if (snapshot.pullRequestRows.length > 0) {
    container.appendChild(
      badge("pr", `PR ${snapshot.pullRequestRows.length}`, snapshot.pullRequestRows.join("\n")),
    );
  }

  if (snapshot.listeningPorts.length > 0) {
    const ports = snapshot.listeningPorts.join(", ");
    container.appendChild(badge("ports", `:${snapshot.listeningPorts.join(" :")}`, ports));
  }

  if (snapshot.progress !== undefined) {
    const pct = Math.round(clampUnit(snapshot.progress) * 100);
    const el = badge("progress", `${pct}%`, `progress ${pct}%`);
    el.dataset.progress = String(pct);
    container.appendChild(el);
  }

  if (snapshot.remoteConnectionStatusText) {
    container.appendChild(
      badge("status", snapshot.remoteConnectionStatusText, snapshot.remoteConnectionStatusText),
    );
  }

  if (snapshot.latestNotificationText) {
    container.appendChild(
      badge("notif", snapshot.latestNotificationText, snapshot.latestNotificationText),
    );
  }

  if (snapshot.latestConversationMessage) {
    container.appendChild(
      badge("conversation", snapshot.latestConversationMessage, snapshot.latestConversationMessage),
    );
  }

  return container;
}

/**
 * Re-render a host's badges from a FRESH snapshot value. The receiver-driven
 * update path (P13 push → P12 store → new snapshot) calls this with the rebuilt
 * snapshot: it discards the old badge container and appends a new one built from
 * the immutable value. No reactive store ref is held (snapshot boundary) — the
 * host owns deciding WHEN a new snapshot arrived; this only maps value → DOM.
 *
 * Returns the new badge container (also appended to `host`).
 */
export function updateMetadataBadges(
  host: HTMLElement,
  snapshot: Readonly<WorkspaceSnapshot>,
): HTMLElement {
  host.querySelector(":scope > .sidebar-badges")?.remove();
  const badges = buildMetadataBadges(snapshot);
  host.appendChild(badges);
  return badges;
}

/** One labelled badge element keyed by `kind` (so the host can style/test it). */
function badge(kind: string, text: string, title: string): HTMLElement {
  const el = document.createElement("span");
  el.className = `sidebar-badge sidebar-badge--${kind}`;
  el.dataset.badge = kind;
  el.textContent = text;
  el.title = title;
  return el;
}

/** Clamp a progress fraction into [0,1] (producers may send slight overshoot). */
function clampUnit(value: number): number {
  return Math.min(1, Math.max(0, value));
}
