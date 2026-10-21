<script lang="ts">
  import { openConnection } from "../../lib/actions";
  import { connections } from "../../lib/state/connections.svelte";
  import { dialogs } from "../../lib/state/ui.svelte";
  import Modal from "../ui/Modal.svelte";

  interface Props {
    id: string;
  }

  let { id }: Props = $props();
  let password = $state("");
  let busy = $state(false);
  const profile = $derived(connections.byId(id));

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    const ok = await openConnection(id, password);
    busy = false;
    if (ok) dialogs.passwordFor = null;
  }
</script>

<Modal title="Password for {profile?.name ?? 'connection'}" width={400} onclose={() => (dialogs.passwordFor = null)}>
  <form id="pw-form" onsubmit={submit}>
    <p>{profile?.user}@{profile?.host} — this password isn't stored, so dbKu asks each time you connect.</p>
    <input class="field" type="password" bind:value={password} aria-label="Password" autocomplete="off" />
    {#if connections.errors[id] && !busy}<p class="error selectable">{connections.errors[id]}</p>{/if}
  </form>
  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" onclick={() => (dialogs.passwordFor = null)}>Cancel</button>
    <button class="btn primary" type="submit" form="pw-form" disabled={busy}>{busy ? "Connecting…" : "Connect"}</button>
  {/snippet}
</Modal>

<style>
  p {
    margin: 0 0 12px;
    color: var(--ink-2);
  }
  .field {
    width: 100%;
  }
  .error {
    margin: 10px 0 0;
    color: var(--danger);
    font-size: var(--text-sm);
  }
  .spacer {
    flex: 1;
  }
</style>
