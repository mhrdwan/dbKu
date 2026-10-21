<script lang="ts">
  import { Braces } from "@lucide/svelte";
  import { copyText } from "../../lib/actions";
  import type { Cell } from "../../lib/types";
  import { tryPrettyJson } from "../../lib/util/format";
  import Modal from "../ui/Modal.svelte";
  import type { GridColumn } from "./DataGrid.svelte";

  interface Props {
    column: GridColumn;
    value: Cell;
    editable: boolean;
    onsave: (v: Cell) => void;
    onclose: () => void;
  }

  let { column, value, editable, onsave, onclose }: Props = $props();
  // svelte-ignore state_referenced_locally — the viewer edits a snapshot.
  let text = $state(value ?? "");
  // svelte-ignore state_referenced_locally
  let isNull = $state(value === null);
  const pretty = $derived(tryPrettyJson(text));

  function save() {
    onsave(isNull ? null : text);
    onclose();
  }
</script>

<Modal title={column.name} width={640} {onclose}>
  <p class="meta">{column.typeName}{isNull ? " — NULL" : ` — ${text.length.toLocaleString()} characters`}</p>
  <textarea
    class="field code"
    bind:value={text}
    readonly={!editable}
    disabled={isNull}
    spellcheck="false"
    oninput={() => (isNull = false)}
    onkeydown={(e) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && editable) save();
    }}
  ></textarea>

  {#snippet footer()}
    <button class="btn small" onclick={() => copyText(isNull ? "NULL" : text, "Value copied")}>Copy</button>
    {#if pretty && pretty !== text}
      <button class="btn small" onclick={() => (text = pretty)}><Braces size={13} /> Format JSON</button>
    {/if}
    {#if editable}
      <label class="null-toggle"><input type="checkbox" bind:checked={isNull} /> NULL</label>
    {/if}
    <span class="spacer"></span>
    <button class="btn" onclick={onclose}>{editable ? "Cancel" : "Close"}</button>
    {#if editable}
      <button class="btn primary" onclick={save}>Apply <kbd>⌘↵</kbd></button>
    {/if}
  {/snippet}
</Modal>

<style>
  .meta {
    margin: 0 0 8px;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  textarea {
    width: 100%;
    height: 340px;
    padding: 10px;
    resize: vertical;
    line-height: 1.5;
    white-space: pre-wrap;
  }
  textarea:disabled {
    opacity: 0.5;
  }
  .null-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: 6px;
    color: var(--ink-2);
  }
  .spacer {
    flex: 1;
  }
  kbd {
    background: transparent;
    color: inherit;
    border-color: color-mix(in srgb, currentColor 40%, transparent);
  }
</style>
