<script lang="ts">
  import { Eye, Plug, Plus, Search, Settings, SquareTerminal, SunMoon, Table2 } from "@lucide/svelte";
  import type { Component } from "svelte";
  import { newQueryFor, openConnection } from "../../lib/actions";
  import { connections } from "../../lib/state/connections.svelte";
  import { schema } from "../../lib/state/schema.svelte";
  import { isDarkNow, settings } from "../../lib/state/settings.svelte";
  import { tabs } from "../../lib/state/tabs.svelte";
  import { dialogs } from "../../lib/state/ui.svelte";

  interface Entry {
    id: string;
    label: string;
    detail?: string;
    icon: Component;
    run: () => void;
  }

  let term = $state("");
  let index = $state(0);
  let list = $state<HTMLUListElement>();

  const activeConn = $derived(tabs.active?.connId ?? connections.connected[0]?.id ?? null);

  const actions = $derived<Entry[]>([
    { id: "a:query", label: "New query", detail: "⌘T", icon: SquareTerminal, run: () => newQueryFor(activeConn) },
    { id: "a:search", label: "Find in database", detail: "⇧⌘F", icon: Search, run: () => tabs.openSearch(activeConn, null, null) },
    { id: "a:conn", label: "New connection", icon: Plus, run: () => dialogs.editConnection(null) },
    {
      id: "a:theme",
      label: isDarkNow() ? "Switch to light theme" : "Switch to dark theme",
      icon: SunMoon,
      run: () => settings.set("theme", isDarkNow() ? "light" : "dark"),
    },
    { id: "a:settings", label: "Settings", detail: "⌘,", icon: Settings, run: () => (dialogs.settings = true) },
    ...connections.profiles
      .filter((p) => !connections.isConnected(p.id))
      .map((p) => ({ id: `c:${p.id}`, label: `Connect to ${p.name}`, icon: Plug, run: () => void openConnection(p.id) })),
  ]);

  const objects = $derived<Entry[]>(
    schema.allObjects().map(({ id, database, schema: sc, object }) => {
      const conn = connections.byId(id);
      return {
        id: `o:${id}:${database}:${sc}:${object.name}`,
        label: object.name,
        detail: [conn?.name, database, sc].filter(Boolean).join(" / "),
        icon: object.kind === "table" ? Table2 : Eye,
        run: () => tabs.openTable(id, { database, schema: sc, table: object.name }, object.kind),
      };
    }),
  );

  /** Subsequence match with a bonus for contiguous and early hits. */
  function score(text: string, q: string): number {
    const t = text.toLowerCase();
    const direct = t.indexOf(q);
    if (direct >= 0) return 1000 - direct - (t.length - q.length) * 0.1;
    let at = 0;
    let s = 0;
    for (const ch of q) {
      const found = t.indexOf(ch, at);
      if (found < 0) return -1;
      s += found === at ? 3 : 1;
      at = found + 1;
    }
    return s;
  }

  const results = $derived.by(() => {
    const q = term.trim().toLowerCase();
    if (!q) return [...actions, ...objects.slice(0, 40)];
    return [...actions, ...objects]
      .map((e) => ({ e, s: Math.max(score(e.label, q), score(`${e.detail ?? ""} ${e.label}`, q) - 50) }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 60)
      .map((x) => x.e);
  });

  $effect(() => {
    void term;
    index = 0;
  });

  $effect(() => {
    list?.querySelector(`[data-i="${index}"]`)?.scrollIntoView({ block: "nearest" });
  });

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
  }

  function choose(e: Entry | undefined) {
    if (!e) return;
    dialogs.palette = false;
    e.run();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      index = Math.min(results.length - 1, index + 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      index = Math.max(0, index - 1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(results[index]);
    } else if (e.key === "Escape") {
      dialogs.palette = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && (dialogs.palette = false)}>
  <div class="palette" role="dialog" aria-label="Go to">
    <div class="input">
      <Search size={16} />
      <input
        bind:value={term}
        {onkeydown}
        use:focusOnMount
        placeholder="Jump to a table, or run a command"
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        aria-activedescendant="palette-{index}"
      />
    </div>
    <ul id="palette-list" role="listbox" bind:this={list} class="scroll">
      {#each results as r, i (r.id)}
        <li id="palette-{i}" role="option" aria-selected={i === index} data-i={i}>
          <button class:on={i === index} onmousemove={() => (index = i)} onclick={() => choose(r)}>
            <r.icon size={14} />
            <span class="label">{r.label}</span>
            {#if r.detail}<span class="detail">{r.detail}</span>{/if}
          </button>
        </li>
      {:else}
        <li class="none">Nothing matches. Tables appear here once their connection has been opened.</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
    background: color-mix(in srgb, var(--ink) 16%, transparent);
  }
  .palette {
    width: min(620px, calc(100vw - 32px));
    background: var(--raised);
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius-l);
    box-shadow: var(--shadow-modal);
    overflow: hidden;
    animation: drop var(--duration) var(--ease-out);
  }
  .input {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 16px;
    border-bottom: 1px solid var(--rule);
    color: var(--muted);
  }
  .input input {
    flex: 1;
    height: 50px;
    border: 0;
    outline: none;
    background: transparent;
    font-size: var(--text-lg);
    color: var(--ink);
  }
  ul {
    max-height: 380px;
    margin: 0;
    padding: 6px;
    list-style: none;
  }
  button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-s);
    background: none;
    color: var(--ink-2);
    text-align: left;
  }
  button.on {
    background: var(--accent);
    color: var(--accent-ink);
  }
  .label {
    font-weight: 500;
    white-space: nowrap;
  }
  .detail {
    margin-left: auto;
    font-size: var(--text-sm);
    opacity: 0.65;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .none {
    padding: 14px 10px;
    color: var(--muted);
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-6px);
    }
  }
</style>
