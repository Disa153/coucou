// Mochi as a dog — picked in the wardrobe, stored as `mochiBreed` (this fork only).
//
// Two families, from design/dogs-preview.html and design/dogs-realistic.html:
// - "corgi" / "shepherd": Mochi's own body with ears, coat markings and a nose.
//   Eyes, states, emotes and outfits stay Mochi's (BotEngine draws them).
// - "corgiReal" / "shepherdReal": a drawn dog's head that replaces the body. It
//   follows the state (blink, look, wide eyes, happy eyes, tongue) but wears no outfit.
// Only the main Mochi becomes a dog; the minis of the pills keep their colours.

import { N_, labels } from "../i18n/i18n";

export const BREEDS = ["mochi", "corgi", "shepherd", "corgiReal", "shepherdReal"] as const;
export type Breed = (typeof BREEDS)[number];
export const DEFAULT_BREED: Breed = "mochi";

export const BREED_KEYS: Record<Breed, string> = {
  mochi: N_("Mochi"),
  corgi: N_("Corgi"),
  shepherd: N_("Shepherd"),
  corgiReal: N_("Corgi (realistic)"),
  shepherdReal: N_("Shepherd (realistic)"),
};
export const BREED_LABELS: Record<Breed, string> = labels(BREED_KEYS);

const SET: ReadonlySet<string> = new Set(BREEDS);
export function parseBreed(raw: unknown): Breed {
  return typeof raw === "string" && SET.has(raw) ? (raw as Breed) : DEFAULT_BREED;
}

export function isRealistic(b: Breed): boolean {
  return b === "corgiReal" || b === "shepherdReal";
}

type Ctx = CanvasRenderingContext2D;

function shade(hex: string, amt: number): string {
  const n = parseInt(hex.slice(1), 16);
  const f = (c: number) => Math.max(0, Math.min(255, Math.round(c + (amt < 0 ? c * amt : (255 - c) * amt))));
  return `rgb(${f(n >> 16)},${f((n >> 8) & 255)},${f(n & 255)})`;
}

// ── Mochi-shaped dogs ─────────────────────────────────────────────────────────

interface Cute {
  top: string; bottom: string;
  ear: "corgi" | "shepherd"; earCol: string; earIn: string;
}

const CUTE: Record<"corgi" | "shepherd", Cute> = {
  corgi: { top: "#F2A65E", bottom: "#C9752E", ear: "corgi", earCol: "#D98940", earIn: "#F7C4B0" },
  shepherd: { top: "#D9A25E", bottom: "#A86F35", ear: "shepherd", earCol: "#2B231E", earIn: "#5A4130" },
};

/** The body gradient of a Mochi-shaped dog, or null for Mochi himself. */
export function cuteBody(b: Breed): [string, string] | null {
  return b === "corgi" || b === "shepherd" ? [CUTE[b].top, CUTE[b].bottom] : null;
}

/** Ear twitch, −1…1 envelope, now and then. */
function twitch(t: number): number {
  const c = (t * 0.7) % 4;
  return c < 0.25 ? Math.sin((c / 0.25) * Math.PI) : 0;
}

/** Behind the body: the ears. `yaw` slides them with the head. */
export function drawCuteBehind(x: Ctx, b: Breed, R: number, rx: number, ry: number, yaw: number, t: number, alert: boolean) {
  if (b !== "corgi" && b !== "shepherd") return;
  const d = CUTE[b];
  const shift = Math.sin(yaw) * rx * 0.5;
  for (const side of [-1, 1]) {
    const tall = d.ear === "corgi" ? 0.95 : 1.15;
    const wide = d.ear === "corgi" ? 0.62 : 0.5;
    const tilt = (d.ear === "corgi" ? 0.42 : 0.26) - (alert ? 0.12 : 0);
    x.save();
    x.translate(side * rx * 0.52 + shift, -ry * 0.62);
    x.rotate(side * tilt + (side > 0 ? twitch(t) * 0.18 : 0));
    const h = R * tall, w = R * wide;
    const shape = (s: number) => {
      x.beginPath();
      x.moveTo(-w / 2 * s, 0);
      x.quadraticCurveTo(-w * 0.42 * s, -h * 0.6 * s, -w * 0.06 * s, -h * s);
      x.quadraticCurveTo(0, -h * 1.04 * s, w * 0.06 * s, -h * s);
      x.quadraticCurveTo(w * 0.42 * s, -h * 0.6 * s, w / 2 * s, 0);
      x.closePath();
    };
    shape(1); x.fillStyle = d.earCol; x.fill();
    x.translate(0, -h * 0.08); shape(0.6); x.fillStyle = d.earIn; x.fill();
    x.restore();
  }
}

