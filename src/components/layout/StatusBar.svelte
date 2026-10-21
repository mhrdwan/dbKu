<script lang="ts">
  import { Lock, Moon, Settings, Sun } from "@lucide/svelte";
  import { connections } from "../../lib/state/connections.svelte";
  import { isDarkNow, settings } from "../../lib/state/settings.svelte";
  import { tabs } from "../../lib/state/tabs.svelte";
  import { dialogs } from "../../lib/state/ui.svelte";

  const tab = $derived(tabs.active);
  const profile = $derived(connections.byId(tab?.connId));
  const info = $derived(profile ? connections.info[profile.id] : undefined);
  const where = $derived.by(() => {
    if (!tab) return [];
    if (tab.kind === "table") return [tab.table.database, tab.table.schema].filter(Boolean);
    return [tab.database, tab.schema].filter(Boolean);
  });
  const dark = $derived(isDarkNow());
</script>

<footer class="status" style:--conn={profile?.color ?? "transparent"}>
  {#if profile}
    <span class="conn" class:colored={!!profile.color}>
      <span class="dot {connections.status[profile.id] ?? 'idle'}"></span>
      {profile.name}
    </span>
    {#each where as w (w)}<span class="seg">{w}</span>{/each}
    {#if profile.readOnly}<span class="ro"><Lock size={11} /> Read-only</span>{/if}
    {#if info?.version}<span class="version" title={info.version}>{info.version}</span>{/if}
  {:else}
    <span class="seg">{connections.connected.length > 0 ? `${connections.connected.length} connected` : "Not connected"}</span>
  {/if}
  <span class="spacer"></span>
  <button
    class="icon-btn"
    aria-label="Toggle theme"
    title="Toggle light / dark"
    onclick={() => settings.set("theme", isDarkNow() ? "light" : "dark")}
  >
    {#if dark}<Sun size={13} />{:else}<Moon size={13} />{/if}
  </button>
  <button class="icon-btn" aria-label="Settings" title="Settings (⌘,)" onclick={() => (dialogs.settings = true)}><Settings size={13} /></button>
</footer>

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 14px;
    height: var(--statusbar-h);
    padding: 0 6px 0 12px;
    border-top: 1px solid var(--rule);
    background: var(--bg);
    color: var(--muted);
    font-size: var(--text-sm);
    white-space: nowrap;
    overflow: hidden;
    box-shadow: inset 0 2px 0 var(--conn);
  }
  .conn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ink);
    font-weight: 600;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--faint);
  }
  .dot.connected {
    background: var(--ok);
  }
  .dot.error {
    background: var(--danger);
  }
  .seg + .seg::before,
  .conn + .seg::before {
    content: "/";
    margin-right: 14px;
    color: var(--rule-strong);
  }
  .ro {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 7px;
    border-radius: 9px;
    color: var(--readonly);
    background: var(--readonly-soft);
    font-weight: 600;
  }
  .version {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--faint);
  }
  .spacer {
    flex: 1;
  }
  .icon-btn {
    display: grid;
    place-items: center;
    width: 22px;
    height: 20px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--muted);
  }
  .icon-btn:hover {
    background: var(--sunken);
    color: var(--ink);
  }
</style>
