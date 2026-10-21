<script lang="ts">
  import type { Snippet } from "svelte";
  import { ChevronRight, LoaderCircle } from "@lucide/svelte";

  interface Props {
    depth: number;
    label: string;
    expandable?: boolean;
    expanded?: boolean;
    loading?: boolean;
    active?: boolean;
    title?: string;
    icon?: Snippet;
    trailing?: Snippet;
    onclick?: (e: MouseEvent) => void;
    ondblclick?: (e: MouseEvent) => void;
    oncontextmenu?: (e: MouseEvent) => void;
    ontoggle?: () => void;
  }

  let {
    depth,
    label,
    expandable = false,
    expanded = false,
    loading = false,
    active = false,
    title,
    icon,
    trailing,
    onclick,
    ondblclick,
    oncontextmenu,
    ontoggle,
  }: Props = $props();

  function onkeydown(e: KeyboardEvent) {
    if (!expandable) return;
    if ((e.key === "ArrowRight" && !expanded) || (e.key === "ArrowLeft" && expanded)) {
      e.preventDefault();
      ontoggle?.();
    }
  }
</script>

<button
  class="row"
  class:active
  style:padding-left="{6 + depth * 14}px"
  role="treeitem"
  aria-level={depth + 1}
  aria-expanded={expandable ? expanded : undefined}
  aria-selected={active}
  {title}
  onclick={(e) => (onclick ? onclick(e) : ontoggle?.())}
  {ondblclick}
  {oncontextmenu}
  {onkeydown}
>
  <span
    class="chev"
    class:open={expanded}
    class:hidden={!expandable}
    role="presentation"
    onclick={(e) => {
      if (!expandable || !onclick) return;
      e.stopPropagation();
      ontoggle?.();
    }}
  >
    {#if loading}<LoaderCircle size={12} class="spin" />{:else}<ChevronRight size={12} />{/if}
  </span>
  {#if icon}{@render icon()}{/if}
  <span class="label">{label}</span>
  {#if trailing}{@render trailing()}{/if}
</button>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 26px;
    padding-right: 8px;
    border: 0;
    border-radius: var(--radius-s);
    background: none;
    color: var(--ink-2);
    text-align: left;
    white-space: nowrap;
  }
  .row:hover {
    background: var(--sunken);
    color: var(--ink);
  }
  .row.active {
    background: var(--accent-soft);
    color: var(--ink);
  }
  .row:focus-visible {
    outline-offset: -2px;
  }
  .chev {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 14px;
    height: 14px;
    color: var(--faint);
    transition: transform var(--duration-fast) var(--ease-out);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .chev.hidden {
    visibility: hidden;
  }
  .chev :global(.spin) {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row :global(.ico) {
    flex: none;
    color: var(--muted);
  }
</style>
