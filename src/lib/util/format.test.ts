import { describe, expect, it } from "vitest";
import { displayValue, estimateWidth, formatMs, isNumericType, tryPrettyJson } from "./format";

describe("format", () => {
  it("recognises numeric column types across engines", () => {
    for (const t of ["INT4", "integer", "bigint", "numeric(10,2)", "DECIMAL", "float8", "money", "i32", "f64", "tinyint(1)"]) {
      expect(isNumericType(t), t).toBe(true);
    }
    for (const t of ["interval", "text", "varchar(20)", "point", "timestamp", "intarray_x"]) {
      expect(isNumericType(t), t).toBe(false);
    }
  });

  it("shows NULL and flattens newlines", () => {
    expect(displayValue(null)).toBe("NULL");
    expect(displayValue("a\nb")).toBe("a ⏎ b");
    expect(displayValue("x".repeat(500)).length).toBe(401);
  });

  it("formats durations", () => {
    expect(formatMs(12)).toBe("12 ms");
    expect(formatMs(1234)).toBe("1.23 s");
    expect(formatMs(125_000)).toBe("2 min 5 s");
  });

  it("keeps column widths within bounds", () => {
    expect(estimateWidth("id", ["1", "2"])).toBe(72);
    expect(estimateWidth("description", ["x".repeat(1000)])).toBe(360);
  });

  it("pretty-prints JSON only when it parses", () => {
    expect(tryPrettyJson('{"a":1}')).toBe('{\n  "a": 1\n}');
    expect(tryPrettyJson("{nope")).toBeNull();
    expect(tryPrettyJson("plain")).toBeNull();
  });
});
