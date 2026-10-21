<script lang="ts">
  import Modal from "./Modal.svelte";

  interface Props {
    title: string;
    label: string;
    value?: string;
    confirmLabel: string;
    onconfirm: (value: string) => void | Promise<void>;
    onclose: () => void;
  }

  let { title, label, value = "", confirmLabel, onconfirm, onclose }: Props = $props();
  // svelte-ignore state_referenced_locally — the prompt edits a copy.
  let text = $state(value);
  let busy = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!text.trim()) return;
    busy = true;
    try {
      await onconfirm(text.trim());
    } finally {
      busy = false;
    }
  }
</script>

<Modal {title} width={420} {onclose}>
  <form id="prompt-form" onsubmit={submit}>
    <label>
      <span>{label}</span>
      <input class="field" bind:value={text} />
    </label>
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" type="submit" form="prompt-form" disabled={busy || !text.trim()}>{confirmLabel}</button>
  {/snippet}
</Modal>

<style>
  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  label span {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--ink-2);
  }
  .field {
    width: 100%;
  }
  .spacer {
    flex: 1;
  }
</style>
