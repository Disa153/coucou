// The volume panel view (src/views/volume.ts): when it reads the mixer, what it
// lists, and what a slider, the mute button, the wheel and the presets send to
// src-tauri/src/volume.rs. The rules behind it are in volume.test.mjs.

import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { calls, internals, sent } from "./tauri.mjs";
import { installFakeDom } from "./fakedom.mjs";

installFakeDom();
globalThis.addEventListener ??= () => {};

// Polling is driven by hand: an interval is only recorded.
const intervals = new Map();
let nextId = 1;
globalThis.setInterval = (fn, ms) => {
  intervals.set(nextId, { fn, ms });
  return nextId++;
};
globalThis.clearInterval = (id) => intervals.delete(id);

// The view subscribes to State for good, so one view serves every test; time moves
// on between tests so that one test's "just moved" never leaks into the next.
const realNow = Date.now;
let skew = 0;
Date.now = () => realNow() + skew;

const { buildVolume } = await import("../src/views/volume.ts");
const { DEFAULT_SETTINGS, State } = await import("../src/core/state.ts");
const { DEFAULT_VOLUME_PREFS } = await import("../src/core/volume.ts");

let mixer;
let reads = 0;
const original = internals.invoke;
internals.invoke = async (cmd, args) => {
  if (cmd === "volume_state") {
    reads++;
    return structuredClone(mixer);
  }
  return original(cmd, args);
};

const app = (id, name, volume, over = {}) => ({ id, name, volume, muted: false, playing: true, system: false, ...over });
const freshMixer = () => ({
  supported: true, master: 45, masterMuted: false,
  apps: [
    app("chrome", "Chrome", 70),
    app("spotify", "Spotify", 55),
    app("discord", "Discord", 100, { playing: false }),
    app("system", "", 60, { system: true }),
  ],
});

const settle = async () => { for (let i = 0; i < 5; i++) await new Promise((r) => setImmediate(r)); };
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const view = buildVolume();

/** The view on screen, its first reading done. */
async function open(prefs = {}) {
  State.settings = { ...DEFAULT_SETTINGS, volumePanel: { ...DEFAULT_VOLUME_PREFS, ...prefs } };
  view.sync();
  State.mode = "expanded";
  State.view = "volume";
  State.notify();
  await settle();
  return view;
}

const names = (view) => view.el.find(".vol-name").map((e) => e.textContent);
const percents = (view) => view.el.find(".vol-pc").map((e) => e.textContent);
const slider = (view, i) => view.el.find("INPUT")[i];
const type = (input, value) => {
  input.value = String(value);
  input.fire("input");
};

beforeEach(() => {
  mixer = freshMixer();
  reads = 0;
  intervals.clear();
  State.mode = "hidden";
  State.view = "overview";
  State.notify();
  calls.length = 0;
  skew += 10_000;
});

test("nothing is read while the panel is not on screen", async () => {
  State.mode = "hidden";
  State.notify();
  await settle();
  assert.equal(reads, 0);
  assert.equal(intervals.size, 0);
});

test("on screen it reads the mixer now and then every second, and stops when it is left", async () => {
  await open();
  assert.equal(reads, 1);
  assert.equal(intervals.size, 1);
  assert.equal([...intervals.values()][0].ms, 1000);
  State.view = "overview";
  State.notify();
  assert.equal(intervals.size, 0);
});

test("it lists the master volume and the apps playing, not the idle ones or Windows' sounds", async () => {
  const view = await open();
  assert.deepEqual(names(view), ["Windows volume", "Chrome", "Spotify"]);
  assert.deepEqual(percents(view), ["45%", "70%", "55%"]);
});

test("the preferences decide what is listed", async () => {
  const view = await open({ onlyPlaying: false, hideSystem: false, showPercent: false });
  assert.deepEqual(names(view), ["Windows volume", "Chrome", "Spotify", "Discord", "System sounds"]);
  assert.ok(view.el.find(".vol-pc").every((e) => e.style.display === "none"));
});

test("a slider shows its level at once and sends it", async () => {
  const view = await open();
  type(slider(view, 1), 40);
  assert.equal(percents(view)[1], "40%");
  await wait(150);
  assert.deepEqual(sent("volume_set"), [{ target: "app:chrome", value: 40 }]);
});

test("the master slider sets the Windows volume", async () => {
  const view = await open();
  type(slider(view, 0), 30);
  await wait(150);
  assert.deepEqual(sent("volume_set"), [{ target: "master", value: 30 }]);
});

test("the limit stops a slider, and no level above it is sent", async () => {
  const view = await open({ limit: 80 });
  assert.equal(slider(view, 1).max, "80");
  type(slider(view, 1), 95);
  await wait(150);
  assert.deepEqual(sent("volume_set"), [{ target: "app:chrome", value: 80 }]);
});

test("a notch of the wheel moves a slider by the step and sends it at once", async () => {
  const view = await open({ wheelStep: 10 });
  const input = slider(view, 1);
  let prevented = false;
  for (const fn of input.listeners.get("wheel")) fn({ deltaY: -120, preventDefault: () => (prevented = true) });
  assert.ok(prevented, "the wheel moves the slider, not the page");
  assert.equal(percents(view)[1], "80%");
  assert.deepEqual(sent("volume_set"), [{ target: "app:chrome", value: 80 }]);
});

test("the mute button mutes an app and marks its row", async () => {
  const view = await open();
  view.el.find(".vol-mute")[1].fire("click");
  assert.deepEqual(sent("volume_mute"), [{ target: "app:chrome", muted: true }]);
  assert.ok(view.el.find(".vol-row")[1].classList.contains("muted"));
  view.el.find(".vol-mute")[1].fire("click");
  assert.deepEqual(sent("volume_mute").at(-1), { target: "app:chrome", muted: false });
});

test("the Night preset lowers the master volume and every app, the idle ones too", async () => {
  const view = await open();
  const night = view.el.find(".vol-chip")[0];
  night.fire("click");
  await settle();
  const sets = Object.fromEntries(sent("volume_set").map((a) => [a.target, a.value]));
  assert.deepEqual(sets, { master: 20, "app:chrome": 25, "app:spotify": 25, "app:discord": 25 });
  assert.deepEqual(percents(view), ["20%", "25%", "25%"]);
});

test("what the mixer says is not shown for a moment after the user moved something", async () => {
  const view = await open();
  type(slider(view, 1), 40);
  mixer.apps[0].volume = 10;
  await [...intervals.values()][0].fn();
  await settle();
  assert.equal(percents(view)[1], "40%");
});

test("a mixer change made outside shows on the next reading", async () => {
  const view = await open();
  mixer.apps[0].volume = 33;
  mixer.master = 12;
  await [...intervals.values()][0].fn();
  await settle();
  assert.deepEqual(percents(view), ["12%", "33%", "55%"]);
});

test("a mixer that is not there shows nothing and never throws", async () => {
  mixer = { supported: false, master: 0, masterMuted: false, apps: [] };
  const view = await open();
  assert.deepEqual(names(view), ["Windows volume"]);
});
