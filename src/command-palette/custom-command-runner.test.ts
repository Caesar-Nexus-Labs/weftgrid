// Trust-gate tests for the custom-command runner (P16) — SECURITY-CRITICAL.
//
// Verifies: a project-local shell command requires confirm (mocked P12
// `weft_trust_check`) before running; a global command runs direct (no prompt);
// "Trust and Run" persists via `weft_trust_grant`; "cancel" aborts the run; a
// `confirm:true` command is never persisted as trusted; a workspace command
// drives the workspace seam, a shell command the terminal seam.

import { describe, it, expect, vi } from "vitest";
import {
  runCustomCommand,
  type TrustGate,
  type ConfirmFn,
  type ConfirmChoice,
  type CustomRunContext,
} from "./custom-command-runner";
import type { WeftCommand } from "../settings/settings-store";

function gate(needsConfirm: boolean): TrustGate & {
  check: ReturnType<typeof vi.fn>;
  grant: ReturnType<typeof vi.fn>;
} {
  return {
    check: vi.fn(async () => needsConfirm),
    grant: vi.fn(async () => true),
  };
}

function seams(): {
  sendToTerminal: ReturnType<typeof vi.fn<(commandText: string) => void>>;
  buildWorkspace: ReturnType<typeof vi.fn<(spec: NonNullable<WeftCommand["workspace"]>) => void>>;
} {
  return {
    sendToTerminal: vi.fn<(commandText: string) => void>(),
    buildWorkspace: vi.fn<(spec: NonNullable<WeftCommand["workspace"]>) => void>(),
  };
}

const projectCtx: CustomRunContext = {
  content: '{"commands":[{"name":"Deploy","command":"./deploy.sh"}]}',
  origin: "project-local",
  sourcePath: "/proj/weft.json",
};
const globalCtx: CustomRunContext = {
  content: '{"commands":[{"name":"Deploy","command":"./deploy.sh"}]}',
  origin: "global",
  sourcePath: "/home/u/.config/weftgrid/weft.json",
};

const deploy: WeftCommand = { name: "Deploy", keywords: [], command: "./deploy.sh" };

describe("runCustomCommand trust gate", () => {
  it("project-local shell command requires confirm before running", async () => {
    const g = gate(true);
    const s = seams();
    const confirm: ConfirmFn = vi.fn(async (): Promise<ConfirmChoice> => "run-once");

    const outcome = await runCustomCommand(deploy, projectCtx, g, confirm, s);

    expect(g.check).toHaveBeenCalledWith(projectCtx.content, "Deploy", true, projectCtx.sourcePath);
    expect(confirm).toHaveBeenCalled();
    expect(s.sendToTerminal).toHaveBeenCalledWith("./deploy.sh");
    expect(outcome).toBe("ran");
  });

  it("never auto-runs a project-local shell command when the user cancels", async () => {
    const g = gate(true);
    const s = seams();
    const confirm: ConfirmFn = vi.fn(async (): Promise<ConfirmChoice> => "cancel");

    const outcome = await runCustomCommand(deploy, projectCtx, g, confirm, s);

    expect(outcome).toBe("cancelled");
    expect(s.sendToTerminal).not.toHaveBeenCalled();
    expect(g.grant).not.toHaveBeenCalled();
  });

  it("global command runs direct with no confirm prompt", async () => {
    const g = gate(false);
    const s = seams();
    const confirm: ConfirmFn = vi.fn(async (): Promise<ConfirmChoice> => "cancel");

    const outcome = await runCustomCommand(deploy, globalCtx, g, confirm, s);

    // Global origin is implicitly trusted — the gate is never even consulted.
    expect(g.check).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(s.sendToTerminal).toHaveBeenCalledWith("./deploy.sh");
    expect(outcome).toBe("ran");
  });

  it("'Trust and Run' persists trust via weft_trust_grant", async () => {
    const g = gate(true);
    const s = seams();
    const confirm: ConfirmFn = vi.fn(async (): Promise<ConfirmChoice> => "trust");

    await runCustomCommand(deploy, projectCtx, g, confirm, s);

    expect(g.grant).toHaveBeenCalledWith(projectCtx.content, "Deploy", projectCtx.sourcePath);
    expect(s.sendToTerminal).toHaveBeenCalledWith("./deploy.sh");
  });

  it("does not persist trust for a confirm:true command (prompt forced every time)", async () => {
    const g = gate(true);
    const s = seams();
    const confirm: ConfirmFn = vi.fn(async (): Promise<ConfirmChoice> => "trust");
    const forced: WeftCommand = { ...deploy, confirm: true };

    await runCustomCommand(forced, projectCtx, g, confirm, s);

    expect(g.grant).not.toHaveBeenCalled();
    expect(s.sendToTerminal).toHaveBeenCalledWith("./deploy.sh");
  });

  it("global command with confirm:true still prompts every run", async () => {
    // `confirm:true` is an explicit opt-in that wins over global trust; the gate
    // is not consulted (origin is global) but the confirm dialog must appear.
    const g = gate(false);
    const s = seams();
    const confirm: ConfirmFn = vi.fn(async (): Promise<ConfirmChoice> => "run-once");
    const forced: WeftCommand = { ...deploy, confirm: true };

    const outcome = await runCustomCommand(forced, globalCtx, g, confirm, s);

    expect(g.check).not.toHaveBeenCalled();
    expect(confirm).toHaveBeenCalled();
    expect(g.grant).not.toHaveBeenCalled();
    expect(s.sendToTerminal).toHaveBeenCalledWith("./deploy.sh");
    expect(outcome).toBe("ran");
  });

  it("routes a workspace command to the workspace seam", async () => {
    const g = gate(false);
    const s = seams();
    const confirm: ConfirmFn = vi.fn();
    const wsCmd: WeftCommand = {
      name: "Scratch",
      keywords: [],
      workspace: { name: "scratch", cwd: "/tmp" },
    };

    await runCustomCommand(wsCmd, globalCtx, g, confirm, s);

    expect(s.buildWorkspace).toHaveBeenCalledWith({ name: "scratch", cwd: "/tmp" });
    expect(s.sendToTerminal).not.toHaveBeenCalled();
  });
});
