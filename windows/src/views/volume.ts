// The volume panel — the Windows mixer inside the island: the master volume,
// the quick presets and each app's own level (src-tauri/src/volume.rs).
//
// It reads the mixer once a second, and only while it is on screen, so a hidden
// island does no work. A slider moved here is shown at once and sent at most
// every 100 ms; what the mixer says is ignored for a moment after, and while a
// slider is held, so it never fights the hand.

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import { t, tl } from "../i18n/i18n";
import {
  EMPTY_VOLUME, PRESET_IDS, appColor, clampLevel, presetLevels, sanitizePrefs, visibleApps, wheelLevel,
  type PresetId, type VolumeApp, type VolumePanelPrefs, type VolumeState,
} from "../core/volume";
import type { ViewHost } from "./views";

const POLL_MS = 1000;
/** After the user moves something, what the mixer reports is not shown for this long. */
const QUIET_MS = 1500;
const SEND_MS = 100;

interface Row {
  el: HTMLElement;
  update(level: number, muted: boolean, name: string, prefs: VolumePanelPrefs): void;
}

export function buildVolume(): ViewHost {
  let prefs = sanitizePrefs(State.settings.volumePanel);
  let state: VolumeState = EMPTY_VOLUME;
  let dragging = false;
  let quietUntil = 0;
  let busy = false;
  let timer: number | null = null;
  let sendTimer: number | null = null;
  const queued = new Map<string, number>();

  // ── Sending ───────────────────────────────────────────────────────────────

  function flush() {
    if (sendTimer != null) window.clearTimeout(sendTimer);
    sendTimer = null;
    for (const [target, value] of queued) void Bridge.volumeSet(target, value);
    queued.clear();
  }

  function setLevel(target: string, value: number) {
    const level = clampLevel(value, prefs.limit);
    quietUntil = Date.now() + QUIET_MS;
    if (target === "master") state = { ...state, master: level, masterMuted: false };
    else {
      const id = target.slice(4);
      state = { ...state, apps: state.apps.map((a) => (a.id === id ? { ...a, volume: level, muted: false } : a)) };
    }
    queued.set(target, level);
    if (sendTimer == null) sendTimer = window.setTimeout(flush, SEND_MS);
    render();
  }

  function toggleMute(target: string) {
    quietUntil = Date.now() + QUIET_MS;
    let muted: boolean;
    if (target === "master") {
      muted = !state.masterMuted;
      state = { ...state, masterMuted: muted };
    } else {
      const id = target.slice(4);
      muted = !(state.apps.find((a) => a.id === id)?.muted ?? false);
      state = { ...state, apps: state.apps.map((a) => (a.id === id ? { ...a, muted } : a)) };
    }
    void Bridge.volumeMute(target, muted);
    render();
  }

  async function applyPreset(id: PresetId) {
    const plan = presetLevels(id, state, prefs.limit);
    quietUntil = Date.now() + QUIET_MS;
    state = {
      ...state,
      master: plan.master ?? state.master,
      apps: state.apps.map((a) => (a.id in plan.apps ? { ...a, volume: plan.apps[a.id], muted: false } : a)),
    };
    render();
    if (plan.master !== undefined) await Bridge.volumeSet("master", plan.master);
    for (const [appId, level] of Object.entries(plan.apps)) await Bridge.volumeSet(`app:${appId}`, level);
    window.setTimeout(() => void poll(true), 300);
  }

  // Letting go of a slider anywhere ends the drag and sends what is left.
  const release = () => {
    if (!dragging) return;
    dragging = false;
    flush();
  };
  window.addEventListener("pointerup", release);
  window.addEventListener("pointercancel", release);

  // ── Rows ──────────────────────────────────────────────────────────────────

  function makeRow(target: string, name: string | ReturnType<typeof tl>, bold: boolean): Row {
    const dot = h("span", { class: "vol-dot" });
    const label = h("span", { class: bold ? "vol-name strong" : "vol-name" }, name);
    const input = h("input", { type: "range", min: "0", max: "100", step: "1" }) as HTMLInputElement;
    const percent = h("span", { class: "vol-pc" });
    const mute = h("button", { class: "vol-mute", onclick: () => toggleMute(target) });
    const el = h("div", { class: "vol-row" }, dot, label, input, percent, mute);

    const paint = () => {
      input.style.setProperty("--p", `${(Number(input.value) / prefs.limit) * 100}%`);
    };
    input.addEventListener("pointerdown", () => {
      dragging = true;
    });
    input.addEventListener("input", () => {
      const v = clampLevel(Number(input.value), prefs.limit);
      input.value = String(v);
      paint();
      percent.textContent = `${v}%`;
      setLevel(target, v);
    });
    // The page scrolls with the wheel by default: over a slider it moves the slider.
    input.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        const v = wheelLevel(Number(input.value), e.deltaY, prefs.wheelStep, prefs.limit);
        input.value = String(v);
        paint();
        percent.textContent = `${v}%`;
        setLevel(target, v);
        flush();
      },
      { passive: false },
    );

    return {
      el,
      update(level, muted, shown, p) {
        input.max = String(p.limit);
        if (!dragging) input.value = String(Math.min(level, p.limit));
        paint();
        if (typeof name === "string") label.textContent = shown;
        dot.style.display = p.showDots && target !== "master" ? "" : "none";
        dot.style.background = target === "master" ? "transparent" : appColor(target.slice(4));
        percent.style.display = p.showPercent ? "" : "none";
        percent.textContent = `${level}%`;
        el.classList.toggle("muted", muted);
        mute.title = muted ? t("Unmute") : t("Mute");
        mute.setAttribute("aria-label", mute.title);
        clear(mute);
        mute.append(svg(muted ? ICONS.speakerOff : ICONS.speakerOn, 13));
      },
    };
  }

  const master = makeRow("master", tl("Windows volume"), true);
  const presets = h(
    "div",
    { class: "vol-presets" },
    ...PRESET_IDS.map((id) =>
      h("button", { class: "vol-chip", onclick: () => void applyPreset(id) },
        id === "night" ? tl("Night") : id === "game" ? tl("Game") : tl("Call")),
    ),
  );
  const caption = h("div", { class: "vol-cap" }, tl("Apps"));
  const list = h("div", { class: "vol-list" });
  const empty = h("div", { class: "vol-empty" }, tl("Nothing is playing"));
  const rows = new Map<string, Row>();

  const stack = h(
    "div",
    { class: "stack vol-stack" },
    master.el, presets, caption, list, empty,
  );
  const card = h("div", { class: "card vol-card" }, stack);
  const el = h("div", { class: "view" }, card);

  // ── Drawing ───────────────────────────────────────────────────────────────

  function render() {
    card.classList.remove("vol-compact", "vol-normal", "vol-large");
    card.classList.add(`vol-${prefs.size}`);
    card.style.setProperty("--vol-acc", prefs.color || "#ffffff");
    card.classList.toggle("vol-nodots", !prefs.showDots);
    presets.style.display = prefs.presets ? "" : "none";

    master.update(state.master, state.masterMuted, "", prefs);

    const shown = visibleApps(state.apps, prefs);
    const wanted = new Set(shown.map((a) => a.id));
    for (const [id, row] of rows) {
      if (!wanted.has(id)) {
        row.el.remove();
        rows.delete(id);
      }
    }
    for (const a of shown) {
      let row = rows.get(a.id);
      if (!row) {
        row = makeRow(`app:${a.id}`, a.name, false);
        rows.set(a.id, row);
      }
      row.update(a.volume, a.muted, appName(a), prefs);
      list.append(row.el); // appending an existing row moves it: the order follows the mixer's
    }
    empty.style.display = shown.length === 0 ? "" : "none";
    caption.style.display = shown.length === 0 ? "none" : "";
  }

  function appName(a: VolumeApp): string {
    return a.system ? t("System sounds") : a.name;
  }

  // ── Reading the mixer ─────────────────────────────────────────────────────

  async function poll(force = false) {
    if (busy) return;
    busy = true;
    try {
      const next = await Bridge.volumeState();
      if (!next) return;
      if (!force && (dragging || Date.now() < quietUntil)) return;
      // A mixer that is gone shows nothing rather than the last apps it had.
      state = next.supported ? next : EMPTY_VOLUME;
      render();
    } finally {
      busy = false;
    }
  }

  // The mixer is read only while the panel is on screen.
  State.subscribe(() => {
    const active = State.view === "volume" && State.mode === "expanded";
    if (active && timer == null) {
      void poll(true);
      timer = window.setInterval(() => void poll(), POLL_MS);
    } else if (!active && timer != null) {
      window.clearInterval(timer);
      timer = null;
      flush();
    }
  });

  return {
    el,
    sync() {
      prefs = sanitizePrefs(State.settings.volumePanel);
      render();
    },
  };
}