/** On the body (clipped to it): the coat markings. */
export function drawCuteMarkings(x: Ctx, b: Breed, body: Path2D, R: number, rx: number, ry: number, yaw: number) {
  if (b !== "corgi" && b !== "shepherd") return;
  const dx = Math.sin(yaw) * rx * 0.8;
  x.save();
  x.clip(body);
  if (b === "corgi") {
    x.fillStyle = "#FBF7F2";
    x.beginPath(); x.ellipse(dx, ry * 0.78, rx * 0.62, ry * 0.62, 0, 0, Math.PI * 2); x.fill();
    x.beginPath(); x.roundRect(dx - R * 0.09, -ry * 1.1, R * 0.18, ry * 1.3, R * 0.09); x.fill();
  } else {
    x.fillStyle = "#2B231E";
    x.beginPath(); x.ellipse(dx * 0.5, -ry * 1.0, rx * 1.25, ry * 0.72, 0, 0, Math.PI * 2); x.fill();
    x.fillStyle = "#E7B572";
    for (const sd of [-1, 1]) { x.beginPath(); x.ellipse(dx + sd * 0.36 * rx, -ry * 0.33, R * 0.1, R * 0.06, 0, 0, Math.PI * 2); x.fill(); }
    x.fillStyle = "#4A3428";
    x.beginPath(); x.ellipse(dx, ry * 0.72, rx * 0.42, ry * 0.4, 0, 0, Math.PI * 2); x.fill();
  }
  x.restore();
}

/** In front of the eyes: nose and mouth; the tongue when happy. */
export function drawCuteNose(x: Ctx, b: Breed, body: Path2D, R: number, rx: number, ry: number, yaw: number, happy: boolean) {
  if (b !== "corgi" && b !== "shepherd") return;
  if (Math.cos(yaw) < 0.2) return;
  const dx = Math.sin(yaw) * rx * 0.8;
  const ny = ry * 0.42, nw = R * 0.2;
  x.save();
  x.clip(body);
  x.translate(dx, 0);
  x.fillStyle = "#14100E";
  x.beginPath(); x.moveTo(-nw / 2, ny - nw * 0.25); x.quadraticCurveTo(0, ny - nw * 0.45, nw / 2, ny - nw * 0.25);
  x.quadraticCurveTo(nw * 0.3, ny + nw * 0.4, 0, ny + nw * 0.45); x.quadraticCurveTo(-nw * 0.3, ny + nw * 0.4, -nw / 2, ny - nw * 0.25); x.fill();
  if (R > 14) {
    x.strokeStyle = "#14100E"; x.lineWidth = R * 0.045; x.lineCap = "round";
    const my = ny + nw * 0.75;
    x.beginPath(); x.moveTo(0, ny + nw * 0.4); x.lineTo(0, my - nw * 0.1);
    x.moveTo(-nw * 0.55, my - nw * 0.15); x.quadraticCurveTo(-nw * 0.25, my + nw * 0.25, 0, my - nw * 0.1);
    x.quadraticCurveTo(nw * 0.25, my + nw * 0.25, nw * 0.55, my - nw * 0.15); x.stroke();
    if (happy) {
      x.fillStyle = "#FF6F8E";
      x.beginPath(); x.roundRect(-nw * 0.28, my - nw * 0.02, nw * 0.56, nw * 0.7, nw * 0.28); x.fill();
    }
  }
  x.restore();
}

