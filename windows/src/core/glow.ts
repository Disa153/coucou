// The glowing strip along the island (Settings → General → Glow): what the
// user picked, read safely, and what the strip shows for the island's state.
// Off by default, so the island looks as it shipped until it is turned on.
// Drawn by src/island/glow.ts.

import type { BotStateName, IslandMode } from "./layout";

export type GlowPlace = "under" | "around";
export type GlowEffect = "steady" | "pulse" | "gradient" | "beat";
export type GlowWhen = "always" | "working" | "music";

/** As stored in settings.json under "glow" (src-tauri/src/settings.rs). */
export interface GlowPref {
  on: boolean;
  place: GlowPlace;
  /** "#rrggbb", or "rainbow". */
  color: string;
  effect: GlowEffect;
  /** 0.1…1. */
  brightness: number;
  /** Pixels, 1…6. */
  thickness: number;
  /** Seconds per cycle of an animated effect, 1…8. */
  speed: number;
  /** Amber while Mochi waits for an OK, red on an error, green when done. */
  byState: boolean;
  when: GlowWhen;
}

export const DEFAULT_GLOW: GlowPref = {
  on: false,
  place: "under",
  color: "#7c5cff",
  effect: "steady",
  brightness: 0.8,
  thickness: 3,
  speed: 3,
  byState: false,
  when: "always",
};

/** The colours offered in Settings, in order; "rainbow" comes after them. */
export const GLOW_COLORS = ["#7c5cff", "#22d3ee", "#22c55e", "#f5a524", "#f4505e", "#ffffff"] as const;
export const RAINBOW = "rainbow";

const HEX = /^#[0-9a-f]{6}$/i;

function clamp(v: unknown, min: number, max: number, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback;
}

function oneOf<T extends string>(v: unknown, values: readonly T[], fallback: T): T {
  return values.includes(v as T) ? (v as T) : fallback;
}

/** Whatever settings.json holds (an older build's, a hand edit) as a usable pick. */
export function readGlow(raw: unknown): GlowPref {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const d = DEFAULT_GLOW;
  const color = typeof o.color === "string" && (o.color === RAINBOW || HEX.test(o.color)) ? o.color.toLowerCase() : d.color;
  return {
    on: o.on === true,
    place: oneOf(o.place, ["under", "around"] as const, d.place),
    color,
    effect: oneOf(o.effect, ["steady", "pulse", "gradient", "beat"] as const, d.effect),
    brightness: clamp(o.brightness, 0.1, 1, d.brightness),
    thickness: Math.round(clamp(o.thickness, 1, 6, d.thickness)),
    speed: clamp(o.speed, 1, 8, d.speed),
    byState: o.byState === true,
    when: oneOf(o.when, ["always", "working", "music"] as const, d.when),
  };
}

/** The states in which an agent is busy, for "when an agent works". */
const BUSY: readonly BotStateName[] = ["working", "thinking", "searching", "approval", "question"];

export function agentBusy(states: readonly BotStateName[]): boolean {
  return states.some((s) => BUSY.includes(s));
}

/** The colour Mochi's state lends the strip, if it lends one. */
export function stateColor(state: BotStateName): string | null {
  switch (state) {
    case "approval":
    case "question":
      return "#f5a524";
    case "error":
    case "ratelimit":
      return "#f4505e";
    case "finished":
      return "#22c55e";
    default:
      return null;
  }
}

/** A colour turned on the colour wheel, for the gradient's other end. */
export function shiftHue(hex: string, degrees: number): string {
  if (!HEX.test(hex)) return hex;
  const n = parseInt(hex.slice(1), 16);
  let r = ((n >> 16) & 255) / 255, g = ((n >> 8) & 255) / 255, b = (n & 255) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2;
  let h = 0, s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    h /= 6;
  }
  h = (((h * 360 + degrees) % 360) + 360) % 360 / 360;
  const f = (p: number, q: number, t: number) => {
    t = (t + 1) % 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  if (s === 0) {
    r = g = b = l;
  } else {
    const q = l < 0.5 ? l * (1 + s) : l + s - l * s, p = 2 * l - q;
    r = f(p, q, h + 1 / 3);
    g = f(p, q, h);
    b = f(p, q, h - 1 / 3);
  }
  const x = (v: number) => Math.round(v * 255).toString(16).padStart(2, "0");
  return `#${x(r)}${x(g)}${x(b)}`;
}

/** What the strip shows right now. */
export interface GlowLook {
  visible: boolean;
  /** A CSS colour or gradient for the strip itself. */
  paint: string;
  /** The colour of the light around it. */
  light: string;
  /** "rainbow" moves its colours; the others are the pick's effect. */
  animation: "none" | "pulse" | "rainbow" | "beat";
}

export function glowLook(
  pref: GlowPref,
  now: { mode: IslandMode; state: BotStateName; busy: boolean; music: boolean },
): GlowLook {
  const hidden: GlowLook = { visible: false, paint: "", light: "", animation: "none" };
  // Hidden, the island's window is down to its wake strip: nothing is drawn,
  // and nothing may keep animating (0 % CPU).
  if (!pref.on || now.mode === "hidden") return hidden;
  if (pref.when === "working" && !now.busy) return hidden;
  if (pref.when === "music" && !now.music) return hidden;

  const fromState = pref.byState ? stateColor(now.state) : null;
  if (fromState) {
    // A state colour stands out: steady, or breathing if that was the pick.
    return { visible: true, paint: fromState, light: fromState, animation: pref.effect === "pulse" ? "pulse" : "none" };
  }
  if (pref.color === RAINBOW) {
    const rainbow = "linear-gradient(90deg,#ff4d6d,#ffb84d,#5cff8d,#4dd2ff,#9d6bff,#ff4d6d)";
    return { visible: true, paint: rainbow, light: "#9d6bff", animation: "rainbow" };
  }
  const color = pref.color;
  switch (pref.effect) {
    case "gradient":
      return { visible: true, paint: `linear-gradient(90deg,${color},${shiftHue(color, 70)})`, light: color, animation: "none" };
    case "pulse":
      return { visible: true, paint: color, light: color, animation: "pulse" };
    case "beat":
      // In time while music plays; steady otherwise.
      return { visible: true, paint: color, light: color, animation: now.music ? "beat" : "none" };
    default:
      return { visible: true, paint: color, light: color, animation: "none" };
  }
}
