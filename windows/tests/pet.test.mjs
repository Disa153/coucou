// Feeding Mochi and the moonwalk (src/core/pet.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_PET, SONGS, canMoonwalk, feed, fullNow, hungry, moonwalkAt, MOONWALK_S, pickSong, readPet, treatsLeft,
} from "../src/core/pet.ts";

const H = 3_600_000;

test("whatever the file holds reads as a usable state", () => {
  assert.deepEqual(readPet(undefined), DEFAULT_PET);
  const p = readPet({ full: 400, at: -5, treats: 2.7, hunger: "yes", music: false, song: "thriller", after: "keep" });
  assert.deepEqual(p, { ...DEFAULT_PET, full: 100, at: 0, treats: 2, music: false, after: "keep" });
  assert.equal(readPet({ song: "random" }).song, "random");
});

test("he slowly gets hungry, unless hunger is off", () => {
  const p = { ...DEFAULT_PET, full: 80, at: 1000 };
  assert.equal(fullNow(p, 1000), 80);
  assert.equal(Math.round(fullNow(p, 1000 + 6 * H)), 30);
  assert.equal(fullNow(p, 1000 + 48 * H), 0);
  assert.equal(hungry(p, 1000 + 8 * H), true);
  assert.equal(fullNow({ ...p, hunger: false }, 1000 + 48 * H), 80);
  // never fed: as stored
  assert.equal(fullNow({ ...DEFAULT_PET }, 48 * H), DEFAULT_PET.full);
});

test("each snack fills him up and counts toward the moonwalk", () => {
  let p = { ...DEFAULT_PET, full: 95, at: 1 };
  p = feed(p, 1);
  assert.equal(p.full, 100);
  assert.equal(p.treats, 1);
  assert.equal(canMoonwalk(p), false);
  assert.equal(treatsLeft(p), 4);
  for (let i = 0; i < 4; i++) p = feed(p, 2);
  assert.equal(canMoonwalk(p), true);
  assert.equal(treatsLeft(p), 0);
});

test("the song, picked or random", () => {
  assert.equal(pickSong({ ...DEFAULT_PET, song: "bad" }, 0.9).title, "Bad");
  assert.equal(pickSong({ ...DEFAULT_PET, song: "random" }, 0).title, "Billie Jean");
  assert.equal(pickSong({ ...DEFAULT_PET, song: "random" }, 0.999).title, "Beat It");
  for (const s of Object.values(SONGS)) assert.match(s.id, /^[A-Za-z0-9]{22}$/);
});

test("the moonwalk goes back, spins, stands on its toes and walks home", () => {
  assert.deepEqual(moonwalkAt(2.1), { phase: "glide", back: 0.5, spin: 0 });
  assert.equal(moonwalkAt(4.6).phase, "spin");
  assert.equal(moonwalkAt(5.5).phase, "toes");
  assert.equal(moonwalkAt(7).phase, "home");
  assert.ok(moonwalkAt(7).back < 1 && moonwalkAt(7).back > 0);
  assert.equal(moonwalkAt(MOONWALK_S + 0.1).phase, "done");
});
