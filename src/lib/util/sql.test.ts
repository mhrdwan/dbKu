import { describe, expect, it } from "vitest";
import { equalsPredicate, quoteIdent, quoteLiteral } from "./sql";

describe("client-side quoting", () => {
  it("quotes identifiers per engine", () => {
    expect(quoteIdent("postgres", 'a"b')).toBe('"a""b"');
    expect(quoteIdent("mysql", "a`b")).toBe("`a``b`");
    expect(quoteIdent("mssql", "a]b")).toBe("[a]]b]");
  });

  it("escapes literals, including MySQL backslashes", () => {
    expect(quoteLiteral("postgres", "O'Neil")).toBe("'O''Neil'");
    expect(quoteLiteral("mysql", "a\\'")).toBe("'a\\\\'''");
    expect(quoteLiteral("mssql", "x")).toBe("N'x'");
  });

  it("builds equality filters, with IS NULL for NULL", () => {
    expect(equalsPredicate("sqlite", "name", null)).toBe('"name" IS NULL');
    expect(equalsPredicate("mysql", "id", "7")).toBe("`id` = '7'");
  });
});
