<script lang="ts">
  import { CircleAlert, CircleCheck, Info, X } from "@lucide/svelte";
  import { toasts } from "../../lib/state/ui.svelte";

  const icons = { info: Info, success: CircleCheck, error: CircleAlert };
</script>

<div class="stack" aria-live="polite">
  {#each toasts.list as t (t.id)}
    {@const Icon = icons[t.kind]}
    <div class="toast {t.kind}" role={t.kind === "error" ? "alert" : "status"}>
      <Icon size={15} />
      <p class="selectable">{t.message}</p>
      <button class="btn ghost icon small" aria-label="Dismiss" onclick={() => toasts.dismiss(t.id)}><X size={13} /></button>
    </div>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: 16px;
    bottom: calc(var(--statusbar-h) + 12px);
    z-index: 70;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(420px, calc(100vw - 32px));
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 8px 10px 12px;
    border-radius: var(--radius-m);
    background: var(--raised);
    border: 1px solid var(--rule-strong);
    border-left: 3px solid var(--muted);
    box-shadow: var(--shadow-pop);
    animation: slide var(--duration) var(--ease-out);
  }
  .toast :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .toast.success {
    border-left-color: var(--ok);
  }
  .toast.success > :global(svg) {
    color: var(--ok);
  }
  .toast.error {
    border-left-color: var(--danger);
  }
  .toast.error > :global(svg) {
    color: var(--danger);
  }
  p {
    flex: 1;
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
  }
  @keyframes slide {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
</style>
