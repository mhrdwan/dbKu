<script lang="ts">
  import { dialogs } from "../../lib/state/ui.svelte";
  import ExportDialog from "./ExportDialog.svelte";
  import ImportCsvDialog from "./ImportCsvDialog.svelte";
  import RunSqlFileDialog from "./RunSqlFileDialog.svelte";

  const req = $derived(dialogs.transfer);
</script>

{#if req?.kind === "export"}
  <ExportDialog connId={req.connId} database={req.database} schemaName={req.schema} only={req.only} />
{:else if req?.kind === "import"}
  <ImportCsvDialog connId={req.connId} table={req.table} />
{:else if req?.kind === "script"}
  <RunSqlFileDialog connId={req.connId} database={req.database} schemaName={req.schema} />
{/if}
