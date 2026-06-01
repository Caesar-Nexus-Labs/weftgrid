// settings-store client round-trip tests (P12). Mocks the Tauri `invoke`
// boundary and asserts each wrapper sends the right command name + args and
// returns the typed payload — verifying the TS↔Rust IPC contract shape.

import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock the Tauri invoke boundary. Each test sets the resolved value + captures
// the (cmd, args) the wrapper sent.
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invokeMock(cmd, args),
}));

import {
  configGet,
  configSet,
  workspaceAdd,
  workspaceReorder,
  weftTrustCheck,
  weftTrustGrant,
  keybindingSet,
  keybindingResolve,
  type Config,
} from "./settings-store";

function sampleConfig(): Config {
  return {
    schema_version: 2,
    tab_layout: "vertical",
    theme: { ui: "dark", terminal_colors: "solarized" },
    default_shell: "/bin/zsh",
    default_respawn_policy: "fresh-shell",
    import_consent: true,
    keybinding_overrides: { "palette.commands": "ctrl+shift+k" },
  };
}

describe("settings-store client", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("config_get returns the typed config unchanged (round-trip)", async () => {
    const cfg = sampleConfig();
    invokeMock.mockResolvedValueOnce(cfg);
    const result = await configGet();
    expect(invokeMock).toHaveBeenCalledWith("config_get", undefined);
    expect(result).toEqual(cfg);
  });

  it("config_set forwards the config payload", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    const cfg = sampleConfig();
    await configSet(cfg);
    expect(invokeMock).toHaveBeenCalledWith("config_set", { config: cfg });
  });

  it("workspace_add passes title + cwd and returns the id", async () => {
    invokeMock.mockResolvedValueOnce("ws-uuid");
    const id = await workspaceAdd("proj", "/proj");
    expect(invokeMock).toHaveBeenCalledWith("workspace_add", {
      title: "proj",
      cwd: "/proj",
    });
    expect(id).toBe("ws-uuid");
  });

  it("workspace_reorder forwards from/to indices", async () => {
    invokeMock.mockResolvedValueOnce(true);
    await workspaceReorder(0, 2);
    expect(invokeMock).toHaveBeenCalledWith("workspace_reorder", {
      from: 0,
      to: 2,
    });
  });

  it("weft_trust_check maps confirm gate and passes project-local flag", async () => {
    invokeMock.mockResolvedValueOnce(true);
    const needsConfirm = await weftTrustCheck(
      '{"commands":[]}',
      "Deploy",
      true,
      "/proj/weft.json",
    );
    expect(invokeMock).toHaveBeenCalledWith("weft_trust_check", {
      content: '{"commands":[]}',
      commandName: "Deploy",
      projectLocal: true,
      sourcePath: "/proj/weft.json",
    });
    expect(needsConfirm).toBe(true);
  });

  it("weft_trust_grant forwards command + source path", async () => {
    invokeMock.mockResolvedValueOnce(true);
    await weftTrustGrant("{}", "Deploy", "/proj/weft.json");
    expect(invokeMock).toHaveBeenCalledWith("weft_trust_grant", {
      content: "{}",
      commandName: "Deploy",
      sourcePath: "/proj/weft.json",
    });
  });

  it("keybinding_set returns the conflict list", async () => {
    invokeMock.mockResolvedValueOnce({
      conflicts: [
        { chord: "ctrl+shift+p", action_a: "palette.commands", action_b: "palette.switcher" },
      ],
    });
    const res = await keybindingSet("palette.switcher", "ctrl+shift+p");
    expect(invokeMock).toHaveBeenCalledWith("keybinding_set", {
      action: "palette.switcher",
      chord: "ctrl+shift+p",
    });
    expect(res.conflicts).toHaveLength(1);
    expect(res.conflicts[0].chord).toBe("ctrl+shift+p");
  });

  it("keybinding_resolve returns the chord or null", async () => {
    invokeMock.mockResolvedValueOnce("ctrl+alt+left");
    const chord = await keybindingResolve("focus.left");
    expect(invokeMock).toHaveBeenCalledWith("keybinding_resolve", {
      action: "focus.left",
    });
    expect(chord).toBe("ctrl+alt+left");
  });
});
