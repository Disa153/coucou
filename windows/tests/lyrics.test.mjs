// The Lyrics view's lines (src/core/lyrics.ts). Placeholder words only.

import { test } from "node:test";
import assert from "node:assert/strict";
import { lineAt, parseLrc, plainLines } from "../src/core/lyrics.ts";

test("timed lines, in order, tags skipped", () => {
  const lrc = "[ar:Someone]\n[00:12.50] line two\n[00:01.00]line one\n[01:02.345] line three\n\n[00:20.00]";
  assert.deepEqual(parseLrc(lrc), [
    { t: 1, text: "line one" },
    { t: 12.5, text: "line two" },
    { t: 20, text: "" },
    { t: 62.345, text: "line three" },
  ]);
});

test("a line with several stamps is sung at each", () => {
  assert.deepEqual(parseLrc("[00:05.00][00:30.00] chorus").map((l) => l.t), [5, 30]);
});

test("the line sung at a second", () => {
  const lines = parseLrc("[00:01.00]a\n[00:05.00]b\n[00:09.00]c");
  assert.equal(lineAt(lines, 0.5), -1);
  assert.equal(lineAt(lines, 1), 0);
  assert.equal(lineAt(lines, 7), 1);
  assert.equal(lineAt(lines, 100), 2);
  assert.equal(lineAt([], 3), -1);
});

test("plain words as lines", () => {
  assert.deepEqual(plainLines("\n\n a \n\n b\n\n"), ["a", "", "b"]);
});