// ── Drawn dogs ────────────────────────────────────────────────────────────────

interface Real {
  base: [string, string];
  light?: string; lightShade?: string; dark?: string;
  earOut: [string, string]; earIn: [string, string];
  iris: string;
  head: { top: number; cheekX: number; cheekY: number; muzzleX: number; muzzleY: number; chin: number };
  ear: { h: number; w: number; tilt: number; x: number; y: number };
  eyeX: number; eyeY: number; noseY: number;
}

const REAL: Record<"corgiReal" | "shepherdReal", Real> = {
  corgiReal: {
    base: ["#EEA55A", "#BF6B2C"], light: "#FFFBF5", lightShade: "#EADFD2",
    earOut: ["#E0904A", "#B4612A"], earIn: ["#F6C9B6", "#E7A08C"], iris: "#4A2A14",
    head: { top: -0.5, cheekX: 0.52, cheekY: 0.04, muzzleX: 0.21, muzzleY: 0.44, chin: 0.56 },
    ear: { h: 0.74, w: 0.5, tilt: 0.36, x: 0.31, y: -0.3 },
    eyeX: 0.21, eyeY: -0.07, noseY: 0.28,
  },
  shepherdReal: {
    base: ["#DCA563", "#A46B33"], dark: "#1E1814",
    earOut: ["#2A221D", "#151110"], earIn: ["#D6A66E", "#8E6038"], iris: "#3A2210",
    head: { top: -0.54, cheekX: 0.47, cheekY: -0.04, muzzleX: 0.18, muzzleY: 0.52, chin: 0.64 },
    ear: { h: 0.86, w: 0.42, tilt: 0.17, x: 0.27, y: -0.36 },
    eyeX: 0.19, eyeY: -0.1, noseY: 0.36,
  },
};

function headPath(S: number, h: Real["head"]): Path2D {
  const p = new Path2D();
  p.moveTo(0, h.top * S);
  p.bezierCurveTo(h.cheekX * 0.62 * S, h.top * S, h.cheekX * S, (h.top + 0.2) * S, h.cheekX * S, h.cheekY * S);
  p.bezierCurveTo(h.cheekX * S, (h.cheekY + 0.22) * S, (h.muzzleX + 0.1) * S, (h.muzzleY - 0.08) * S, h.muzzleX * S, h.muzzleY * S);
  p.bezierCurveTo((h.muzzleX - 0.03) * S, (h.chin - 0.04) * S, 0.08 * S, h.chin * S, 0, h.chin * S);
  p.bezierCurveTo(-0.08 * S, h.chin * S, -(h.muzzleX - 0.03) * S, (h.chin - 0.04) * S, -h.muzzleX * S, h.muzzleY * S);
  p.bezierCurveTo(-(h.muzzleX + 0.1) * S, (h.muzzleY - 0.08) * S, -h.cheekX * S, (h.cheekY + 0.22) * S, -h.cheekX * S, h.cheekY * S);
  p.bezierCurveTo(-h.cheekX * S, (h.top + 0.2) * S, -h.cheekX * 0.62 * S, h.top * S, 0, h.top * S);
  p.closePath();
  return p;
}

function earPath(S: number, e: Real["ear"]): Path2D {
  const h = e.h * S, w = e.w * S, p = new Path2D();
  p.moveTo(-w / 2, 0);
  p.bezierCurveTo(-w * 0.46, -h * 0.5, -w * 0.2, -h * 0.9, -w * 0.04, -h * 0.99);
  p.quadraticCurveTo(w * 0.02, -h * 1.02, w * 0.07, -h * 0.98);
  p.bezierCurveTo(w * 0.26, -h * 0.8, w * 0.5, -h * 0.42, w / 2, 0);
  p.closePath();
  return p;
}

