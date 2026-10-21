<script lang="ts">
  import { Lock, Plus } from "@lucide/svelte";
  import { newQueryFor, openConnection } from "../../lib/actions";
  import { connections, DRIVERS } from "../../lib/state/connections.svelte";
  import { dialogs } from "../../lib/state/ui.svelte";
  import EngineMark from "../ui/EngineMark.svelte";

  const shortcuts: [string, string][] = [
    ["⌘ K", "Jump to any table or command"],
    ["⌘ T", "New query"],
    ["⌘ ↵", "Run the selection, or the whole script"],
    ["⇧ ⌘ F", "Find in database"],
    ["⌘ S", "Review and save grid edits"],
    ["⌘ W", "Close tab"],
  ];

  async function start(id: string) {
    if (await openConnection(id)) newQueryFor(id);
  }
</script>

<div class="welcome scroll">
  <div class="inner">
    <h1>dbKu</h1>
    <p class="lead">Open a connection to browse tables, edit rows and run SQL.</p>

    <section aria-label="Connections">
      {#if connections.profiles.length === 0}
        <button class="btn primary" onclick={() => dialogs.editConnection(null)}><Plus size={14} /> Add your first connection</button>
        <p class="hint">PostgreSQL, MySQL / MariaDB, SQLite and SQL Server are supported.</p>
      {:else}
        <ul>
          {#each connections.profiles.slice(0, 8) as p (p.id)}
            <li>
              <button onclick={() => start(p.id)}>
                <EngineMark driver={p.driver} color={p.color} size={22} />
                <span class="name">{p.name}</span>
                {#if p.readOnly}<Lock size={12} class="ro" />{/if}
                <span class="where">{p.driver === "sqlite" ? p.database.split(/[\\/]/).pop() : `${p.host}${p.database ? ` / ${p.database}` : ""}`}</span>
                <span class="engine">{DRIVERS[p.driver].label}</span>
              </button>
            </li>
          {/each}
        </ul>
        <button class="btn ghost small add" onclick={() => dialogs.editConnection(null)}><Plus size={13} /> New connection</button>
      {/if}
    </section>

    <dl class="keys">
      {#each shortcuts as [k, label] (k)}
        <dt><kbd>{k}</kbd></dt>
        <dd>{label}</dd>
      {/each}
    </dl>
  </div>
</div>

<style>
  .welcome {
    height: 100%;
    background: var(--surface);
  }
  .inner {
    max-width: 640px;
    margin: 0 auto;
    padding: 9vh 32px 48px;
  }
  h1 {
    margin: 0;
    font-size: 44px;
    font-weight: 750;
    letter-spacing: -0.045em;
    line-height: 1;
  }
  .lead {
    margin: 12px 0 34px;
    font-size: var(--text-md);
    color: var(--muted);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--rule);
  }
  li button {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    height: 46px;
    padding: 0 10px;
    border: 0;
    border-bottom: 1px solid var(--rule);
    background: none;
    text-align: left;
  }
  li button:hover {
    background: var(--row-hover);
  }
  .name {
    font-weight: 600;
  }
  li :global(.ro) {
    color: var(--readonly);
  }
  .where {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }
  .engine {
    font-size: var(--text-sm);
    color: var(--faint);
  }
  .add {
    margin-top: 8px;
  }
  .hint {
    color: var(--muted);
  }
  .keys {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 8px 16px;
    margin: 44px 0 0;
    color: var(--ink-2);
  }
  dt {
    text-align: right;
  }
  dd {
    margin: 0;
  }
</style>
