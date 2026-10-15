// The overview's pills show what a busy agent is doing (src/core/pill-status.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import { pillStatus } from "../src/core/pill-status.ts";
import { makeDiffStep } from "../src/core/diff.ts";

test("a working agent shows its latest step", () => {
  assert.equal(pillStatus({ state: "working", steps: ["Read · a.ts", "Bash · npm test"] }), "Bash · npm test");
  assert.equal(pillStatus({ state: "thinking", steps: ["Fix the login bug"] }), "Fix the login bug");
  assert.equal(pillStatus({ state: "searching", steps: ["Search · coucou"] }), "Search · coucou");
});

test("an agent that is not busy shows its name", () => {
  for (const state of ["idle", "finished", "error", "approval", "question", "ratelimit", "sleeping", "dizzy"]) {
    assert.equal(pillStatus({ state, steps: ["Bash · npm test"] }), null, state);
  }
});

test("no step, or a blank one, keeps the name", () => {
  assert.equal(pillStatus({ state: "working", steps: [] }), null);
  assert.equal(pillStatus({ state: "working", steps: ["   \n "] }), null);
});

test("an edit shows the file and its line counts, never the raw marker", () => {
  const status = pillStatus({ state: "working", steps: [makeDiffStep("main.ts", 3, 1, 7)] });
  assert.equal(status, "main.ts +3 −1");
});

test("a multi-line or very long step becomes one short line", () => {
  assert.equal(pillStatus({ state: "working", steps: ["first line\n  second"] }), "first line second");
  const long = pillStatus({ state: "working", steps: ["x".repeat(200)] });
  assert.equal(long.length, 80);
  assert.ok(long.endsWith("…"));
});