function drawEar(x: Ctx, S: number, d: Real, side: number, lift: number) {
  const e = d.ear;
  x.save();
  x.translate(side * e.x * S, e.y * S);
  x.rotate(side * (e.tilt - lift));
  x.scale(side, 1);
  const g = x.createLinearGradient(-e.w * S / 2, 0, e.w * S / 2, -e.h * S);
  g.addColorStop(0, d.earOut[0]); g.addColorStop(1, d.earOut[1]);
  x.fillStyle = g; x.fill(earPath(S, e));
  x.translate(S * 0.015, -S * 0.05); x.scale(0.66, 0.76);
  const inner = earPath(S, e);
  const ig = x.createLinearGradient(0, 0, 0, -e.h * S);
  ig.addColorStop(0, d.earIn[0]); ig.addColorStop(1, d.earIn[1]);
  x.fillStyle = ig; x.fill(inner);
  const dg = x.createLinearGradient(-e.w * S / 2, 0, e.w * S / 2, 0);
  dg.addColorStop(0, "rgba(0,0,0,0)"); dg.addColorStop(1, "rgba(60,25,10,0.28)");
  x.fillStyle = dg; x.fill(inner);
  x.restore();
}

function softBlob(x: Ctx, cx: number, cy: number, rx: number, ry: number, rgb: string, alpha: number) {
  x.save(); x.translate(cx, cy); x.scale(1, ry / rx);
  const g = x.createRadialGradient(0, 0, 0, 0, 0, rx);
  g.addColorStop(0, `rgba(${rgb},${alpha})`); g.addColorStop(1, `rgba(${rgb},0)`);
  x.fillStyle = g; x.beginPath(); x.arc(0, 0, rx, 0, Math.PI * 2); x.fill();
  x.restore();
}

/** Eye looks the drawn dog understands, from Mochi's eye shapes. */
export type RealEye = "open" | "wide" | "happy" | "closed" | "half" | "wink";

export function realEyeFor(shape: string): RealEye {
  switch (shape) {
    case "wide": case "dot": return "wide";
    case "happy": case "heart": case "star": return "happy";
    case "closed": return "closed";
    case "tired": case "flat": case "line": return "half";
    case "wink": return "wink";
    default: return "open";
  }
}

function drawRealEye(x: Ctx, S: number, d: Real, side: number, look: RealEye, open: number, gaze: number) {
  const r = 0.072 * S;
  x.save();
  x.translate(side * d.eyeX * S, d.eyeY * S);
  x.rotate(side * -0.12);
  const lidOnly = look === "happy" || look === "closed" || (look === "wink" && side > 0);
  if (lidOnly) {
    x.strokeStyle = "#1A120C"; x.lineWidth = S * 0.022; x.lineCap = "round";
    x.beginPath();
    if (look === "closed" || look === "wink") x.arc(0, -r * 0.6, r * 1.0, Math.PI * 0.18, Math.PI * 0.82);
    else x.arc(0, r * 0.5, r * 1.0, Math.PI * 1.15, Math.PI * 1.85);
    x.stroke();
    x.restore();
    return;
  }
  let o = Math.max(0.08, open);
  if (look === "half") o *= 0.45;
  if (look === "wide") o *= 1.15;
  x.fillStyle = "rgba(40,20,8,0.28)";
  x.beginPath(); x.ellipse(0, r * 0.1, r * 1.55, r * 1.25, 0, 0, Math.PI * 2); x.fill();
  const almond = new Path2D();
  almond.moveTo(-r * 1.25, 0);
  almond.bezierCurveTo(-r * 0.7, -r * 1.2 * o, r * 0.7, -r * 1.2 * o, r * 1.25, -r * 0.05);
  almond.bezierCurveTo(r * 0.7, r * 1.05 * o, -r * 0.7, r * 1.05 * o, -r * 1.25, 0);
  x.fillStyle = "#120B07"; x.fill(almond);
  x.save(); x.clip(almond);
  const lx = gaze * r * 0.45;
  const ig = x.createRadialGradient(lx, 0, r * 0.1, lx, 0, r * 0.95);
  ig.addColorStop(0, shade(d.iris, 0.35)); ig.addColorStop(0.7, d.iris); ig.addColorStop(1, "#140C06");
  x.fillStyle = ig; x.beginPath(); x.arc(lx, 0, r * 0.95, 0, Math.PI * 2); x.fill();
  x.fillStyle = "#050302"; x.beginPath(); x.arc(lx, 0, r * (look === "wide" ? 0.6 : 0.45), 0, Math.PI * 2); x.fill();
  x.fillStyle = "rgba(255,255,255,0.92)"; x.beginPath(); x.arc(lx + r * 0.32, -r * 0.34, r * 0.22, 0, Math.PI * 2); x.fill();
  x.fillStyle = "rgba(255,255,255,0.45)"; x.beginPath(); x.arc(lx - r * 0.3, r * 0.32, r * 0.1, 0, Math.PI * 2); x.fill();
  x.restore();
  x.strokeStyle = "#0E0805"; x.lineWidth = S * 0.012; x.stroke(almond);
  x.restore();
}

