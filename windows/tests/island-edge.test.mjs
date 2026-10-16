// Where the island sits in its window on each screen edge (src/core/state.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import { islandPlacement } from "../src/core/state.ts";

test("on top, as shipped: centred, the top corners flat", () => {
  assert.deepEqual(islandPlacement("top", 720, 320, 200, 40, 14), { x: 260, y: 0, radius: "0 0 14px 14px" });
  // no edge saved yet is the top
  assert.deepEqual(islandPlacement(undefined, 720, 320, 200, 40, 14), islandPlacement("top", 720, 320, 200, 40, 14));
});

test("on the bottom it sits on the window's floor, the bottom corners flat", () => {
  assert.deepEqual(islandPlacement("bottom", 720, 320, 200, 40, 14), { x: 260, y: 280, radius: "14px 14px 0 0" });
});

test("on a side it is against that side, centred up and down", () => {
  assert.deepEqual(islandPlacement("left", 720, 320, 200, 40, 14), { x: 0, y: 140, radius: "0 14px 14px 0" });
  assert.deepEqual(islandPlacement("right", 720, 320, 200, 40, 14), { x: 520, y: 140, radius: "14px 0 0 14px" });
});
