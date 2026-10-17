// The glowing strip along the island (src/core/glow.ts says what it shows).
// It sits outside the island, on the side that faces the screen, and lets the
// pointer through: the island's own clicks and its window's hit area stay as
// they are.

import { h } from "../views/dom";
import type { GlowLook, GlowPref } from "../core/glow";
import type { IslandEdge } from "../core/state";

/** Between the island and the strip. */
const GAP = 2;

export class GlowStrip {
  /** Carries the light (a drop shadow), which a mask would cut off. */
  readonly el: HTMLElement;
  private readonly bar: HTMLElement;
  private place: GlowPref["place"] = "under";
  private thickness = 3;
  private shown = false;
  private vertical = false;
  private paint = "";

  constructor() {
    this.bar = h("div", { class: "glow-bar" });
    this.el = h("div", { class: "glow-strip", "aria-hidden": "true" }, this.bar);
    this.el.style.display = "none";
  }

  /** The pick or the island's state changed. */
  style(look: GlowLook, pref: GlowPref) {
    this.shown = look.visible;
    this.el.style.display = look.visible ? "" : "none";
    if (!look.visible) return;
    this.place = pref.place;
    this.thickness = pref.thickness;
    this.paint = look.paint;
    this.el.classList.toggle("around", pref.place === "around");
    for (const a of ["pulse", "rainbow", "beat"]) this.el.classList.toggle(a, look.animation === a);
    this.el.style.setProperty("--glow-bright", String(pref.brightness));
    this.el.style.setProperty("--glow-speed", `${pref.speed}s`);
    this.el.style.setProperty("--glow-light", look.light);
    this.el.style.setProperty("--glow-t", `${pref.thickness}px`);
    this.applyPaint();
  }

  private applyPaint() {
    // Along a side, the strip runs top to bottom: so does its gradient.
    this.bar.style.background = this.vertical ? this.paint.replace("90deg", "180deg") : this.paint;
    this.bar.style.backgroundSize = this.el.classList.contains("rainbow") ? (this.vertical ? "100% 300%" : "300% 100%") : "";
  }

  /** Follows the island as it moves and resizes (every frame of its animation). */
  follow(rect: { x: number; y: number; w: number; h: number }, radius: number, edge: IslandEdge | undefined) {
    if (!this.shown) return;
    const e = edge ?? "top";
    const vertical = e === "left" || e === "right";
    const t = this.thickness;
    const s = this.el.style;
    if (this.place === "around") {
      // A frame on the three sides away from the screen edge.
      const out = GAP + t;
      let { x, y, w, h: hh } = rect;
      if (e !== "left") { x -= out; w += out; }
      if (e !== "right") w += out;
      if (e !== "top") { y -= out; hh += out; }
      if (e !== "bottom") hh += out;
      const r = `${Math.max(0, radius + out)}px`;
      const corners = { top: `0 0 ${r} ${r}`, bottom: `${r} ${r} 0 0`, left: `0 ${r} ${r} 0`, right: `${r} 0 0 ${r}` }[e];
      Object.assign(s, { left: `${x}px`, top: `${y}px`, width: `${w}px`, height: `${hh}px` });
      this.bar.style.borderRadius = corners;
      // The frame is drawn where the padding is: the island's side of it is cut out.
      const pad = { top: `0 ${t}px ${t}px ${t}px`, bottom: `${t}px ${t}px 0 ${t}px`, left: `${t}px ${t}px ${t}px 0`, right: `${t}px 0 ${t}px ${t}px` }[e];
      this.bar.style.padding = pad;
    } else {
      // A line along the side facing the screen, clear of the rounded corners.
      const inset = Math.min(radius, (vertical ? rect.h : rect.w) / 2);
      let x: number, y: number, w: number, hh: number;
      if (vertical) {
        x = e === "left" ? rect.x + rect.w + GAP : rect.x - GAP - t;
        y = rect.y + inset;
        w = t;
        hh = Math.max(0, rect.h - 2 * inset);
      } else {
        x = rect.x + inset;
        y = e === "top" ? rect.y + rect.h + GAP : rect.y - GAP - t;
        w = Math.max(0, rect.w - 2 * inset);
        hh = t;
      }
      Object.assign(s, { left: `${x}px`, top: `${y}px`, width: `${w}px`, height: `${hh}px` });
      this.bar.style.borderRadius = "999px";
      this.bar.style.padding = "0";
    }
    if (vertical !== this.vertical) {
      this.vertical = vertical;
      this.applyPaint();
    }
  }
}
