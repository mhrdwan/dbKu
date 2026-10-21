<script lang="ts">
  import { Eye, Plus, Search, SquareTerminal, Table2, X } from "@lucide/svelte";
  import { newQueryFor, requestCloseOtherTabs, requestCloseTab } from "../../lib/actions";
  import { connections } from "../../lib/state/connections.svelte";
  import { tabs, type Tab } from "../../lib/state/tabs.svelte";
  import { menu } from "../../lib/state/ui.svelte";

  function iconFor(t: Tab) {
    if (t.kind === "query") return SquareTerminal;
    if (t.kind === "search") return Search;
    return t.objectKind === "table" ? Table2 : Eye;
  }

  function tabMenu(e: MouseEvent, t: Tab) {
    menu.show(e, [
      { label: "Close", shortcut: "⌘W", action: () => void requestCloseTab(t.id) },
      { label: "Close others", disabled: tabs.list.length < 2, action: () => void requestCloseOtherTabs(t.id) },
    ]);
  }

  function onauxclick(e: MouseEvent, t: Tab) {
    if (e.button === 1) void requestCloseTab(t.id);
  }
</script>

<div class="tabbar" role="tablist" aria-label="Open tabs" data-tauri-drag-region>
  {#each tabs.list as t (t.id)}
    {@const Icon = iconFor(t)}
    {@const color = connections.byId(t.connId)?.color}
    <div
      class="tab"
      class:on={t.id === tabs.activeId}
      style:--conn={color ?? "var(--accent)"}
      role="tab"
      tabindex="0"
      aria-selected={t.id === tabs.activeId}
      title={t.title}
      onmousedown={(e) => e.button === 0 && tabs.activate(t.id)}
      onkeydown={(e) => (e.key === "Enter" || e.key === " ") && tabs.activate(t.id)}
      onauxclick={(e) => onauxclick(e, t)}
      oncontextmenu={(e) => tabMenu(e, t)}
    >
      <Icon size={13} />
      <span class="title">{t.title}</span>
      {#if tabs.dirty.has(t.id)}<span class="unsaved" title="Unsaved changes"></span>{/if}
      <button class="close" aria-label="Close {t.title}" onmousedown={(e) => e.stopPropagation()} onclick={() => requestCloseTab(t.id)}><X size={12} /></button>
    </div>
  {/each}
  <button class="new btn ghost icon small" title="New query (⌘T)" aria-label="New query" onclick={() => newQueryFor(tabs.active?.connId ?? connections.connected[0]?.id ?? null)}>
    <Plus size={15} />
  </button>
</div>

<style>
  .tabbar {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 100%;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
    padding: 0 6px;
  }
  .tab {
    position: relative;
    flex: 0 1 200px;
    min-width: 96px;
    display: flex;
    align-items: center;
    gap: 7px;
    height: 32px;
    padding: 0 6px 0 11px;
    border-radius: 7px 7px 0 0;
    color: var(--muted);
    background: transparent;
    outline-offset: -2px;
  }
  .tab:hover {
    background: color-mix(in srgb, var(--surface) 55%, transparent);
    color: var(--ink-2);
  }
  .tab.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: 0 0 0 1px var(--rule);
    clip-path: inset(-1px -1px 0 -1px);
  }
  /* The connection's colour marks the active tab, so you always know where you are. */
  .tab.on::before {
    content: "";
    position: absolute;
    left: 8px;
    right: 8px;
    top: 0;
    height: 2px;
    border-radius: 0 0 2px 2px;
    background: var(--conn);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .unsaved {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--edited-mark);
  }
  .close {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    opacity: 0;
  }
  .tab:hover .close,
  .tab.on .close,
  .close:focus-visible {
    opacity: 0.7;
  }
  .close:hover {
    opacity: 1;
    background: var(--sunken);
  }
  .new {
    align-self: center;
    flex: none;
  }
</style>
