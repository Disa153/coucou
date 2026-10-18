// Feeding Mochi (the island's Kitchen, views/kitchen.ts) and what he learns:
// after MOONWALK_TREATS snacks he can moonwalk, to a Michael Jackson song in
// Spotify if the user wants. Stored in settings.json under "pet"
// (src-tauri/src/settings.rs); everything here is pure, so it is tested.

export type SongId = "billieJean" | "smoothCriminal" | "bad" | "beatIt";
export type SongPick = SongId | "random";

export interface PetPref {
  /** 0…100 at `at`. */
  full: number;
  /** Unix ms of `full` (0: never fed). */
  at: number;
  /** Snacks eaten, all time. */
  treats: number;
  hunger: boolean;
  music: boolean;
  song: SongPick;
  after: "pause" | "keep";
}

export const DEFAULT_PET: PetPref = {
  full: 60, at: 0, treats: 0, hunger: true, music: true, song: "billieJean", after: "pause",
};

/** Snacks before he can moonwalk. */
export const MOONWALK_TREATS = 5;
/** What one snack adds. */
export const SNACK_FULL = 12;
/** From full to empty, when he gets hungry. */
export const HUNGER_HOURS = 12;
/** Below this he shows a little 🍙 now and then. */
export const HUNGRY_BELOW = 25;

/** The moonwalk's songs on Spotify (track ids). */
/** The moonwalk's songs on Spotify (track ids) and their tempo, so he steps on the beat. */
export interface Song { title: string; id: string; bpm: number }
export const SONGS: Record<SongId, Song> = {
  billieJean: { title: "Billie Jean", id: "7J1uxwnxfQLu4APicE5Rnj", bpm: 117 },
  smoothCriminal: { title: "Smooth Criminal", id: "5T7ywazdGIydr6JCW6t02j", bpm: 118 },
  bad: { title: "Bad", id: "6JkYFfx8ucsKXFhbUXkzie", bpm: 114 },
  beatIt: { title: "Beat It", id: "3BovdzfaX4jb5KFQwoPfAw", bpm: 139 },
};
/** His own count when no song plays. */
export const OWN_BPM = 116;

export const SNACKS = ["🍙", "🍪", "🍓", "🍣", "🍩"] as const;

const SONG_IDS = Object.keys(SONGS) as SongId[];

function num(v: unknown, min: number, max: number, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback;
}

/** Whatever settings.json holds as a usable state. */
export function readPet(raw: unknown): PetPref {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const d = DEFAULT_PET;
  return {
    full: num(o.full, 0, 100, d.full),
    at: num(o.at, 0, Number.MAX_SAFE_INTEGER, d.at),
    treats: Math.floor(num(o.treats, 0, 1e9, d.treats)),
    hunger: typeof o.hunger === "boolean" ? o.hunger : d.hunger,
    music: typeof o.music === "boolean" ? o.music : d.music,
    song: o.song === "random" || SONG_IDS.includes(o.song as SongId) ? (o.song as SongPick) : d.song,
    after: o.after === "keep" ? "keep" : "pause",
  };
}

/** How full he is now: it slowly drops from `at` when hunger is on. */
export function fullNow(p: PetPref, nowMs: number): number {
  if (!p.hunger || p.at <= 0) return p.full;
  const hours = Math.max(0, nowMs - p.at) / 3_600_000;
  return Math.max(0, p.full - (hours / HUNGER_HOURS) * 100);
}

export function hungry(p: PetPref, nowMs: number): boolean {
  return p.hunger && fullNow(p, nowMs) < HUNGRY_BELOW;
}

/** He eats one snack. */
export function feed(p: PetPref, nowMs: number): PetPref {
  return { ...p, full: Math.min(100, fullNow(p, nowMs) + SNACK_FULL), at: nowMs, treats: p.treats + 1 };
}

export function canMoonwalk(p: PetPref): boolean {
  return p.treats >= MOONWALK_TREATS;
}

/** Snacks still to go before the moonwalk. */
export function treatsLeft(p: PetPref): number {
  return Math.max(0, MOONWALK_TREATS - p.treats);
}

/** The song for this moonwalk; `random` is 0…1. */
export function pickSong(p: PetPref, random: number): Song {
  const id = p.song === "random" ? SONG_IDS[Math.min(SONG_IDS.length - 1, Math.floor(random * SONG_IDS.length))] : p.song;
  return SONGS[id];
}

/** The dance, in beats; it repeats while the song plays (DANCE_LOOP beats a round). */
export type Move = "moonwalk" | "spin" | "toes" | "lean" | "kick" | "strut";
export const DANCE: readonly { move: Move; from: number; to: number }[] = [
  { move: "moonwalk", from: 0, to: 16 },
  { move: "spin", from: 16, to: 18 },
  { move: "toes", from: 18, to: 20 },
  { move: "lean", from: 20, to: 28 },
  { move: "kick", from: 28, to: 32 },
  { move: "strut", from: 32, to: 48 },
];
export const DANCE_LOOP = 48;

/** Where the dance is at `beat` (beats since the first one): the move, how far
 *  through it (0…1), and the beat's whole number and fraction. */
export function danceAt(beat: number): { move: Move; k: number; whole: number; frac: number; round: number } {
  const b = Math.max(0, beat);
  const whole = Math.floor(b);
  const frac = b - whole;
  const round = Math.floor(b / DANCE_LOOP);
  const inLoop = b - round * DANCE_LOOP;
  const part = DANCE.find((p) => inLoop >= p.from && inLoop < p.to) ?? DANCE[DANCE.length - 1];
  return { move: part.move, k: (inLoop - part.from) / (part.to - part.from), whole, frac, round };
}

/** Beats since the first one, `seconds` into a song at `bpm`. */
export function beatAt(seconds: number, bpm: number): number {
  return Math.max(0, seconds) * bpm / 60;
}

/** He has the song: it plays and it is the one he asked for. */
export function songPlaying(state: { playing: boolean; track: { title: string } | null }, song: Song): boolean {
  return state.playing && !!state.track && state.track.title.toLowerCase().includes(song.title.toLowerCase());
}
