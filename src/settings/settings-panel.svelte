<script lang="ts">
  // Settings panel (P12) — tabbed editor over the typed config: General (tab
  // layout, default shell, respawn policy), Theme (UI + terminal colors),
  // Keybindings (delegated editor), Privacy (browser-import consent). Loads the
  // config on mount and persists each change through settings-store (atomic write
  // in Rust). Svelte 5 runes.
  import { onMount } from "svelte";
  import KeybindingEditor from "./keybinding-editor.svelte";
  import {
    configGet,
    configSet,
    type Config,
    type TabLayout,
    type RespawnPolicy,
  } from "./settings-store";

  type Tab = "general" | "theme" | "keybindings" | "privacy";

  let config = $state<Config | null>(null);
  let activeTab = $state<Tab>("general");
  let error = $state<string | null>(null);
  let saving = $state(false);

  const tabs: Tab[] = ["general", "theme", "keybindings", "privacy"];

  onMount(async () => {
    try {
      config = await configGet();
    } catch (e) {
      error = String(e);
    }
  });

  // Persist the whole config after any field edit. Config writes are infrequent
  // so a full save (atomic temp+rename in Rust) is fine and keeps logic simple.
  async function save() {
    if (!config) return;
    saving = true;
    error = null;
    try {
      await configSet($state.snapshot(config));
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function setTabLayout(value: TabLayout) {
    if (config) {
      config.tab_layout = value;
      save();
    }
  }

  function setRespawn(value: RespawnPolicy) {
    if (config) {
      config.default_respawn_policy = value;
      save();
    }
  }
</script>

<div class="settings-panel">
  <div class="tabs" role="tablist">
    {#each tabs as tab (tab)}
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === tab}
        class:active={activeTab === tab}
        onclick={() => (activeTab = tab)}
      >
        {tab}
      </button>
    {/each}
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if !config}
    <p>Loading settings…</p>
  {:else if activeTab === "general"}
    <section>
      <label>
        Tab layout
        <select
          value={config.tab_layout}
          onchange={(e) => setTabLayout(e.currentTarget.value as TabLayout)}
        >
          <option value="horizontal">Horizontal</option>
          <option value="vertical">Vertical</option>
        </select>
      </label>
      <label>
        Default shell
        <input
          type="text"
          placeholder="(OS default)"
          bind:value={config.default_shell}
          onblur={save}
        />
      </label>
      <label>
        Session restore policy
        <select
          value={config.default_respawn_policy}
          onchange={(e) => setRespawn(e.currentTarget.value as RespawnPolicy)}
        >
          <option value="fresh-shell">Fresh shell (safe)</option>
          <option value="rerun-last-command">Re-run last command</option>
        </select>
      </label>
      {#if config.default_respawn_policy === "rerun-last-command"}
        <p class="warn">
          Re-running the last command on restore can execute destructive commands.
        </p>
      {/if}
    </section>
  {:else if activeTab === "theme"}
    <section>
      <label>
        UI theme
        <input type="text" bind:value={config.theme.ui} onblur={save} />
      </label>
      <label>
        Terminal colors
        <input
          type="text"
          bind:value={config.theme.terminal_colors}
          onblur={save}
        />
      </label>
    </section>
  {:else if activeTab === "keybindings"}
    <KeybindingEditor />
  {:else if activeTab === "privacy"}
    <section>
      <label class="checkbox">
        <input
          type="checkbox"
          bind:checked={config.import_consent}
          onchange={save}
        />
        Allow importing data from other browsers
      </label>
      <p class="hint">
        Consent is stored as a flag only. Credentials are never kept in settings.
      </p>
    </section>
  {/if}

  {#if saving}<span class="saving">Saving…</span>{/if}
</div>

<style>
  .settings-panel {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .tabs {
    display: flex;
    gap: 0.25rem;
    border-bottom: 1px solid #ddd;
  }
  .tabs button {
    text-transform: capitalize;
    background: transparent;
    border: none;
    padding: 0.5rem 0.75rem;
    cursor: pointer;
  }
  .tabs button.active {
    border-bottom: 2px solid #396cd8;
    font-weight: 600;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  label.checkbox {
    flex-direction: row;
    align-items: center;
    gap: 0.5rem;
  }
  .warn {
    color: #b06a00;
    font-size: 0.85em;
  }
  .error {
    color: #b00020;
  }
  .hint {
    color: #666;
    font-size: 0.85em;
  }
</style>
