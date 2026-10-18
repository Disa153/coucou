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
export const SONGS: Record<SongId, { title: string; id: string }> = {
  billieJean: { title: "Billie Jean", id: "7J1uxwnxfQLu4APicE5Rnj" },
  smoothCriminal: { title: "Smooth Criminal", id: "5T7ywazdGIydr6JCW6t02j" },
  bad: { title: "Bad", id: "6JkYFfx8ucsKXFhbUXkzie" },
  beatIt: { title: "Beat It", id: "3BovdzfaX4jb5KFQwoPfAw" },
};

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
export function pickSong(p: PetPref, random: number): { title: string; id: string } {
  const id = p.song === "random" ? SONG_IDS[Math.min(SONG_IDS.length - 1, Math.floor(random * SONG_IDS.length))] : p.song;
  return SONGS[id];
}

/** The moonwalk's timeline in seconds: glide back, spin, toes, walk home. */
export const MOONWALK = { glide: 4.2, spin: 0.8, toes: 1.2, home: 1.6 } as const;
export const MOONWALK_S = MOONWALK.glide + MOONWALK.spin + MOONWALK.toes + MOONWALK.home;

export type MoonPhase = "glide" | "spin" | "toes" | "home" | "done";

/** Where he is in the moonwalk at `t` seconds, and how far back he is (0…1 of the glide). */
export function moonwalkAt(t: number): { phase: MoonPhase; back: number; spin: number } {
  const { glide, spin, toes, home } = MOONWALK;
  if (t < 0) return { phase: "glide", back: 0, spin: 0 };
  if (t < glide) return { phase: "glide", back: t / glide, spin: 0 };
  if (t < glide + spin) return { phase: "spin", back: 1, spin: (t - glide) / spin };
  if (t < glide + spin + toes) return { phase: "toes", back: 1, spin: 0 };
  if (t < MOONWALK_S) return { phase: "home", back: 1 - (t - glide - spin - toes) / home, spin: 0 };
  return { phase: "done", back: 0, spin: 0 };
}
