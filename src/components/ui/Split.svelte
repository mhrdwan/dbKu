<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    /** Fraction of the space given to the first pane. */
    ratio?: number;
    min?: number;
    first: Snippet;
    second: Snippet;
  }

  let { ratio = $bindable(0.45), min = 0.12, first, second }: Props = $props();
  let box: HTMLDivElement;
  let dragging = $state(false);

  function start(e: PointerEvent) {
    e.preventDefault();
    dragging = true;
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const rect = box.getBoundingClientRect();
    const move = (ev: PointerEvent) => {
      ratio = Math.min(1 - min, Math.max(min, (ev.clientY - rect.top) / rect.height));
    };
    const up = () => {
      dragging = false;
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
  }

  function key(e: KeyboardEvent) {
    if (e.key === "ArrowUp") ratio = Math.max(min, ratio - 0.04);
    if (e.key === "ArrowDown") ratio = Math.min(1 - min, ratio + 0.04);
  }
</script>

<div class="split" class:dragging bind:this={box} style:grid-template-rows="{ratio}fr 5px {1 - ratio}fr">
  <div class="pane">{@render first()}</div>
  <!-- A focusable separator is a valid ARIA widget (resizable with the arrow keys). -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="handle"
    role="separator"
    aria-orientation="horizontal"
    aria-valuenow={Math.round(ratio * 100)}
    tabindex="0"
    onpointerdown={start}
    onkeydown={key}
  ></div>
  <div class="pane">{@render second()}</div>
</div>

<style>
  .split {
    display: grid;
    height: 100%;
    min-height: 0;
  }
  .pane {
    min-height: 0;
    overflow: hidden;
  }
  .handle {
    cursor: row-resize;
    background: var(--rule);
    border-top: 2px solid var(--bg);
    border-bottom: 2px solid var(--bg);
    transition: background var(--duration-fast);
  }
  .handle:hover,
  .dragging .handle,
  .handle:focus-visible {
    background: var(--accent);
    outline: none;
  }
  .dragging {
    cursor: row-resize;
    user-select: none;
  }
</style>
