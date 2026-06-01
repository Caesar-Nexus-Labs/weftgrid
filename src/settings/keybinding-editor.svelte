<script lang="ts">
  // Keybinding editor (P12) — lists action→chord bindings and lets the user
  // rebind. Setting a chord that collides surfaces the conflict inline (the Rust
  // registry returns conflicts on set). Reads/writes go through settings-store.
  import { onMount } from "svelte";
  import {
    keybindingList,
    keybindingSet,
    type KeybindingRow,
    type Conflict,
  } from "./settings-store";

  let rows = $state<KeybindingRow[]>([]);
  let conflicts = $state<Conflict[]>([]);
  let editing = $state<string | null>(null);
  let draft = $state("");
  let error = $state<string | null>(null);

  onMount(refresh);

  async function refresh() {
    try {
      rows = await keybindingList();
    } catch (e) {
      error = String(e);
    }
  }

  function startEdit(row: KeybindingRow) {
    editing = row.action;
    draft = row.chord;
    error = null;
  }

  async function commit(action: string) {
    try {
      const result = await keybindingSet(action, draft.trim());
      conflicts = result.conflicts;
      editing = null;
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  function conflictsFor(action: string): Conflict[] {
    return conflicts.filter(
      (c) => c.action_a === action || c.action_b === action,
    );
  }
</script>

<section class="keybindings">
  <h3>Keybindings</h3>
  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
  <ul>
    {#each rows as row (row.action)}
      <li>
        <span class="action">{row.action}</span>
        {#if editing === row.action}
          <input
            class="chord-input"
            bind:value={draft}
            onkeydown={(e) => {
              if (e.key === "Enter") commit(row.action);
              if (e.key === "Escape") editing = null;
            }}
            aria-label={`Chord for ${row.action}`}
          />
          <button type="button" onclick={() => commit(row.action)}>Save</button>
        {:else}
          <button
            type="button"
            class="chord"
            onclick={() => startEdit(row)}
            aria-label={`Edit chord for ${row.action}, currently ${row.chord}`}
          >
            {row.chord}
          </button>
        {/if}
        {#each conflictsFor(row.action) as c (c.chord + c.action_a + c.action_b)}
          <span class="conflict" role="alert">
            conflicts with {c.action_a === row.action ? c.action_b : c.action_a}
          </span>
        {/each}
      </li>
    {/each}
  </ul>
</section>

<style>
  .keybindings ul {
    list-style: none;
    padding: 0;
    margin: 0;
  }
  .keybindings li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0;
  }
  .action {
    flex: 1;
    font-family: monospace;
  }
  .chord {
    font-family: monospace;
    border: 1px solid #ccc;
    border-radius: 4px;
    padding: 0.1rem 0.4rem;
    background: transparent;
    cursor: pointer;
  }
  .conflict {
    color: #b00020;
    font-size: 0.85em;
  }
  .error {
    color: #b00020;
  }
</style>
