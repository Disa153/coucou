// The volume panel's rules (src/core/volume.ts): the preferences read from the
// settings file, levels and the mouse wheel, which apps are listed, the presets and
// the colours. The Core Audio side is in src-tauri/src/volume.rs.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_VOLUME_PREFS, appColor, clampLevel, presetLevels, sanitizePrefs, visibleApps, wheelLevel,
} from "../src/core/volume.ts";

const app = (id, volume, over = {}) => ({ id, name: id, volume, muted: false, playing: true, system: false, ...over });
const state = (apps, master = 60) => ({ supported: true, master, masterMuted: false, apps });

test("a settings file with nothing about the panel gives the defaults", () => {
  assert.deepEqual(sanitizePrefs(undefined), DEFAULT_VOLUME_PREFS);
  assert.deepEqual(sanitizePrefs(null), DEFAULT_VOLUME_PREFS);
  assert.deepEqual(sanitizePrefs("loud"), DEFAULT_VOLUME_PREFS);
  assert.deepEqual(sanitizePrefs({}), DEFAULT_VOLUME_PREFS);
});

test("what is usable is kept and what is not takes its default", () => {
  const prefs = sanitizePrefs({
    size: "large", color: "#22c55e", showPercent: false, showDots: "yes", onlyPlaying: false,
    hideSystem: false, presets: false, wheelStep: 10, limit: 80,
  });
  assert.equal(prefs.size, "large");
  assert.equal(prefs.color, "#22C55E");
  assert.equal(prefs.showPercent, false);
  assert.equal(prefs.showDots, true, "a wrong type takes the default");
  assert.equal(prefs.onlyPlaying, false);
  assert.equal(prefs.wheelStep, 10);
  assert.equal(prefs.limit, 80);
});

test("a size, step, colour or limit nobody offers is not used", () => {
  const prefs = sanitizePrefs({ size: "huge", color: "green", wheelStep: 3, limit: 5 });
  assert.equal(prefs.size, "normal");
  assert.equal(prefs.color, "");
  assert.equal(prefs.wheelStep, 5);
  assert.equal(prefs.limit, 20, "below the lowest limit, the lowest");
  assert.equal(sanitizePrefs({ limit: 400 }).limit, 100);
  assert.equal(sanitizePrefs({ limit: Number.NaN }).limit, 100);
  assert.equal(sanitizePrefs({ limit: 64.6 }).limit, 65);
});

test("a level is whole and never past the limit", () => {
  assert.equal(clampLevel(45.4, 100), 45);
  assert.equal(clampLevel(150, 80), 80);
  assert.equal(clampLevel(-3, 80), 0);
  assert.equal(clampLevel(Number.NaN, 80), 0);
});

test("a notch of the wheel moves the level by the step, up or down, inside 0…limit", () => {
  assert.equal(wheelLevel(50, -100, 5, 100), 55);
  assert.equal(wheelLevel(50, 100, 5, 100), 45);
  assert.equal(wheelLevel(98, -100, 5, 100), 100);
  assert.equal(wheelLevel(78, -100, 5, 80), 80, "the limit stops it");
  assert.equal(wheelLevel(2, 100, 5, 100), 0);
  assert.equal(wheelLevel(50, 0, 5, 100), 50);
  assert.equal(wheelLevel(50, -120, 1, 100), 51);
});

test("the list follows only-playing and hide-Windows-sounds", () => {
  const apps = [
    app("chrome", 70),
    app("discord", 100, { playing: false }),
    app("system", 60, { system: true }),
  ];
  const ids = (prefs) => visibleApps(apps, { ...DEFAULT_VOLUME_PREFS, ...prefs }).map((a) => a.id);
  assert.deepEqual(ids({}), ["chrome"]);
  assert.deepEqual(ids({ onlyPlaying: false }), ["chrome", "discord"]);
  assert.deepEqual(ids({ hideSystem: false }), ["chrome", "system"]);
  assert.deepEqual(ids({ onlyPlaying: false, hideSystem: false }), ["chrome", "discord", "system"]);
});

test("Night lowers everything and never raises anything", () => {
  const out = presetLevels("night", state([app("chrome", 70), app("spotify", 10), app("system", 60, { system: true })], 60), 100);
  assert.equal(out.master, 20);
  assert.deepEqual(out.apps, { chrome: 25, spotify: 10 });
  assert.equal(presetLevels("night", state([], 12), 100).master, 12, "already quieter than 20");
});

test("Game puts an unknown program at full, keeps voice audible and drops music", () => {
  const out = presetLevels("game", state([app("somegame", 30), app("discord", 100), app("spotify", 55), app("chrome", 70)]), 100);
  assert.deepEqual(out.apps, { somegame: 100, discord: 70, spotify: 15, chrome: 15 });
  assert.equal(out.master, undefined, "the master volume is not touched");
});

test("Call puts voice apps at full and the rest low", () => {
  const out = presetLevels("call", state([app("discord", 40), app("zoom", 20), app("spotify", 55), app("somegame", 90)]), 100);
  assert.deepEqual(out.apps, { discord: 100, zoom: 100, spotify: 10, somegame: 10 });
});

test("a preset never goes past the limit, and leaves Windows' own sounds alone", () => {
  const out = presetLevels("game", state([app("somegame", 30), app("system", 60, { system: true })]), 80);
  assert.deepEqual(out.apps, { somegame: 80 });
});

test("an app has the same colour every time, a known one its brand's", () => {
  assert.equal(appColor("spotify"), "#1DB954");
  assert.equal(appColor("system"), "#888780");
  assert.equal(appColor("some-game"), appColor("some-game"));
  assert.match(appColor("some-game"), /^#[0-9A-F]{6}$/);
});
