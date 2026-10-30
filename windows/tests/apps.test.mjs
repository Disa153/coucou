// The app panel (src/views/apps.ts, src-tauri/src/apps): what the island opens
// on when no agent is at work, and the panel's two views.

import { afterEach, test } from "node:test";
import assert from "node:assert/strict";
import { State } from "../src/core/state.ts";
import { VIEW_LAYOUTS } from "../src/core/layout.ts";

afterEach(() => {
  State.appsWhenIdle = false;
});

test("with nothing at work the island opens on the app panel when the user keeps it on", () => {
  State.tasks = [];
  State.appsWhenIdle = false;
  assert.equal(State.defaultView(), "empty");
  State.appsWhenIdle = true;
  assert.equal(State.defaultView(), "apps");
});

test("an agent at work still comes first", () => {
  State.appsWhenIdle = true;
  State.tasks = [{ id: "integration_claude" }];
  assert.equal(State.defaultView(), "overview");
  State.tasks = [];
});

test("both views have a place in the island", () => {
  assert.ok(VIEW_LAYOUTS.apps.height >= 140 && VIEW_LAYOUTS.apps.height <= 320);
  assert.ok(VIEW_LAYOUTS.appsEdit.height <= 320);
});
