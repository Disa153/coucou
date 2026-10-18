// Feeding Mochi and the moonwalk (src/core/pet.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DANCE_LOOP, DEFAULT_PET, SONGS, beatAt, canMoonwalk, danceAt, feed, fullNow, hungry, pickSong, readPet, songPlaying,
  treatsLeft,
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

test("the dance, beat by beat, round after round", () => {
  assert.equal(danceAt(0).move, "moonwalk");
  assert.deepEqual(danceAt(8), { move: "moonwalk", k: 0.5, whole: 8, frac: 0, round: 0 });
  assert.equal(danceAt(16.5).move, "spin");
  assert.equal(danceAt(19).move, "toes");
  assert.equal(danceAt(21).move, "lean");
  assert.equal(danceAt(30).move, "kick");
  assert.equal(danceAt(40).move, "strut");
  assert.equal(danceAt(DANCE_LOOP + 1).move, "moonwalk");
  assert.equal(danceAt(DANCE_LOOP + 1).round, 1);
  assert.equal(danceAt(-3).whole, 0);
});

test("the beat follows the song's tempo", () => {
  assert.equal(beatAt(60, 117), 117);
  assert.equal(beatAt(-1, 117), 0);
  for (const s of Object.values(SONGS)) assert.ok(s.bpm > 90 && s.bpm < 160, s.title);
});

test("the song plays when Spotify plays it by that name", () => {
  const song = SONGS.billieJean;
  assert.equal(songPlaying({ playing: true, track: { title: "Billie Jean" } }, song), true);
  assert.equal(songPlaying({ playing: true, track: { title: "Billie Jean - Single Version" } }, song), true);
  assert.equal(songPlaying({ playing: false, track: { title: "Billie Jean" } }, song), false);
  assert.equal(songPlaying({ playing: true, track: { title: "Thriller" } }, song), false);
  assert.equal(songPlaying({ playing: true, track: null }, song), false);
});
