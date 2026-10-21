import { describe, expect, it } from "vitest";
import { toClipboardTsv, toDelimited, toJson, toMarkdown } from "./export";

const cols = ["id", "note"];
const rows = [
  ["1", 'say "hi", ok'],
  ["2", null],
  ["3", "line\nbreak"],
];

describe("exports", () => {
  it("quotes CSV fields only when needed", () => {
    expect(toDelimited(cols, rows)).toBe('id,note\r\n1,"say ""hi"", ok"\r\n2,\r\n3,"line\nbreak"\r\n');
  });

  it("writes NULL as JSON null", () => {
    expect(JSON.parse(toJson(cols, rows))[1]).toEqual({ id: "2", note: null });
  });

  it("flattens tabs and newlines for the clipboard", () => {
    expect(toClipboardTsv([["a\tb", "c\nd"], [null, "e"]])).toBe("a b\tc d\n\te");
  });

  it("escapes pipes in markdown and shows NULL", () => {
    expect(toMarkdown(["a"], [["x|y"], [null]])).toBe("| a |\n| --- |\n| x\\|y |\n| NULL |\n");
  });
});
