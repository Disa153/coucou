// The glowing strip along the island (src/core/glow.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_GLOW, agentBusy, glowLook, readGlow, shiftHue, stateColor } from "../src/core/glow.ts";

const now = (o = {}) => ({ mode: "compact", state: "idle", busy: false, music: false, ...o });

test("off until it is turned on, as the island shipped", () => {
  assert.equal(readGlow(undefined).on, false);
  assert.equal(glowLook(readGlow(undefined), now()).visible, false);
});

test("whatever the file holds reads as a usable pick", () => {
  const g = readGlow({ on: true, place: "sideways", color: "blue", effect: 3, brightness: 9, thickness: 0.2, speed: -1, when: "never" });
  assert.deepEqual(g, { ...DEFAULT_GLOW, on: true, brightness: 1, thickness: 1, speed: 1 });
  assert.equal(readGlow({ color: "#ABCDEF" }).color, "#abcdef");
  assert.equal(readGlow({ color: "rainbow" }).color, "rainbow");
});

test("never drawn while the island is hidden (nothing may animate then)", () => {
  const on = { ...DEFAULT_GLOW, on: true, effect: "pulse" };
  assert.equal(glowLook(on, now({ mode: "hidden" })).visible, false);
  assert.equal(glowLook(on, now({ mode: "expanded" })).animation, "pulse");
});

test("only while an agent works, or only while music plays", () => {
  const working = { ...DEFAULT_GLOW, on: true, when: "working" };
  assert.equal(glowLook(working, now()).visible, false);
  assert.equal(glowLook(working, now({ busy: true })).visible, true);
  const music = { ...DEFAULT_GLOW, on: true, when: "music" };
  assert.equal(glowLook(music, now()).visible, false);
  assert.equal(glowLook(music, now({ music: true })).visible, true);
  assert.equal(agentBusy(["idle", "approval"]), true);
  assert.equal(agentBusy(["idle", "finished", "sleeping"]), false);
});

test("Mochi's state lends its colour when asked to", () => {
  const g = { ...DEFAULT_GLOW, on: true, byState: true, color: "rainbow" };
  assert.equal(glowLook(g, now({ state: "approval" })).paint, "#f5a524");
  assert.equal(glowLook(g, now({ state: "error" })).paint, "#f4505e");
  assert.equal(glowLook(g, now({ state: "finished" })).paint, "#22c55e");
  assert.equal(glowLook(g, now({ state: "working" })).animation, "rainbow");
  assert.equal(stateColor("idle"), null);
  // Without the switch, the pick stays.
  assert.equal(glowLook({ ...g, byState: false, color: "#7c5cff" }, now({ state: "error" })).paint, "#7c5cff");
});

test("the effects", () => {
  const g = { ...DEFAULT_GLOW, on: true, color: "#ff0000" };
  assert.equal(glowLook({ ...g, effect: "steady" }, now()).animation, "none");
  assert.match(glowLook({ ...g, effect: "gradient" }, now()).paint, /^linear-gradient\(90deg,#ff0000,#/);
  assert.equal(glowLook({ ...g, effect: "beat" }, now()).animation, "none");
  assert.equal(glowLook({ ...g, effect: "beat" }, now({ music: true })).animation, "beat");
});

test("hues turn on the colour wheel", () => {
  assert.equal(shiftHue("#ff0000", 120), "#00ff00");
  assert.equal(shiftHue("#ff0000", 360), "#ff0000");
  assert.equal(shiftHue("#808080", 90), "#808080");
  assert.equal(shiftHue("nope", 30), "nope");
});
