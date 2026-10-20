// The beta chat's sound for a new message (src-tauri/src/betachat.rs keeps
// which one). All of Coucou's own are made here, note by note, with Web Audio:
// "hee" a falsetto whoop in the spirit of the King of Pop, "chime" three notes
// like a phone's message, "quack" a duck, "knock" three knocks on a door,
// "mochi" Coucou's blip — none is a recording. "own" is the user's file.

import { Bridge } from "./bridge";

export type ChatSound = "hee" | "chime" | "quack" | "knock" | "mochi" | "own" | "none";
export const CHAT_SOUNDS: ChatSound[] = ["hee", "chime", "quack", "knock", "mochi", "own"];

let ctx: AudioContext | null = null;
const audio = () => (ctx ??= new AudioContext());

function tone(f: number, t0: number, d: number, vol: number, type: OscillatorType = "sine", glide?: number): OscillatorNode {
  const a = audio();
  const o = a.createOscillator();
  const g = a.createGain();
  const at = a.currentTime + t0;
  o.type = type;
  o.frequency.setValueAtTime(f, at);
  if (glide) o.frequency.exponentialRampToValueAtTime(glide, at + d);
  g.gain.setValueAtTime(0.0001, at);
  g.gain.linearRampToValueAtTime(vol, at + 0.012);
  g.gain.exponentialRampToValueAtTime(0.0001, at + d);
  o.connect(g).connect(a.destination);
  o.start(at);
  o.stop(at + d + 0.03);
  return o;
}

const MADE: Record<Exclude<ChatSound, "own" | "none">, (v: number) => void> = {
  hee(v) {
    const a = audio();
    const o = tone(700, 0, 0.34, 0.3 * v, "triangle", 1500);
    const lfo = a.createOscillator();
    const depth = a.createGain();
    lfo.frequency.value = 9;
    depth.gain.value = 40;
    lfo.connect(depth).connect(o.frequency);
    lfo.start();
    lfo.stop(a.currentTime + 0.4);
    tone(1300, 0.37, 0.22, 0.24 * v, "triangle", 1700);
    tone(1700, 0.6, 0.12, 0.16 * v, "sine", 1200);
  },
  chime(v) {
    tone(1318.5, 0, 0.35, 0.26 * v);
    tone(1046.5, 0.14, 0.35, 0.26 * v);
    tone(1568, 0.28, 0.6, 0.26 * v);
  },
  quack(v) {
    const a = audio();
    for (const t0 of [0, 0.22]) {
      const at = a.currentTime + t0;
      const o = a.createOscillator();
      const f = a.createBiquadFilter();
      const g = a.createGain();
      o.type = "sawtooth";
      o.frequency.setValueAtTime(260, at);
      o.frequency.linearRampToValueAtTime(180, at + 0.16);
      f.type = "bandpass";
      f.Q.value = 4;
      f.frequency.setValueAtTime(1400, at);
      f.frequency.linearRampToValueAtTime(700, at + 0.16);
      g.gain.setValueAtTime(0.0001, at);
      g.gain.linearRampToValueAtTime(0.6 * v, at + 0.02);
      g.gain.exponentialRampToValueAtTime(0.001, at + 0.17);
      o.connect(f).connect(g).connect(a.destination);
      o.start(at);
      o.stop(at + 0.2);
    }
  },
  knock(v) {
    const a = audio();
    for (const t0 of [0, 0.18, 0.36]) {
      const at = a.currentTime + t0;
      const buf = a.createBuffer(1, Math.round(a.sampleRate * 0.08), a.sampleRate);
      const d = buf.getChannelData(0);
      for (let i = 0; i < d.length; i++) d[i] = (Math.random() * 2 - 1) * Math.exp(-i / (a.sampleRate * 0.012));
      const n = a.createBufferSource();
      n.buffer = buf;
      const f = a.createBiquadFilter();
      f.type = "lowpass";
      f.frequency.value = 600;
      const g = a.createGain();
      g.gain.value = 1.4 * v;
      n.connect(f).connect(g).connect(a.destination);
      n.start(at);
      tone(110, t0, 0.09, 0.5 * v);
    }
  },
  mochi(v) {
    tone(880, 0, 0.12, 0.25 * v, "sine", 1320);
  },
};

let own: { name: string; url: string } | null = null;

async function ownUrl(name: string): Promise<string | null> {
  if (own?.name === name) return own.url;
  const data = await Bridge.betaChatOwnSound();
  if (!data) return null;
  const s = atob(data);
  const bytes = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) bytes[i] = s.charCodeAt(i);
  if (own) URL.revokeObjectURL(own.url);
  own = { name, url: URL.createObjectURL(new Blob([bytes])) };
  return own.url;
}

/** Plays `sound` at `volume` (0…1). `ownName` tells a new file of the user's from the last one. */
export async function playChatSound(sound: ChatSound, volume: number, ownName = ""): Promise<void> {
  if (sound === "none" || volume <= 0) return;
  if (sound === "own") {
    const url = await ownUrl(ownName);
    if (!url) return MADE.chime(volume);
    const el = new Audio(url);
    el.volume = Math.min(1, volume);
    await el.play().catch(() => undefined);
    return;
  }
  if (audio().state === "suspended") await audio().resume().catch(() => undefined);
  MADE[sound](volume);
}
