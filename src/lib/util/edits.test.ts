import { describe, expect, it } from "vitest";
import {
  addInsert,
  cellValue,
  editCount,
  emptyEdits,
  removeInsert,
  setCell,
  setInsertCell,
  toRowChanges,
  toggleDelete,
} from "./edits";

const columns = ["id", "name", "email"];
const rows = [
  ["1", "Ana", "ana@x.io"],
  ["2", "Budi", null],
  ["3", "Citra", "c@x.io"],
];

describe("grid edits", () => {
  it("records a changed cell and forgets it when set back to the original", () => {
    const edited = setCell(emptyEdits(), 1, "name", "Bud", "Budi");
    expect(cellValue(edited, 1, "name", "Budi")).toBe("Bud");
    expect(editCount(edited)).toBe(1);

    const reverted = setCell(edited, 1, "name", "Budi", "Budi");
    expect(editCount(reverted)).toBe(0);
    expect(reverted.updates).toEqual({});
  });

  it("never mutates its input", () => {
    const base = emptyEdits();
    const frozen = Object.freeze({ ...base, updates: Object.freeze({}) });
    expect(() => setCell(frozen, 0, "name", "x", "Ana")).not.toThrow();
    expect(frozen.updates).toEqual({});
  });

  it("distinguishes NULL from empty string", () => {
    const edited = setCell(emptyEdits(), 1, "email", "", null);
    expect(editCount(edited)).toBe(1);
    expect(cellValue(edited, 1, "email", null)).toBe("");
  });

  it("builds changes keyed by the original primary key", () => {
    let e = setCell(emptyEdits(), 0, "id", "10", "1"); // editing the key itself
    e = setCell(e, 0, "name", "Ani", "Ana");
    e = toggleDelete(e, 2);
    e = addInsert(e, "n1");
    e = setInsertCell(e, "n1", "name", "Dewi");

    expect(toRowChanges(e, rows, columns, ["id"])).toEqual([
      { type: "update", key: [["id", "1"]], values: [["id", "10"], ["name", "Ani"]] },
      { type: "delete", key: [["id", "3"]] },
      { type: "insert", values: [["name", "Dewi"]] },
    ]);
  });

  it("drops updates for rows that are also deleted", () => {
    let e = setCell(emptyEdits(), 1, "name", "x", "Budi");
    e = toggleDelete(e, 1);
    expect(toRowChanges(e, rows, columns, ["id"])).toEqual([{ type: "delete", key: [["id", "2"]] }]);
    expect(editCount(e)).toBe(1);
    expect(editCount(toggleDelete(e, 1))).toBe(1); // undelete brings the update back
  });

  it("removes a pending insert", () => {
    const e = removeInsert(addInsert(emptyEdits(), "a"), "a");
    expect(editCount(e)).toBe(0);
  });
});
