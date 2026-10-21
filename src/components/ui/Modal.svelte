<script lang="ts">
  import type { Snippet } from "svelte";
  import { X } from "@lucide/svelte";

  interface Props {
    title: string;
    width?: number;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
  }

  let { title, width = 520, onclose, children, footer }: Props = $props();
  let dialog: HTMLDivElement;

  $effect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const first = dialog.querySelector<HTMLElement>("input, select, textarea, button:not(.close)");
    first?.focus();
    return () => previous?.focus?.();
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    }
    if (e.key !== "Tab") return;
    const focusable = [...dialog.querySelectorAll<HTMLElement>("input, select, textarea, button, [tabindex='0']")].filter(
      (el) => !el.hasAttribute("disabled"),
    );
    const first = focusable[0];
    const last = focusable.at(-1);
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last?.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first?.focus();
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-label={title}
    style:width="{width}px"
    bind:this={dialog}
    {onkeydown}
  >
    <header>
      <h2>{title}</h2>
      <button class="btn ghost icon small close" aria-label="Close" onclick={onclose}><X size={15} /></button>
    </header>
    <div class="body scroll">
      {@render children()}
    </div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--ink) 22%, transparent);
    animation: fade var(--duration) var(--ease-out);
  }
  .dialog {
    max-width: 100%;
    max-height: min(86vh, 760px);
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius-l);
    box-shadow: var(--shadow-modal);
    animation: rise var(--duration) var(--ease-out);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 14px 10px 20px;
  }
  h2 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .body {
    padding: 4px 20px 18px;
    min-height: 0;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--rule);
    background: var(--surface-2);
    border-radius: 0 0 var(--radius-l) var(--radius-l);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.99);
    }
  }
</style>
