<script lang="ts">
  import { Plus, Search, X } from "@lucide/svelte";
  import { connections } from "../../lib/state/connections.svelte";
  import { dialogs } from "../../lib/state/ui.svelte";
  import type { ConnectionProfile } from "../../lib/types";
  import ConnectionNode from "./ConnectionNode.svelte";

  let filter = $state("");
  let filterInput = $state<HTMLInputElement>();

  export function focusFilter() {
    filterInput?.focus();
  }

  // Group connections by their optional group name; ungrouped first.
  const groups = $derived.by(() => {
    const map = new Map<string, ConnectionProfile[]>();
    for (const p of connections.profiles) {
      const g = p.group?.trim() ?? "";
      map.set(g, [...(map.get(g) ?? []), p]);
    }
    return [...map.entries()].sort(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : a.localeCompare(b)));
  });
</script>

<aside class="sidebar" aria-label="Connections">
  <div class="filter">
    <Search size={13} />
    <input
      bind:this={filterInput}
      class="field"
      placeholder="Filter tables"
      bind:value={filter}
      onkeydown={(e) => e.key === "Escape" && (filter = "")}
      aria-label="Filter tables in the tree"
    />
    {#if filter}
      <button class="btn ghost icon small" aria-label="Clear filter" onclick={() => (filter = "")}><X size={12} /></button>
    {/if}
  </div>

  <div class="tree scroll" role="tree">
    {#if connections.loaded && connections.profiles.length === 0}
      <div class="empty">
        <p>Add a connection to start browsing your databases.</p>
        <button class="btn primary small" onclick={() => dialogs.editConnection(null)}><Plus size={13} /> New connection</button>
      </div>
    {/if}
    {#each groups as [group, profiles] (group)}
      {#if group}<div class="group">{group}</div>{/if}
      {#each profiles as profile (profile.id)}
        <ConnectionNode {profile} {filter} />
      {/each}
    {/each}
  </div>

  <div class="foot">
    <button class="btn ghost small" onclick={() => dialogs.editConnection(null)}><Plus size={14} /> New connection</button>
  </div>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--bg);
  }
  .filter {
    position: relative;
    display: flex;
    align-items: center;
    padding: 8px 8px 6px;
  }
  .filter :global(svg:first-child) {
    position: absolute;
    left: 17px;
    color: var(--faint);
    pointer-events: none;
  }
  .filter .field {
    width: 100%;
    height: 26px;
    padding-left: 26px;
    background: var(--surface);
  }
  .filter .btn {
    position: absolute;
    right: 11px;
  }
  .tree {
    flex: 1;
    min-height: 0;
    padding: 2px 6px 12px;
  }
  .group {
    margin: 12px 8px 4px;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--faint);
  }
  .empty {
    padding: 18px 10px;
    color: var(--muted);
  }
  .empty p {
    margin: 0 0 12px;
  }
  .foot {
    padding: 6px 8px;
    border-top: 1px solid var(--rule);
  }
  .foot .btn {
    width: 100%;
    justify-content: flex-start;
    color: var(--ink-2);
  }
</style>