function drawRealNose(x: Ctx, S: number, d: Real, tongue: boolean, dark: boolean) {
  const ny = d.noseY * S, w = 0.18 * S;
  const p = new Path2D();
  p.moveTo(-w / 2, ny - w * 0.18);
  p.bezierCurveTo(-w * 0.45, ny - w * 0.5, w * 0.45, ny - w * 0.5, w / 2, ny - w * 0.18);
  p.bezierCurveTo(w * 0.48, ny + w * 0.18, w * 0.12, ny + w * 0.36, 0, ny + w * 0.42);
  p.bezierCurveTo(-w * 0.12, ny + w * 0.36, -w * 0.48, ny + w * 0.18, -w / 2, ny - w * 0.18);
  const g = x.createLinearGradient(0, ny - w * 0.5, 0, ny + w * 0.4);
  g.addColorStop(0, "#3A3230"); g.addColorStop(1, "#0C0908");
  x.fillStyle = g; x.fill(p);
  x.fillStyle = "#050404";
  for (const sd of [-1, 1]) { x.beginPath(); x.ellipse(sd * w * 0.22, ny + w * 0.02, w * 0.1, w * 0.06, sd * 0.5, 0, Math.PI * 2); x.fill(); }
  x.fillStyle = "rgba(255,255,255,0.55)";
  x.beginPath(); x.ellipse(-w * 0.08, ny - w * 0.26, w * 0.16, w * 0.06, -0.2, 0, Math.PI * 2); x.fill();
  // On a black mask the mouth is drawn lighter, or it would not show.
  x.strokeStyle = dark ? "rgba(120,90,70,0.9)" : "#1B120D"; x.lineWidth = S * 0.014; x.lineCap = "round";
  const my = ny + w * 0.95;
  x.beginPath(); x.moveTo(0, ny + w * 0.42); x.lineTo(0, my - w * 0.15);
  x.moveTo(-w * 0.75, my - w * 0.25); x.quadraticCurveTo(-w * 0.35, my + w * 0.3, 0, my - w * 0.15);
  x.quadraticCurveTo(w * 0.35, my + w * 0.3, w * 0.75, my - w * 0.25); x.stroke();
  if (tongue) {
    const tg = x.createLinearGradient(0, my, 0, my + w * 1.2);
    tg.addColorStop(0, "#E2566F"); tg.addColorStop(1, "#F27D92");
    x.fillStyle = tg;
    x.beginPath(); x.moveTo(-w * 0.36, my - w * 0.02);
    x.bezierCurveTo(-w * 0.42, my + w * 1.1, w * 0.42, my + w * 1.1, w * 0.36, my - w * 0.02); x.closePath(); x.fill();
  }
}

export interface RealPose {
  /** Mochi's body radius: the head is sized from it. */
  R: number;
  eye: RealEye;
  /** 1 = open, 0 = closed (blinks). */
  open: number;
  /** −1…1, where the eyes look. */
  gaze: number;
  /** Ears up (approval). */
  alert: boolean;
  /** Seconds, for the ear twitch. */
  t: number;
}

