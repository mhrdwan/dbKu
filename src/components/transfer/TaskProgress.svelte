<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { api } from "../../lib/api";
  import type { TransferProgress } from "../../lib/types";

  interface Props {
    taskId: string;
    /** How to read `done`/`total`: bytes of a file, or a count of things. */
    unit?: "bytes" | "count";
    noun?: string;
  }

  let { taskId, unit = "count", noun = "" }: Props = $props();
  let progress = $state<TransferProgress | null>(null);
  let cancelling = $state(false);

  $effect(() => {
    let stop: (() => void) | undefined;
    let closed = false;
    void listen<TransferProgress>("transfer-progress", (e) => {
      if (e.payload.taskId === taskId) progress = e.payload;
    }).then((un) => (closed ? un() : (stop = un)));
    return () => {
      closed = true;
      stop?.();
    };
  });

  const percent = $derived(
    progress?.total ? Math.min(100, Math.round((progress.done / progress.total) * 100)) : null,
  );

  function amount(n: number): string {
    if (unit === "bytes") {
      return n < 1024 * 1024 ? `${Math.round(n / 1024)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
    }
    return n.toLocaleString();
  }

  async function cancel() {
    cancelling = true;
    await api.cancelTask(taskId).catch(() => undefined);
  }
</script>

<div class="task" role="status" aria-live="polite">
  <div class="track" class:indeterminate={percent === null}>
    <div class="fill" style:width="{percent ?? 30}%"></div>
  </div>
  <div class="line">
    <span class="label">
      {#if progress?.label}{progress.label}{:else}Starting…{/if}
    </span>
    <span class="count">
      {#if progress?.total}{amount(progress.done)} of {amount(progress.total)} {noun}{:else if percent !== null}{percent}%{/if}
    </span>
    <button class="btn small" onclick={cancel} disabled={cancelling}>{cancelling ? "Stopping…" : "Cancel"}</button>
  </div>
</div>

<style>
  .task {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .track {
    height: 6px;
    border-radius: 3px;
    background: var(--sunken);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width var(--duration) var(--ease-out);
  }
  .indeterminate .fill {
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .line {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: var(--text-sm);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
  }
  .count {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
