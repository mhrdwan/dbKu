<script lang="ts">
  import { menu } from "../../lib/state/ui.svelte";

  let el = $state<HTMLDivElement>();
  let pos = $state({ x: 0, y: 0 });

  // Keep the menu inside the window.
  $effect(() => {
    if (!menu.open || !el) return;
    const { width, height } = el.getBoundingClientRect();
    pos = {
      x: Math.min(menu.x, window.innerWidth - width - 6),
      y: Math.min(menu.y, window.innerHeight - height - 6),
    };
    el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  });

  function run(action?: () => void) {
    menu.close();
    action?.();
  }

  function onkeydown(e: KeyboardEvent) {
    if (!menu.open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      menu.close();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...(el?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.key === "ArrowDown" ? (index + 1) % items.length : (index - 1 + items.length) % items.length;
    items[next]?.focus();
  }
</script>

<svelte:window {onkeydown} onblur={() => menu.close()} />

{#if menu.open}
  <div class="catcher" role="presentation" onmousedown={() => menu.close()} oncontextmenu={(e) => { e.preventDefault(); menu.close(); }}></div>
  <div class="menu" role="menu" bind:this={el} style:left="{pos.x || menu.x}px" style:top="{pos.y || menu.y}px">
    {#each menu.items as item, i (i)}
      {#if item.separator}
        <div class="sep" role="separator"></div>
      {:else}
        <button role="menuitem" class:danger={item.danger} disabled={item.disabled} onclick={() => run(item.action)}>
          <span class="icon">
            {#if item.icon}<item.icon size={14} />{/if}
          </span>
          <span class="label">{item.label}</span>
          {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 60;
  }
  .menu {
    position: fixed;
    z-index: 61;
    min-width: 200px;
    padding: 4px;
    background: var(--raised);
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius-m);
    box-shadow: var(--shadow-pop);
    animation: pop var(--duration-fast) var(--ease-out);
  }
  button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-s);
    background: none;
    text-align: left;
  }
  button:hover:not(:disabled),
  button:focus-visible {
    background: var(--accent);
    color: var(--accent-ink);
    outline: none;
  }
  button:disabled {
    color: var(--faint);
  }
  button.danger:not(:hover):not(:focus-visible) {
    color: var(--danger);
  }
  .icon {
    display: inline-flex;
    width: 14px;
    opacity: 0.8;
  }
  .label {
    flex: 1;
  }
  .shortcut {
    font-size: var(--text-xs);
    opacity: 0.6;
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--rule);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }
</style>