/** A drawn dog's head, centred where Mochi's body is. */
export function drawRealDog(x: Ctx, b: Breed, p: RealPose) {
  if (b !== "corgiReal" && b !== "shepherdReal") return;
  const d = REAL[b];
  // Sized so the ears stay within the space a tall hat takes above Mochi.
  const S = p.R * 1.45;
  x.save();
  x.translate(0, p.R * 0.04);
  const lift = p.alert ? 0.1 : 0;
  drawEar(x, S, d, -1, lift);
  drawEar(x, S, d, 1, lift + twitch(p.t) * 0.14);

  const head = headPath(S, d.head);
  const hg = x.createRadialGradient(-0.1 * S, -0.24 * S, S * 0.04, 0, 0, S * 0.72);
  hg.addColorStop(0, shade(d.base[0], 0.14)); hg.addColorStop(0.6, d.base[0]); hg.addColorStop(1, d.base[1]);
  x.fillStyle = hg; x.fill(head);

  x.save(); x.clip(head);
  if (b === "corgiReal") {
    const w = new Path2D();
    w.moveTo(-0.04 * S, -0.52 * S); w.lineTo(0.04 * S, -0.52 * S);
    w.bezierCurveTo(0.055 * S, -0.24 * S, 0.07 * S, -0.04 * S, 0.16 * S, 0.06 * S);
    w.bezierCurveTo(0.3 * S, 0.12 * S, 0.44 * S, 0.2 * S, 0.44 * S, 0.38 * S);
    w.bezierCurveTo(0.44 * S, 0.62 * S, 0.2 * S, 0.7 * S, 0, 0.7 * S);
    w.bezierCurveTo(-0.2 * S, 0.7 * S, -0.44 * S, 0.62 * S, -0.44 * S, 0.38 * S);
    w.bezierCurveTo(-0.44 * S, 0.2 * S, -0.3 * S, 0.12 * S, -0.16 * S, 0.06 * S);
    w.bezierCurveTo(-0.07 * S, -0.04 * S, -0.055 * S, -0.24 * S, -0.04 * S, -0.52 * S);
    w.closePath();
    const wg = x.createLinearGradient(0, -0.5 * S, 0, 0.7 * S);
    wg.addColorStop(0, d.light!); wg.addColorStop(1, d.lightShade!);
    x.fillStyle = wg; x.fill(w);
  } else {
    const cap = new Path2D();
    cap.moveTo(-0.7 * S, -0.8 * S); cap.lineTo(0.7 * S, -0.8 * S); cap.lineTo(0.7 * S, -0.16 * S);
    cap.bezierCurveTo(0.42 * S, -0.22 * S, 0.22 * S, -0.26 * S, 0.1 * S, -0.2 * S);
    cap.bezierCurveTo(0.04 * S, -0.14 * S, 0.03 * S, 0, 0, 0.06 * S);
    cap.bezierCurveTo(-0.03 * S, 0, -0.04 * S, -0.14 * S, -0.1 * S, -0.2 * S);
    cap.bezierCurveTo(-0.22 * S, -0.26 * S, -0.42 * S, -0.22 * S, -0.7 * S, -0.16 * S); cap.closePath();
    const cg = x.createLinearGradient(0, -0.6 * S, 0, 0.06 * S);
    cg.addColorStop(0, d.dark!); cg.addColorStop(1, "#3A2D24");
    x.fillStyle = cg; x.fill(cap);
    x.fillStyle = "#E6B06E";
    for (const sd of [-1, 1]) { x.beginPath(); x.ellipse(sd * 0.19 * S, -0.235 * S, 0.055 * S, 0.028 * S, sd * 0.2, 0, Math.PI * 2); x.fill(); }
    const mask = new Path2D();
    mask.moveTo(-0.16 * S, 0.02 * S);
    mask.bezierCurveTo(-0.26 * S, 0.16 * S, -0.3 * S, 0.4 * S, -0.22 * S, 0.56 * S);
    mask.bezierCurveTo(-0.14 * S, 0.7 * S, 0.14 * S, 0.7 * S, 0.22 * S, 0.56 * S);
    mask.bezierCurveTo(0.3 * S, 0.4 * S, 0.26 * S, 0.16 * S, 0.16 * S, 0.02 * S);
    mask.quadraticCurveTo(0, 0.08 * S, -0.16 * S, 0.02 * S); mask.closePath();
    const mg = x.createLinearGradient(0, 0.02 * S, 0, 0.3 * S);
    mg.addColorStop(0, "rgba(28,22,18,0.15)"); mg.addColorStop(0.55, "rgba(28,22,18,0.85)"); mg.addColorStop(1, "rgba(24,19,16,0.97)");
    x.fillStyle = mg; x.fill(mask);
  }
  softBlob(x, -0.12 * S, -0.3 * S, 0.3 * S, 0.2 * S, "255,255,255", 0.22);
  softBlob(x, 0, d.noseY * S - 0.16 * S, 0.07 * S, 0.16 * S, "255,255,255", b === "corgiReal" ? 0.25 : 0.12);
  for (const sd of [-1, 1]) softBlob(x, sd * 0.3 * S, 0.14 * S, 0.18 * S, 0.14 * S, "255,255,255", 0.1);
  const vg = x.createRadialGradient(0, 0, S * 0.32, 0, 0.04 * S, S * 0.68);
  vg.addColorStop(0, "rgba(0,0,0,0)"); vg.addColorStop(1, "rgba(0,0,0,0.26)");
  x.fillStyle = vg; x.fill(head);
  x.restore();

  drawRealEye(x, S, d, -1, p.eye, p.open, p.gaze);
  drawRealEye(x, S, d, 1, p.eye, p.open, p.gaze);
  drawRealNose(x, S, d, p.eye === "happy", b === "shepherdReal");
  x.restore();
}

