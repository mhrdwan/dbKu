<script lang="ts">
  import { copyText } from "../../lib/actions";
  import { plural } from "../../lib/util/format";
  import Modal from "../ui/Modal.svelte";

  interface Props {
    statements: string[];
    saving: boolean;
    onapply: () => void;
    onclose: () => void;
  }

  let { statements, saving, onapply, onclose }: Props = $props();
</script>

<Modal title="Review changes" width={720} {onclose}>
  <p class="lead">
    These {plural(statements.length, "statement")} run in one transaction. If any edited row no longer
    matches exactly one row, nothing is saved.
  </p>
  <ol class="sql scroll selectable">
    {#each statements as s, i (i)}
      <li class:del={s.startsWith("DELETE")} class:ins={s.startsWith("INSERT")}><code>{s};</code></li>
    {/each}
  </ol>

  {#snippet footer()}
    <button class="btn small ghost" onclick={() => copyText(statements.map((s) => s + ";").join("\n"), "SQL copied")}>Copy SQL</button>
    <span class="spacer"></span>
    <button class="btn" onclick={onclose}>Keep editing</button>
    <button class="btn primary" disabled={saving} onclick={onapply}>{saving ? "Saving…" : "Save changes"}</button>
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 12px;
    color: var(--ink-2);
    max-width: 64ch;
  }
  .sql {
    margin: 0;
    padding: 0;
    list-style: none;
    max-height: 380px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-m);
    background: var(--surface-2);
  }
  li {
    padding: 7px 12px 7px 14px;
    border-bottom: 1px solid var(--rule);
    box-shadow: inset 3px 0 0 var(--edited-mark);
  }
  li:last-child {
    border-bottom: 0;
  }
  li.del {
    box-shadow: inset 3px 0 0 var(--danger);
  }
  li.ins {
    box-shadow: inset 3px 0 0 var(--ok);
  }
  code {
    font: var(--text-sm) / 1.5 var(--font-code);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .spacer {
    flex: 1;
  }
</style>
