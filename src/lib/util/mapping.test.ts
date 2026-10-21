import { describe, expect, it } from "vitest";
import { autoMap } from "./mapping";

describe("autoMap", () => {
  it("matches headers by name, ignoring case and punctuation", () => {
    expect(autoMap(["id", "full_name", "email", "created_at"], ["Full Name", "ID", "E-mail", "notes"], true)).toEqual([
      1, 0, 2, -1,
    ]);
  });

  it("uses the first of duplicate headers", () => {
    expect(autoMap(["a"], ["A", "a"], true)).toEqual([0]);
  });

  it("maps by position without a header", () => {
    expect(autoMap(["a", "b", "c"], ["Column 1", "Column 2"], false)).toEqual([0, 1, -1]);
  });
});
