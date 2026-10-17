// Settings → General → Glow: the glowing strip along the island
// (core/glow.ts reads the pick, island/glow.ts draws it). Off by default.

import { GLOW_COLORS, RAINBOW, readGlow, type GlowPref } from "../core/glow";
import type { Settings } from "../core/state";
import { h } from "../views/dom";
import { t } from "../i18n/i18n";

type Toggle = (on: boolean, onChange: (v: boolean) => void) => HTMLElement;

/** A slider's changes are saved this long after it stops moving. */
const SAVE_AFTER_MS = 150;

export function glowSection(settings: Settings, makeToggle: Toggle, save: () => void): HTMLElement {
  let pref: GlowPref = readGlow(settings.glow);
  let timer: number | null = null;

  const details = h("div", { style: "display:flex;flex-direction:column;gap:12px" });

  function store(change: Partial<GlowPref>, now = true) {
    pref = { ...pref, ...change };
    settings.glow = { ...pref };
    if (timer != null) window.clearTimeout(timer);
    timer = null;
    if (now) save();
    else timer = window.setTimeout(() => { timer = null; save(); }, SAVE_AFTER_MS);
    details.style.display = pref.on ? "" : "none";
  }

  function select<K extends "place" | "effect" | "when">(key: K, options: [GlowPref[K], string][]): HTMLSelectElement {
    const el = h("select", {}) as HTMLSelectElement;
    for (const [value, label] of options) el.append(h("option", { value, text: label }));
    el.value = pref[key];
    el.addEventListener("change", () => store({ [key]: el.value } as Partial<GlowPref>));
    return el;
  }

  function slider(
    key: "brightness" | "thickness" | "speed",
    min: number, max: number, step: number,
    show: (v: number) => string,
  ): HTMLElement[] {
    const input = h("input", { type: "range", min: String(min), max: String(max), step: String(step) }) as HTMLInputElement;
    input.value = String(pref[key]);
    const value = h("span", { class: "hint", style: "min-width:44px", text: show(pref[key]) });
    input.addEventListener("input", () => {
      const v = Number(input.value);
      value.textContent = show(v);
      store({ [key]: v } as Partial<GlowPref>, false);
    });
    return [input, value];
  }

  // The colours, the rainbow, then one of your own.
  const swatches: HTMLButtonElement[] = [];
  const custom = h("input", { type: "color", class: "glow-pick", title: t("Your own colour") }) as HTMLInputElement;
  custom.value = pref.color === RAINBOW ? GLOW_COLORS[0] : pref.color;
  function paintSwatches() {
    for (const s of swatches) s.classList.toggle("on", s.dataset.color === pref.color);
    custom.classList.toggle("on", pref.color !== RAINBOW && !GLOW_COLORS.includes(pref.color as (typeof GLOW_COLORS)[number]));
  }
  for (const color of [...GLOW_COLORS, RAINBOW]) {
    const s = h("button", {
      class: color === RAINBOW ? "glow-swatch rainbow" : "glow-swatch",
      title: color === RAINBOW ? t("Rainbow") : color,
      "aria-label": color === RAINBOW ? t("Rainbow") : color,
      style: color === RAINBOW ? "" : `background:${color}`,
    }) as HTMLButtonElement;
    s.dataset.color = color;
    s.addEventListener("click", () => {
      store({ color });
      paintSwatches();
    });
    swatches.push(s);
  }
  custom.addEventListener("input", () => {
    store({ color: custom.value.toLowerCase() }, false);
    paintSwatches();
  });
  paintSwatches();

  details.append(
    h("div", { class: "row" },
      h("label", { text: t("Where") }),
      select("place", [["under", t("Under the island")], ["around", t("Around the island")]]),
    ),
    h("div", { class: "row" },
      h("label", { text: t("Colour") }),
      h("div", { class: "glow-swatches" }, ...swatches, custom),
    ),
    h("div", { class: "row" },
      h("label", { text: t("Effect") }),
      select("effect", [
        ["steady", t("Steady")],
        ["pulse", t("Breathing")],
        ["gradient", t("Gradient")],
        ["beat", t("With the music")],
      ]),
    ),
    h("div", { class: "row" },
      h("label", { text: t("Brightness") }),
      ...slider("brightness", 0.1, 1, 0.05, (v) => `${Math.round(v * 100)} %`),
    ),
    h("div", { class: "row" },
      h("label", { text: t("Thickness") }),
      ...slider("thickness", 1, 6, 1, (v) => `${v} px`),
    ),
    h("div", { class: "row" },
      h("label", { text: t("Speed") }),
      ...slider("speed", 1, 8, 0.5, (v) => t("{seconds} s", { seconds: v })),
    ),
    h("div", { class: "row" },
      h("label", { text: t("Colour from Mochi") }),
      makeToggle(pref.byState, (v) => store({ byState: v })),
      h("span", { class: "hint", text: t("Amber while he waits for your OK, red on an error, green when it's done.") }),
    ),
    h("div", { class: "row" },
      h("label", { text: t("Glows") }),
      select("when", [
        ["always", t("Whenever the island shows")],
        ["working", t("While an agent works")],
        ["music", t("While music plays")],
      ]),
    ),
    h("div", { class: "hint", text: t("A steady strip costs nothing. Breathing, the rainbow and the music beat keep the computer slightly busy while they glow.") }),
  );
  details.style.display = pref.on ? "" : "none";

  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: t("Glow") })),
    h("div", { class: "row" },
      h("label", { text: t("Glowing strip") }),
      makeToggle(pref.on, (v) => store({ on: v })),
      h("span", { class: "hint", text: t("A thin light along the island, in the colour you pick.") }),
    ),
    details,
  );
}
