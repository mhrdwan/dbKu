<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage } from "./lib/api";
  import { newQueryFor, requestCloseTab } from "./lib/actions";
  import { connections } from "./lib/state/connections.svelte";
  import { savedQueries } from "./lib/state/saved.svelte";
  import { applyTheme, settings } from "./lib/state/settings.svelte";
  import { tabs } from "./lib/state/tabs.svelte";
  import { dialogs, toasts } from "./lib/state/ui.svelte";
  import ConnectionDialog from "./components/connection/ConnectionDialog.svelte";
  import PasswordPrompt from "./components/connection/PasswordPrompt.svelte";
  import StatusBar from "./components/layout/StatusBar.svelte";
  import TabBar from "./components/layout/TabBar.svelte";
  import Welcome from "./components/layout/Welcome.svelte";
  import QueryTab from "./components/query/QueryTab.svelte";
  import CommandPalette from "./components/search/CommandPalette.svelte";
  import SearchTab from "./components/search/SearchTab.svelte";
  import SettingsDialog from "./components/SettingsDialog.svelte";
  import Sidebar from "./components/sidebar/Sidebar.svelte";
  import TableTab from "./components/table/TableTab.svelte";
  import TransferDialogs from "./components/transfer/TransferDialogs.svelte";
  import ContextMenu from "./components/ui/ContextMenu.svelte";
  import Toasts from "./components/ui/Toasts.svelte";

  const isMac = navigator.userAgent.includes("Mac");
  let sidebarWidth = $state(settings.value.sidebarWidth);
  let sidebar = $state<Sidebar>();

  onMount(() => {
    applyTheme();
    connections.load().catch((e) => toasts.show(`Couldn't read saved connections. ${errorMessage(e)}`, "error"));
    savedQueries.load().catch((e) => toasts.show(`Couldn't read saved queries. ${errorMessage(e)}`, "error"));
  });

  function activeConn(): string | null {
    return tabs.active?.connId ?? connections.connected[0]?.id ?? null;
  }

  function onkeydown(e: KeyboardEvent) {
    const mod = isMac ? e.metaKey : e.ctrlKey;
    // Ctrl+Tab cycles tabs on every platform.
    if (!mod && !(e.ctrlKey && e.key === "Tab")) return;
    if (e.defaultPrevented) return;
    const k = e.key.toLowerCase();
    if (k === "k" || (k === "p" && !e.shiftKey)) {
      e.preventDefault();
      dialogs.palette = !dialogs.palette;
    } else if (k === "t") {
      e.preventDefault();
      newQueryFor(activeConn());
    } else if (k === "w") {
      e.preventDefault();
      if (tabs.activeId) void requestCloseTab(tabs.activeId);
    } else if (k === "f" && e.shiftKey) {
      e.preventDefault();
      tabs.openSearch(activeConn(), null, null);
    } else if (k === ",") {
      e.preventDefault();
      dialogs.settings = true;
    } else if (k === "e" && e.shiftKey) {
      e.preventDefault();
      sidebar?.focusFilter();
    } else if ((e.shiftKey && (e.key === "}" || e.key === "]")) || (e.key === "Tab" && e.ctrlKey && !e.shiftKey)) {
      e.preventDefault();
      tabs.cycle(1);
    } else if ((e.shiftKey && (e.key === "{" || e.key === "[")) || (e.key === "Tab" && e.ctrlKey && e.shiftKey)) {
      e.preventDefault();
      tabs.cycle(-1);
    }
  }

  function resizeSidebar(e: PointerEvent) {
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => (sidebarWidth = Math.min(520, Math.max(180, ev.clientX)));
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      settings.set("sidebarWidth", sidebarWidth);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
  }
</script>

<svelte:window {onkeydown} />

<div class="app" class:mac={isMac} style:--sidebar-w="{sidebarWidth}px">
  <header class="titlebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <span class="wordmark" data-tauri-drag-region>dbKu</span>
      <button class="palette-btn" onclick={() => (dialogs.palette = true)} title="Jump to a table or command">
        Search <kbd>⌘K</kbd>
      </button>
    </div>
    <TabBar />
  </header>

  <div class="body">
    <Sidebar bind:this={sidebar} />
    <div class="resizer" role="separator" aria-orientation="vertical" aria-label="Resize sidebar" onpointerdown={resizeSidebar}></div>
    <main class="main">
      {#if tabs.list.length === 0}
        <Welcome />
      {/if}
      {#each tabs.list as tab (tab.id)}
        <section class="tab-pane" class:shown={tab.id === tabs.activeId} aria-hidden={tab.id !== tabs.activeId}>
          {#if tab.kind === "query"}
            <QueryTab {tab} />
          {:else if tab.kind === "table"}
            <TableTab {tab} />
          {:else}
            <SearchTab {tab} />
          {/if}
        </section>
      {/each}
    </main>
  </div>

  <StatusBar />
</div>

{#if dialogs.connectionEditor.open}
  <ConnectionDialog />
{/if}
{#if dialogs.passwordFor}
  <PasswordPrompt id={dialogs.passwordFor} />
{/if}
{#if dialogs.palette}
  <CommandPalette />
{/if}
{#if dialogs.settings}
  <SettingsDialog />
{/if}
<TransferDialogs />
<ContextMenu />
<Toasts />

<style>
  .app {
    display: grid;
    /* minmax(0, 1fr): the middle row must not grow with a long sidebar tree. */
    grid-template-rows: var(--titlebar-h) minmax(0, 1fr) var(--statusbar-h);
    height: 100%;
  }
  .titlebar {
    display: grid;
    grid-template-columns: var(--sidebar-w) 1fr;
    align-items: stretch;
    background: var(--bg);
    border-bottom: 1px solid var(--rule);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 8px 0 14px;
    min-width: 0;
  }
  /* Leave room for the macOS traffic lights in the overlay title bar. */
  .mac .brand {
    padding-left: 80px;
  }
  .wordmark {
    font-weight: 750;
    font-size: 15px;
    letter-spacing: -0.04em;
  }
  .palette-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 4px 0 9px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-m);
    background: var(--surface);
    color: var(--faint);
    font-size: var(--text-sm);
  }
  .palette-btn:hover {
    border-color: var(--rule-strong);
    color: var(--muted);
  }
  .body {
    display: grid;
    grid-template-columns: var(--sidebar-w) 4px 1fr;
    grid-template-rows: minmax(0, 1fr);
    min-height: 0;
  }
  .resizer {
    cursor: col-resize;
    background: var(--rule);
    border-left: 1.5px solid var(--bg);
    border-right: 1.5px solid var(--bg);
  }
  .resizer:hover {
    background: var(--accent);
  }
  .main {
    position: relative;
    min-width: 0;
    min-height: 0;
    background: var(--surface);
  }
  /* Tabs stay mounted so editors, results and scroll positions survive switching. */
  .tab-pane {
    position: absolute;
    inset: 0;
    visibility: hidden;
    z-index: 0;
  }
  .tab-pane.shown {
    visibility: visible;
    z-index: 1;
  }
</style>
