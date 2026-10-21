<script lang="ts">
  import { settings, type ThemeChoice } from "../lib/state/settings.svelte";
  import { dialogs, history, toasts } from "../lib/state/ui.svelte";
  import Modal from "./ui/Modal.svelte";

  const themes: { id: ThemeChoice; label: string }[] = [
    { id: "system", label: "Match system" },
    { id: "light", label: "Light" },
    { id: "dark", label: "Dark" },
  ];
</script>

<Modal title="Settings" width={480} onclose={() => (dialogs.settings = false)}>
  <div class="rows">
    <div class="row">
      <span>Theme</span>
      <div class="seg" role="radiogroup" aria-label="Theme">
        {#each themes as t (t.id)}
          <button role="radio" aria-checked={settings.value.theme === t.id} class:on={settings.value.theme === t.id} onclick={() => settings.set("theme", t.id)}>{t.label}</button>
        {/each}
      </div>
    </div>
    <label class="row">
      <span>Row limit for queries<small>Larger results are cut off to keep the app responsive.</small></span>
      <select class="field" value={settings.value.maxRows} onchange={(e) => settings.set("maxRows", Number(e.currentTarget.value))}>
        {#each [500, 1000, 5000, 20000, 50000, 200000] as n (n)}<option value={n}>{n.toLocaleString()} rows</option>{/each}
      </select>
    </label>
    <label class="row">
      <span>Editor font size</span>
      <select class="field" value={settings.value.editorFontSize} onchange={(e) => settings.set("editorFontSize", Number(e.currentTarget.value))}>
        {#each [12, 13, 14, 15, 16, 18] as n (n)}<option value={n}>{n}px</option>{/each}
      </select>
    </label>
    <div class="row">
      <span>Query history<small>{history.entries.length} saved on this computer.</small></span>
      <button class="btn small danger" disabled={history.entries.length === 0} onclick={() => { history.clear(); toasts.show("History cleared", "success", 1500); }}>Clear history</button>
    </div>
  </div>
</Modal>

<style>
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 0;
    border-bottom: 1px solid var(--rule);
  }
  .row:last-child {
    border-bottom: 0;
  }
  .row > span {
    display: flex;
    flex-direction: column;
    font-weight: 500;
  }
  small {
    font-weight: 400;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .seg {
    display: flex;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--sunken);
  }
  .seg button {
    height: 24px;
    padding: 0 10px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--muted);
    font-weight: 500;
  }
  .seg button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: 0 0 0 1px var(--rule);
  }
</style>
