<script lang="ts">
  import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
  import { defaultKeymap, history, historyKeymap, indentWithTab, toggleComment } from "@codemirror/commands";
  import { MSSQL, MySQL, PostgreSQL, SQLite, sql, StandardSQL, type SQLNamespace } from "@codemirror/lang-sql";
  import { bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from "@codemirror/language";
  import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
  import { Compartment, EditorState, type Extension } from "@codemirror/state";
  import {
    drawSelection,
    EditorView,
    highlightActiveLine,
    highlightActiveLineGutter,
    keymap,
    lineNumbers,
    placeholder as placeholderExt,
  } from "@codemirror/view";
  import { tags as t } from "@lezer/highlight";
  import { untrack } from "svelte";
  import type { Driver } from "../../lib/types";

  interface Props {
    value?: string;
    driver?: Driver | null;
    schema?: SQLNamespace;
    defaultTable?: string;
    readonly?: boolean;
    placeholder?: string;
    fontSize?: number;
    onrun?: (sql: string) => void;
    onchange?: (sql: string) => void;
    onsave?: () => void;
  }

  let {
    value = "",
    driver = null,
    schema = {},
    readonly = false,
    placeholder = "",
    fontSize = 13,
    onrun,
    onchange,
    onsave,
  }: Props = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  const language = new Compartment();
  const editable = new Compartment();
  const theme = new Compartment();

  const dialects = { postgres: PostgreSQL, mysql: MySQL, sqlite: SQLite, mssql: MSSQL };

  const highlight = HighlightStyle.define([
    { tag: t.keyword, color: "var(--cm-keyword)", fontWeight: "600" },
    { tag: [t.string, t.special(t.string)], color: "var(--cm-string)" },
    { tag: [t.number, t.bool, t.null], color: "var(--cm-number)" },
    { tag: [t.lineComment, t.blockComment], color: "var(--cm-comment)", fontStyle: "italic" },
    { tag: [t.typeName, t.standard(t.name)], color: "var(--cm-type)" },
    { tag: [t.operator, t.punctuation], color: "var(--muted)" },
    { tag: t.special(t.name), color: "var(--cm-ident)" },
  ]);

  function themeFor(size: number): Extension {
    return EditorView.theme({
      "&": { height: "100%", fontSize: `${size}px`, backgroundColor: "var(--surface)", color: "var(--ink)" },
      ".cm-scroller": { fontFamily: "var(--font-code)", lineHeight: "1.6" },
      ".cm-content": { caretColor: "var(--accent)", padding: "10px 0" },
      ".cm-gutters": { backgroundColor: "var(--surface)", color: "var(--faint)", border: "none", paddingLeft: "6px" },
      ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--ink-2)" },
      ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--accent-soft) 45%, transparent)" },
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
        backgroundColor: "var(--accent-soft) !important",
      },
      ".cm-selectionMatch": { backgroundColor: "color-mix(in srgb, var(--edited) 70%, transparent)" },
      ".cm-cursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
      "&.cm-focused": { outline: "none" },
      ".cm-tooltip": {
        backgroundColor: "var(--raised)",
        border: "1px solid var(--rule-strong)",
        borderRadius: "6px",
        boxShadow: "var(--shadow-pop)",
        overflow: "hidden",
      },
      ".cm-tooltip-autocomplete > ul > li": { padding: "2px 10px", fontFamily: "var(--font-code)" },
      ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
        backgroundColor: "var(--accent)",
        color: "var(--accent-ink)",
      },
      ".cm-completionDetail": { color: "inherit", opacity: "0.6", fontStyle: "normal" },
      ".cm-panels": { backgroundColor: "var(--surface-2)", color: "var(--ink)", borderColor: "var(--rule)" },
      ".cm-panel.cm-search input, .cm-panel.cm-search button": { fontFamily: "var(--font-ui)", fontSize: "12px" },
      ".cm-matchingBracket": { backgroundColor: "var(--accent-soft)", outline: "1px solid var(--accent-rule)" },
      ".cm-placeholder": { color: "var(--faint)", fontStyle: "italic" },
    });
  }

  function languageFor(d: Driver | null, ns: SQLNamespace): Extension {
    return sql({ dialect: d ? dialects[d] : StandardSQL, schema: ns, upperCaseKeywords: true });
  }

  /** Selected text, or the whole document. */
  export function currentSql(): string {
    if (!view) return value;
    const { from, to } = view.state.selection.main;
    return from === to ? view.state.doc.toString() : view.state.sliceDoc(from, to);
  }

  export function getValue(): string {
    return view?.state.doc.toString() ?? value;
  }

  export function setValue(text: string) {
    view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  }

  export function insert(text: string) {
    if (!view) return;
    const { from, to } = view.state.selection.main;
    view.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length } });
    view.focus();
  }

  export function focus() {
    view?.focus();
  }

  const runKeys = keymap.of([
    {
      key: "Mod-Enter",
      preventDefault: true,
      run: () => {
        onrun?.(currentSql());
        return true;
      },
    },
    {
      key: "Mod-s",
      preventDefault: true,
      run: () => {
        onsave?.();
        return true;
      },
    },
    { key: "Mod-/", run: toggleComment },
  ]);

  // Created once; later prop changes go through the compartments below.
  $effect(() => {
    view = untrack(() => createView());
    return () => view?.destroy();
  });

  function createView(): EditorView {
    return new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          highlightActiveLineGutter(),
          highlightActiveLine(),
          history(),
          drawSelection(),
          indentOnInput(),
          bracketMatching(),
          closeBrackets(),
          highlightSelectionMatches(),
          search({ top: true }),
          autocompletion({ activateOnTyping: true, icons: false }),
          syntaxHighlighting(highlight),
          placeholderExt(placeholder),
          runKeys,
          keymap.of([
            ...closeBracketsKeymap,
            ...defaultKeymap,
            ...searchKeymap,
            ...historyKeymap,
            ...completionKeymap,
            indentWithTab,
          ]),
          language.of(languageFor(driver, schema)),
          editable.of([EditorState.readOnly.of(readonly), EditorView.editable.of(!readonly)]),
          theme.of(themeFor(fontSize)),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) onchange?.(u.state.doc.toString());
          }),
        ],
      }),
    });
  }

  $effect(() => {
    view?.dispatch({ effects: language.reconfigure(languageFor(driver, schema)) });
  });

  $effect(() => {
    view?.dispatch({ effects: theme.reconfigure(themeFor(fontSize)) });
  });

  $effect(() => {
    if (!view || !readonly) return;
    // Read-only views (DDL) follow their value.
    if (view.state.doc.toString() !== value) setValue(value);
  });
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 100%;
    min-height: 0;
    overflow: hidden;
    --cm-keyword: #2445d6;
    --cm-string: #1d7a4a;
    --cm-number: #b0560f;
    --cm-comment: #8a948f;
    --cm-type: #7a3fb0;
    --cm-ident: #9b5d00;
  }
  :global(:root[data-theme="dark"]) .editor {
    --cm-keyword: #8fa3ff;
    --cm-string: #79d39f;
    --cm-number: #f0a868;
    --cm-comment: #6f7a75;
    --cm-type: #c49bf0;
    --cm-ident: #e6c06a;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme="light"])) .editor {
      --cm-keyword: #8fa3ff;
      --cm-string: #79d39f;
      --cm-number: #f0a868;
      --cm-comment: #6f7a75;
      --cm-type: #c49bf0;
      --cm-ident: #e6c06a;
    }
  }
</style>
