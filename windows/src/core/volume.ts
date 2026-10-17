// The volume panel (src-tauri/src/volume.rs): what the backend sends, the panel's
// preferences and the small pure rules around them, so the view stays plain DOM.

export type PanelSize = "compact" | "normal" | "large";

/** Settings → Volume. Mirrors `VolumePanelPref` in src-tauri/src/settings.rs. */
export interface VolumePanelPrefs {
  size: PanelSize;
  /** "#RRGGBB", or "" for white. */
  color: string;
  showPercent: boolean;
  showDots: boolean;
  /** Only the apps that make sound right now. */
  onlyPlaying: boolean;
  /** Hide Windows' own sounds. */
  hideSystem: boolean;
  /** The quick presets under the master volume. */
  presets: boolean;
  /** Percent per notch of the mouse wheel. */
  wheelStep: number;
  /** The highest level the panel's sliders reach. */
  limit: number;
}

export const DEFAULT_VOLUME_PREFS: VolumePanelPrefs = {
  size: "normal",
  color: "",
  showPercent: true,
  showDots: true,
  onlyPlaying: true,
  hideSystem: true,
  presets: true,
  wheelStep: 5,
  limit: 100,
};

export const PANEL_SIZES: readonly PanelSize[] = ["compact", "normal", "large"];
export const WHEEL_STEPS: readonly number[] = [1, 2, 5, 10];
export const LIMIT_MIN = 20;
export const LIMIT_MAX = 100;
/** Slider colours offered in Settings (white first: the default). */
export const SLIDER_COLORS: readonly string[] = ["", "#22C55E", "#378ADD", "#9A7CF0", "#F5A524", "#F4505E"];

/** What the settings file says, brought back to something the panel can use. */
export function sanitizePrefs(raw: unknown): VolumePanelPrefs {
  const d = DEFAULT_VOLUME_PREFS;
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const flag = (v: unknown, fallback: boolean) => (typeof v === "boolean" ? v : fallback);
  const limit = typeof r.limit === "number" && Number.isFinite(r.limit)
    ? Math.min(LIMIT_MAX, Math.max(LIMIT_MIN, Math.round(r.limit)))
    : d.limit;
  return {
    size: PANEL_SIZES.includes(r.size as PanelSize) ? (r.size as PanelSize) : d.size,
    color: typeof r.color === "string" && /^#[0-9a-fA-F]{6}$/.test(r.color) ? r.color.toUpperCase() : "",
    showPercent: flag(r.showPercent, d.showPercent),
    showDots: flag(r.showDots, d.showDots),
    onlyPlaying: flag(r.onlyPlaying, d.onlyPlaying),
    hideSystem: flag(r.hideSystem, d.hideSystem),
    presets: flag(r.presets, d.presets),
    wheelStep: typeof r.wheelStep === "number" && WHEEL_STEPS.includes(r.wheelStep) ? r.wheelStep : d.wheelStep,
    limit,
  };
}

// ── What the backend sends ────────────────────────────────────────────────────

export interface VolumeApp {
  /** The program's file name, lower case; the target is `app:<id>`. */
  id: string;
  /** Empty for Windows' own sounds. */
  name: string;
  volume: number;
  muted: boolean;
  playing: boolean;
  system: boolean;
}

export interface VolumeState {
  supported: boolean;
  master: number;
  masterMuted: boolean;
  apps: VolumeApp[];
}

export const EMPTY_VOLUME: VolumeState = { supported: false, master: 0, masterMuted: false, apps: [] };

// ── Rules ─────────────────────────────────────────────────────────────────────

/** A level from a slider, whole and within 0…limit. */
export function clampLevel(value: number, limit: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(limit, Math.max(0, Math.round(value)));
}

/** One notch of the wheel: up (negative deltaY) raises the level, down lowers it. */
export function wheelLevel(current: number, deltaY: number, step: number, limit: number): number {
  if (deltaY === 0) return clampLevel(current, limit);
  return clampLevel(current + (deltaY < 0 ? step : -step), limit);
}

/** The apps the panel lists under the user's preferences. */
export function visibleApps(apps: VolumeApp[], prefs: VolumePanelPrefs): VolumeApp[] {
  return apps.filter((a) => !(prefs.hideSystem && a.system) && !(prefs.onlyPlaying && !a.playing));
}

// ── Presets ───────────────────────────────────────────────────────────────────

export type PresetId = "night" | "game" | "call";
export const PRESET_IDS: readonly PresetId[] = ["night", "game", "call"];

/** Programs people talk in: the Game preset lowers them, the Call preset raises them. */
const VOICE = new Set(["discord", "telegram", "teams", "ms-teams", "zoom", "skype"]);
/** Music and video: lowered in a game or a call. */
const MEDIA = new Set([
  "chrome", "msedge", "firefox", "opera", "brave", "browser", "spotify", "vlc", "steam",
]);

export interface PresetLevels {
  /** Undefined: the master volume stays. */
  master?: number;
  /** App id → level. Apps not named stay as they are. */
  apps: Record<string, number>;
}

/**
 * What a preset sets. Night: everything quieter, never louder. Game: a program
 * Coucou does not know is taken for the game and goes full; voice apps stay
 * audible, music and video drop. Call: voice apps full, the rest low.
 * Windows' own sounds are left alone.
 */
export function presetLevels(id: PresetId, state: VolumeState, limit: number): PresetLevels {
  const apps: Record<string, number> = {};
  for (const a of state.apps) {
    if (a.system) continue;
    const voice = VOICE.has(a.id);
    const media = MEDIA.has(a.id);
    let level: number;
    switch (id) {
      case "night":
        level = Math.min(a.volume, 25);
        break;
      case "game":
        level = voice ? 70 : media ? 15 : 100;
        break;
      case "call":
        level = voice ? 100 : 10;
        break;
    }
    apps[a.id] = clampLevel(level, limit);
  }
  const out: PresetLevels = { apps };
  if (id === "night") out.master = clampLevel(Math.min(state.master, 20), limit);
  return out;
}

// ── Colours ───────────────────────────────────────────────────────────────────

const BRAND: Record<string, string> = {
  chrome: "#4285F4", msedge: "#0EA5E9", firefox: "#F97316", opera: "#F4505E", brave: "#FB542B",
  browser: "#F43F5E", spotify: "#1DB954", discord: "#9A7CF0", telegram: "#38BDF8", vlc: "#F59E0B",
  steam: "#64748B", zoom: "#2D8CFF", teams: "#7C6FE0", "ms-teams": "#7C6FE0", coucou: "#2DD4BF",
};
const FALLBACK = ["#F5A524", "#22C55E", "#378ADD", "#D4537E", "#2DD4BF", "#A78BFA", "#F4505E"];

/** The dot before an app: its brand colour, else one picked from its name. */
export function appColor(id: string): string {
  if (id === "system") return "#888780";
  if (BRAND[id]) return BRAND[id];
  let hash = 0;
  for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) >>> 0;
  return FALLBACK[hash % FALLBACK.length];
}
