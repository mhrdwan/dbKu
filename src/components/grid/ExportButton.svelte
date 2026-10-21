<script lang="ts">
  import { Download } from "@lucide/svelte";
  import { copyText } from "../../lib/actions";
  import { menu } from "../../lib/state/ui.svelte";
  import { toasts } from "../../lib/state/ui.svelte";
  import { errorMessage } from "../../lib/api";
  import type { Cell } from "../../lib/types";
  import { toClipboardTsv, toDelimited, toJson, toMarkdown } from "../../lib/util/export";
  import { safeFileName, saveTextFile } from "../../lib/util/files";

  interface Props {
    name: string;
    columns: string[];
    rows: Cell[][];
    /** Fetch every row instead of the visible page (table export). */
    loadAll?: () => Promise<Cell[][]>;
  }

  let { name, columns, rows, loadAll }: Props = $props();

  async function data(all: boolean): Promise<Cell[][]> {
    return all && loadAll ? loadAll() : rows;
  }

  async function exportAs(format: "csv" | "tsv" | "json" | "md", all: boolean) {
    try {
      const d = await data(all);
      const content =
        format === "csv" ? toDelimited(columns, d) : format === "tsv" ? toDelimited(columns, d, "\t")
        : format === "json" ? toJson(columns, d) : toMarkdown(columns, d);
      const labels = { csv: "CSV", tsv: "TSV", json: "JSON", md: "Markdown" };
      const path = await saveTextFile(`${safeFileName(name)}.${format}`, content, [
        { name: labels[format], extensions: [format] },
      ]);
      if (path) toasts.show(`Exported ${d.length.toLocaleString()} rows to ${path}`, "success");
    } catch (e) {
      toasts.show(`Export failed. ${errorMessage(e)}`, "error");
    }
  }

  function open(e: MouseEvent) {
    const scope = loadAll ? " (this page)" : "";
    const items = [
      { label: `Copy as TSV${scope}`, action: () => void copyText(toClipboardTsv([columns, ...rows]), "Copied for spreadsheet") },
      { label: "", separator: true },
      { label: `Save CSV${scope}…`, action: () => void exportAs("csv", false) },
      { label: `Save JSON${scope}…`, action: () => void exportAs("json", false) },
      { label: `Save Markdown${scope}…`, action: () => void exportAs("md", false) },
    ];
    if (loadAll) {
      items.push(
        { label: "", separator: true },
        { label: "Save all rows as CSV…", action: () => void exportAs("csv", true) },
        { label: "Save all rows as JSON…", action: () => void exportAs("json", true) },
      );
    }
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu.show(new MouseEvent("contextmenu", { clientX: rect.left, clientY: rect.bottom + 4 }), items);
  }
</script>

<button class="btn small ghost" onclick={open} disabled={columns.length === 0} title="Export or copy results">
  <Download size={13} /> Export
</button>