// ── Wardrobe icon ─────────────────────────────────────────────────────────────

const MOCHI_BODY: [string, string] = ["#EDEDEF", "#C4C5CA"];

/** A breed's button in the wardrobe, `size` px square. */
export function drawBreedIcon(x: Ctx, size: number, b: Breed) {
  x.clearRect(0, 0, size, size);
  x.save();
  if (isRealistic(b)) {
    x.translate(size / 2, size * 0.6);
    drawRealDog(x, b, { R: size * 0.3, eye: "open", open: 1, gaze: 0, alert: false, t: 0 });
    x.restore();
    return;
  }
  const R = size * 0.27, rx = R * 1.14, ry = R * 0.88;
  x.translate(size / 2, size * (b === "mochi" ? 0.52 : 0.62));
  drawCuteBehind(x, b, R, rx, ry, 0, 0, false);
  const body = new Path2D();
  const n = 48, e = 2 / 2.7;
  for (let i = 0; i <= n; i++) {
    const a = (i / n) * Math.PI * 2, c = Math.cos(a), s = Math.sin(a);
    const px = rx * Math.sign(c) * Math.pow(Math.abs(c), e), py = ry * Math.sign(s) * Math.pow(Math.abs(s), e);
    if (i) body.lineTo(px, py); else body.moveTo(px, py);
  }
  body.closePath();
  const [top, bottom] = cuteBody(b) ?? MOCHI_BODY;
  const g = x.createLinearGradient(rx * 0.7, -ry * 0.85, -rx * 0.8, ry * 0.9);
  g.addColorStop(0, top); g.addColorStop(1, bottom);
  x.fillStyle = g; x.fill(body);
  drawCuteMarkings(x, b, body, R, rx, ry, 0);
  x.fillStyle = "#1A1412";
  for (const sd of [-1, 1]) {
    const w = R * 0.16, h = R * 0.27;
    x.beginPath(); x.roundRect(sd * 0.36 * rx - w / 2, 0.1 * ry - h / 2, w, h, w / 2); x.fill();
  }
  drawCuteNose(x, b, body, R, rx, ry, 0, false);
  x.restore();
}
