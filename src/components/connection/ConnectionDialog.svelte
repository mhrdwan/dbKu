<script lang="ts">
  import { CircleAlert, CircleCheck, FilePlus, FolderOpen, LoaderCircle, Lock } from "@lucide/svelte";
  import { api, errorMessage } from "../../lib/api";
  import { openConnection } from "../../lib/actions";
  import { blankProfile, CONNECTION_COLORS, connections, DRIVERS } from "../../lib/state/connections.svelte";
  import { tabs } from "../../lib/state/tabs.svelte";
  import { dialogs, toasts } from "../../lib/state/ui.svelte";
  import type { ConnectionProfile, Driver } from "../../lib/types";
  import { pickFile } from "../../lib/util/files";
  import EngineMark from "../ui/EngineMark.svelte";
  import Modal from "../ui/Modal.svelte";

  const editingId = dialogs.connectionEditor.id;
  const source = connections.byId(editingId ?? dialogs.connectionEditor.duplicateOf);
  const isEdit = !!editingId && !!source;

  let form = $state<ConnectionProfile>(
    source ? { ...source, ...(isEdit ? {} : { id: "", name: `${source.name} copy` }) } : blankProfile(),
  );
  let password = $state("");
  let test = $state<{ state: "idle" | "running" | "ok" | "error"; message: string }>({ state: "idle", message: "" });
  let saving = $state(false);
  let error = $state<string | null>(null);

  const isSqlite = $derived(form.driver === "sqlite");
  const drivers = Object.entries(DRIVERS) as [Driver, (typeof DRIVERS)[Driver]][];

  function chooseDriver(d: Driver) {
    if (d === form.driver) return;
    const fresh = blankProfile(d);
    form = { ...form, driver: d, port: fresh.port, user: form.user || fresh.user, host: d === "sqlite" ? "" : form.host || "localhost", database: "" };
    test = { state: "idle", message: "" };
  }

  function input() {
    const name = form.name.trim() || (isSqlite ? form.database.split(/[\\/]/).pop() ?? "SQLite" : `${form.host}${form.database ? ` / ${form.database}` : ""}`);
    return {
      ...form,
      name,
      port: Number(form.port) || DRIVERS[form.driver].port,
      // Only send a password when the user typed one; blank keeps the saved one.
      password: password ? password : isEdit ? null : "",
    };
  }

  async function runTest() {
    test = { state: "running", message: "" };
    try {
      const info = await api.testConnection(input());
      test = { state: "ok", message: info.version || "Connected" };
    } catch (e) {
      test = { state: "error", message: errorMessage(e) };
    }
  }

  async function save(connect: boolean) {
    error = null;
    saving = true;
    try {
      const saved = await connections.save(input());
      // The old session is closed on save; its table tabs would only show errors.
      if (isEdit) tabs.closeForConnection(saved.id);
      dialogs.closeConnectionEditor();
      toasts.show(`Saved “${saved.name}”`, "success", 1800);
      if (connect) await openConnection(saved.id, saved.savePassword ? undefined : password || undefined);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  /** Scroll a result into view: it can sit below the dialog's fold. */
  function reveal(node: HTMLElement) {
    node.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  async function browse(create: boolean) {
    const path = await pickFile([{ name: "SQLite database", extensions: ["db", "sqlite", "sqlite3", "db3"] }], create);
    if (path) form.database = path;
  }
</script>

<Modal title={isEdit ? "Edit connection" : "New connection"} width={600} onclose={() => dialogs.closeConnectionEditor()}>
  <form id="conn-form" onsubmit={(e) => { e.preventDefault(); void save(true); }}>
    <div class="engines" role="radiogroup" aria-label="Database engine">
      {#each drivers as [d, info] (d)}
        <button type="button" role="radio" aria-checked={form.driver === d} class:on={form.driver === d} onclick={() => chooseDriver(d)}>
          <EngineMark driver={d} size={22} />
          <span>{info.label}</span>
        </button>
      {/each}
    </div>

    <div class="grid">
      <label class="wide">
        <span>Name</span>
        <input class="field" bind:value={form.name} placeholder={isSqlite ? "My SQLite file" : "Production orders"} />
      </label>

      {#if isSqlite}
        <label class="wide">
          <span>Database file</span>
          <div class="file">
            <input class="field code" bind:value={form.database} placeholder="/path/to/database.db" required />
            <button type="button" class="btn" onclick={() => browse(false)}><FolderOpen size={14} /> Open…</button>
            <button type="button" class="btn" onclick={() => browse(true)} title="Create a new database file"><FilePlus size={14} /> New…</button>
          </div>
        </label>
      {:else}
        <label class="host">
          <span>Host</span>
          <input class="field" bind:value={form.host} placeholder="localhost" required autocapitalize="off" spellcheck="false" />
        </label>
        <label class="port">
          <span>Port</span>
          <input class="field" type="number" min="1" max="65535" bind:value={form.port} />
        </label>
        <label>
          <span>User</span>
          <input class="field" bind:value={form.user} autocapitalize="off" spellcheck="false" />
        </label>
        <label>
          <span>Password</span>
          <input
            class="field"
            type="password"
            bind:value={password}
            placeholder={isEdit && form.savePassword ? "Saved — leave blank to keep" : ""}
            autocomplete="off"
          />
        </label>
        <label class="wide check">
          <input type="checkbox" bind:checked={form.savePassword} />
          <span>Remember the password in the system keychain</span>
        </label>
        <label>
          <span>Database <em>optional</em></span>
          <input class="field" bind:value={form.database} placeholder={form.driver === "postgres" ? "postgres" : ""} autocapitalize="off" spellcheck="false" />
        </label>
        <label>
          <span>Encryption</span>
          <select class="field" bind:value={form.sslMode}>
            <option value="prefer">Use TLS if available</option>
            <option value="require">Require TLS</option>
            <option value="disable">Off</option>
          </select>
        </label>
        {#if form.driver === "mssql"}
          <label class="wide check">
            <input type="checkbox" bind:checked={form.trustServerCert} />
            <span>Trust server certificate (for self-signed certificates on internal servers)</span>
          </label>
          <label class="wide check">
            <input type="checkbox" bind:checked={form.legacyTls} />
            <span>Allow old TLS 1.0 (SQL Server 2008, or 2012 without updates)</span>
          </label>
        {/if}
      {/if}

      <div class="wide readonly" class:on={form.readOnly}>
        <label class="check">
          <input type="checkbox" bind:checked={form.readOnly} />
          <span><Lock size={12} /> Read-only connection</span>
        </label>
        <p>
          Only SELECT-style statements run. Writes, DDL and procedure calls are refused before they reach the
          server, and the session itself is opened read-only where the database supports it. Use this for
          production systems you must not change.
        </p>
      </div>

      <div class="wide colors">
        <span>Colour</span>
        <div class="swatches" role="radiogroup" aria-label="Connection colour">
          <button type="button" role="radio" aria-checked={!form.color} class="swatch none" class:on={!form.color} aria-label="No colour" onclick={() => (form.color = null)}></button>
          {#each CONNECTION_COLORS as c (c)}
            <button type="button" role="radio" aria-checked={form.color === c} class="swatch" class:on={form.color === c} style:--c={c} aria-label="Colour {c}" onclick={() => (form.color = c)}></button>
          {/each}
        </div>
        <label class="group-field">
          <span>Group</span>
          <input class="field" bind:value={form.group} placeholder="e.g. Clients" />
        </label>
      </div>
    </div>

    {#if test.state !== "idle"}
      <div class="test {test.state}" role="status" use:reveal>
        {#if test.state === "running"}<LoaderCircle size={14} class="spin" /> Connecting…
        {:else if test.state === "ok"}<CircleCheck size={14} /> <span class="selectable">{test.message}</span>
        {:else}<CircleAlert size={14} /> <span class="selectable">{test.message}</span>{/if}
      </div>
    {/if}
    {#if error}<p class="error selectable" use:reveal>{error}</p>{/if}
  </form>

  {#snippet footer()}
    <button type="button" class="btn" onclick={runTest} disabled={test.state === "running"}>Test connection</button>
    <span class="spacer"></span>
    <button type="button" class="btn" onclick={() => save(false)} disabled={saving}>Save</button>
    <button type="submit" form="conn-form" class="btn primary" disabled={saving}>Save & connect</button>
  {/snippet}
</Modal>

<style>
  .engines {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 6px;
    margin-bottom: 18px;
  }
  .engines button {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 10px 6px 8px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-m);
    background: var(--surface);
    color: var(--ink-2);
    font-size: var(--text-sm);
    font-weight: 500;
  }
  .engines button:hover {
    border-color: var(--rule-strong);
  }
  .engines button.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
    color: var(--ink);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px 12px;
  }
  .grid .host {
    grid-column: 1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }
  label > span,
  .colors > span {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--ink-2);
  }
  em {
    font-style: normal;
    font-weight: 400;
    color: var(--faint);
  }
  .wide {
    grid-column: 1 / -1;
  }
  .check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
  .check span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-weight: 400;
    color: var(--ink-2);
  }
  .file {
    display: flex;
    gap: 6px;
  }
  .file .field {
    flex: 1;
  }
  .readonly {
    padding: 10px 12px;
    border-radius: var(--radius-m);
    border: 1px solid var(--rule);
    background: var(--surface-2);
  }
  .readonly.on {
    border-color: color-mix(in srgb, var(--readonly) 45%, var(--rule));
    background: var(--readonly-soft);
  }
  .readonly.on .check span {
    color: var(--readonly);
    font-weight: 600;
  }
  .readonly p {
    margin: 6px 0 0 24px;
    font-size: var(--text-sm);
    color: var(--muted);
    max-width: 62ch;
  }
  .colors {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .swatches {
    display: flex;
    gap: 6px;
  }
  .swatch {
    width: 20px;
    height: 20px;
    padding: 0;
    border-radius: 50%;
    border: 0;
    background: var(--c);
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.12);
  }
  .swatch.none {
    background: repeating-linear-gradient(135deg, var(--surface) 0 4px, var(--sunken) 4px 8px);
  }
  .swatch.on {
    box-shadow: 0 0 0 2px var(--surface), 0 0 0 4px var(--c, var(--faint));
  }
  .group-field {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    margin-left: auto;
  }
  .group-field .field {
    width: 140px;
  }
  .test {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-top: 16px;
    padding: 9px 12px;
    border-radius: var(--radius-m);
    background: var(--surface-2);
    font-size: var(--text-sm);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .test :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .test.ok {
    background: var(--ok-soft);
    color: var(--ok);
  }
  .test.error {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .test :global(.spin) {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .error {
    margin: 12px 0 0;
    color: var(--danger);
  }
  .spacer {
    flex: 1;
  }
</style>
