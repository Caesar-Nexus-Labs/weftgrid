// Custom command runner (P16) — the trust-gated execution path for weft.json
// `command` / `workspace` entries. SECURITY-CRITICAL.
//
// Trust boundary (P12 owns the store; we CALL it, never reimplement):
//   - `confirm:true` on a command → ALWAYS prompt (every run, any origin); it is
//     an explicit per-command "always ask me" opt-in and is never persisted.
//   - GLOBAL config (user's ~/.config/weftgrid) → otherwise runs unconfirmed.
//   - PROJECT-LOCAL config (a repo's weft.json) → `weft_trust_check` decides; if
//     it needs confirm we MUST prompt and only run on "Trust and Run" (which
//     persists via `weft_trust_grant`). A project-local shell command is NEVER
//     auto-run.
//
// Execution seams (host injects):
//   - shell `command` → `sendToTerminal` (P3 `pty_write` path — define the seam;
//     actual pane wiring is integration work in P3/P4).
//   - `workspace` builder → `buildWorkspace` (P12 WorkspaceStore — integration).

import type { WeftCommand } from "../settings/settings-store";

/** Where a command's defining weft.json lives (decides the trust gate). */
export type CommandOrigin = "global" | "project-local";

/** The P12 trust API surface this runner depends on (inject the real client). */
export interface TrustGate {
  /** True when the command must be confirmed before running. */
  check(
    content: string,
    commandName: string,
    projectLocal: boolean,
    sourcePath: string,
  ): Promise<boolean>;
  /** Persist "Trust and Run" so future runs skip the prompt. */
  grant(content: string, commandName: string, sourcePath: string): Promise<boolean>;
}

/** Host-provided confirm dialog. Resolves to the user's choice. */
export type ConfirmFn = (request: ConfirmRequest) => Promise<ConfirmChoice>;

export interface ConfirmRequest {
  commandName: string;
  /** The shell text (when a shell command) shown so the user sees what runs. */
  commandText?: string;
  sourcePath: string;
}

/** "trust" persists + runs; "run-once" runs without persisting; "cancel" aborts. */
export type ConfirmChoice = "trust" | "run-once" | "cancel";

/** Side-effect seams the runner drives once trust is satisfied. */
export interface RunSeams {
  /** P3 seam: send shell text to the active terminal (pty_write). */
  sendToTerminal(commandText: string): void | Promise<void>;
  /** P12 seam: build/select a workspace from a weft.json `workspace` spec. */
  buildWorkspace(spec: NonNullable<WeftCommand["workspace"]>): void | Promise<void>;
}

/** Context for one custom-command run: the source config + its origin. */
export interface CustomRunContext {
  /** Raw weft.json text (passed to the trust API so fingerprints match). */
  content: string;
  origin: CommandOrigin;
  sourcePath: string;
}

/** Outcome of a run attempt — lets the overlay decide whether to dismiss. */
export type RunOutcome = "ran" | "cancelled";

/**
 * Run a custom command through the trust gate. Returns "cancelled" when the user
 * declines a required confirm (overlay should stay open), "ran" otherwise.
 */
export async function runCustomCommand(
  cmd: WeftCommand,
  ctx: CustomRunContext,
  gate: TrustGate,
  confirm: ConfirmFn,
  seams: RunSeams,
): Promise<RunOutcome> {
  const projectLocal = ctx.origin === "project-local";
  // `confirm:true` is an explicit per-command "always ask me" opt-in that wins
  // over origin trust — even a global command must prompt every run. Otherwise
  // global is implicitly trusted and only project-local consults P12's gate.
  const needsConfirm =
    cmd.confirm === true ||
    (projectLocal &&
      (await gate.check(ctx.content, cmd.name, true, ctx.sourcePath)));

  if (needsConfirm) {
    const choice = await confirm({
      commandName: cmd.name,
      commandText: cmd.command,
      sourcePath: ctx.sourcePath,
    });
    if (choice === "cancel") {
      return "cancelled";
    }
    // "Trust and Run" persists the fingerprint; "run-once" skips persistence.
    // `confirm:true` commands are intentionally NOT persisted as trusted — the
    // gate forces a prompt every time regardless, so granting would be useless.
    if (choice === "trust" && cmd.confirm !== true) {
      await gate.grant(ctx.content, cmd.name, ctx.sourcePath);
    }
  }

  await execute(cmd, seams);
  return "ran";
}

/** Dispatch to the correct seam (shell XOR workspace — validated by P12 parse). */
async function execute(cmd: WeftCommand, seams: RunSeams): Promise<void> {
  if (cmd.command) {
    await seams.sendToTerminal(cmd.command);
  } else if (cmd.workspace) {
    await seams.buildWorkspace(cmd.workspace);
  }
}
