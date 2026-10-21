import { describe, expect, it } from "vitest";
import { findColumns } from "./columns";

const names = ["RoomNo", "Suite", "RoomSuite", "RoomType", "IsRoomSuiteMaster", "Floor"];
const indexes = (q: string) => findColumns(names, q).map((m) => m.index);

describe("findColumns", () => {
  it("lists every column in table order for an empty query", () => {
    expect(indexes("  ")).toEqual([0, 1, 2, 3, 4, 5]);
  });

  it("ignores case and ranks exact, then prefix, then contained matches", () => {
    expect(indexes("roomsuite")).toEqual([2, 4]);
    expect(indexes("suite")).toEqual([1, 2, 4]);
    expect(indexes("room")).toEqual([0, 2, 3, 4]);
  });

  it("matches across spaces, underscores and dashes", () => {
    expect(indexes("room suite")).toEqual([2, 4]);
    expect(findColumns(["room_suite", "notes"], "RoomSuite").map((m) => m.index)).toEqual([0]);
  });

  it("reports where a plain match sits so it can be highlighted", () => {
    expect(findColumns(names, "suite")[1]).toEqual({ index: 2, start: 4, length: 5 });
    expect(findColumns(names, "room suite")[0]).toEqual({ index: 2, start: -1, length: 0 });
  });

  it("returns nothing when no column matches", () => {
    expect(findColumns(names, "balance")).toEqual([]);
  });
});
