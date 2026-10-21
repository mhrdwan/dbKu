<script lang="ts">
  import { DRIVERS } from "../../lib/state/connections.svelte";
  import type { Driver } from "../../lib/types";

  interface Props {
    driver: Driver;
    color?: string | null;
    size?: number;
  }

  let { driver, color = null, size = 18 }: Props = $props();
  const info = $derived(DRIVERS[driver]);
</script>

<!-- Engine monogram; the connection's own colour (if any) shows as a ring. -->
<span
  class="mark"
  title={info.label}
  style:--engine={info.color}
  style:--conn={color ?? "transparent"}
  style:width="{size}px"
  style:height="{size}px"
  style:font-size="{Math.round(size * 0.5)}px"
>
  {info.mono}
</span>

<style>
  .mark {
    flex: none;
    display: inline-grid;
    place-items: center;
    border-radius: 5px;
    background: color-mix(in srgb, var(--engine) 14%, var(--surface));
    color: color-mix(in srgb, var(--engine), white var(--engine-lift));
    font-weight: 700;
    letter-spacing: -0.02em;
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--engine) 30%, transparent),
      0 0 0 2px var(--conn);
  }
</style>
