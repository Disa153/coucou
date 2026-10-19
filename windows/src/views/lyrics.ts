// The island's Lyrics view, opened from the music card: the playing song's
// words (any player Windows reports: Spotify, Яндекс Музыка, a browser tab),
// the line being sung lit and kept in the middle, a click on a line seeks
// there. The words come from LRCLIB once the user turns lyrics on
// (src-tauri/src/lyrics.rs); timed lines follow the song, plain ones just show.

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import { State } from "../core/state";
import { Bridge } from "../core/bridge";
import { Spotify, isAd, playerName, spotifyPosition } from "../core/spotify";
import { lineAt, parseLrc, plainLines, type LyricLine } from "../core/lyrics";
import { seek } from "./spotify";
import { t, tl } from "../i18n/i18n";
import type { ViewActions, ViewHost } from "./views";

const TICK_MS = 200;
/** Lyrics a little ahead feel in time (the line shows as it is sung). */
const LEAD_S = 0.25;

type Shown =
  | { kind: "off" }
  | { kind: "loading" }
  | { kind: "none"; instrumental: boolean }
  | { kind: "error" }
  | { kind: "plain"; lines: string[] }
  | { kind: "synced"; lines: LyricLine[] };

export function buildLyrics(actions: ViewActions): ViewHost {
  const back = h("button", { class: "ly-back", title: tl("Back"), onclick: () => actions.setView("overview") }, svg(ICONS.chevronLeft, 12, { stroke: 2.2 }));
  const title = h("b", {});
  const sub = h("small", {});
  const head = h("div", { class: "ly-head" }, back, h("div", { class: "ly-title" }, title, sub));
  const lines = h("div", { class: "ly-lines" });
  const viewport = h("div", { class: "ly-viewport" }, lines);
  const note = h("div", { class: "ly-note" });
  const body = h("div", { class: "ly-body" }, viewport, note);
  const el = h("div", { class: "view lyrics" }, h("div", { class: "card ly-card" }, head, body));

  let shown: Shown = { kind: "loading" };
  let key = "";
  let current = -2;
  let timer: number | null = null;
  let held = false;

  function songKey(): string {
    const tr = Spotify.state.track;
    return tr ? `${tr.title}\u0000${tr.artist}\u0000${Math.round(tr.duration)}` : "";
  }

  function paintNote(text: string, button?: { label: string; run: () => void }) {
    clear(note);
    note.style.display = text ? "" : "none";
    if (!text) return;
    note.append(h("span", { text }));
    if (button) note.append(h("button", { class: "btn primary", onclick: button.run }, h("span", { text: button.label })));
  }

  function paint() {
    clear(lines);
    current = -2;
    lines.style.transform = "";
    switch (shown.kind) {
      case "off":
        paintNote(t("Lyrics come from LRCLIB, an open database of song words on the internet. Coucou asks it only for the song that plays."), {
          label: t("Turn on lyrics"),
          run: () => {
            State.settings.lyrics = true;
            void Bridge.saveSettings(State.settings).then(() => load(true));
          },
        });
        return;
      case "loading":
        return paintNote(t("Looking for the words…"));
      case "error":
        return paintNote(t("LRCLIB could not be reached. Try again in a moment."));
      case "none":
        return paintNote(shown.instrumental ? t("This one has no words.") : t("No lyrics found for this song."));
      case "plain":
        paintNote("");
        for (const line of shown.lines) lines.append(h("div", { class: "ly-line plain", text: line || " " }));
        return;
      case "synced":
        paintNote("");
        shown.lines.forEach((line) => {
          const row = h("div", { class: "ly-line", text: line.text || "♪" });
          row.addEventListener("click", () => seek(line.t));
          lines.append(row);
        });
        follow();
    }
  }

  /** The line being sung, lit and brought to the middle. */
  function follow() {
    if (shown.kind !== "synced") return;
    const at = lineAt(shown.lines, spotifyPosition(Spotify.state, Date.now()) + LEAD_S);
    if (at === current) return;
    current = at;
    Array.from(lines.children).forEach((row, i) => {
      row.classList.toggle("ly-on", i === at);
      row.classList.toggle("past", i < at);
    });
    const row = lines.children[Math.max(0, at)] as HTMLElement | undefined;
    if (row) lines.style.transform = `translateY(${viewport.clientHeight * 0.4 - row.offsetTop}px)`;
  }

  async function load(force = false) {
    const tr = Spotify.state.track;
    const k = songKey();
    if (!force && k === key) return;
    key = k;
    if (!tr || isAd(tr)) {
      shown = { kind: "none", instrumental: false };
      return paint();
    }
    if (!State.settings.lyrics) {
      shown = { kind: "off" };
      return paint();
    }
    shown = { kind: "loading" };
    paint();
    try {
      const found = await Bridge.lyricsGet(tr.title, tr.artist, tr.album, tr.duration);
      if (key !== k) return;
      const timed = found.synced ? parseLrc(found.synced) : [];
      shown = timed.length
        ? { kind: "synced", lines: timed }
        : found.plain
          ? { kind: "plain", lines: plainLines(found.plain) }
          : { kind: "none", instrumental: found.instrumental };
    } catch (e) {
      if (key !== k) return;
      shown = String(e).includes("off") ? { kind: "off" } : { kind: "error" };
    }
    paint();
  }

  function syncTimer(on: boolean) {
    if (on && timer == null) timer = window.setInterval(follow, TICK_MS);
    else if (!on && timer != null) {
      window.clearInterval(timer);
      timer = null;
    }
  }

  // Only the view on screen is synced: leaving it stops the clock and lets the island fold again.
  State.subscribe(() => {
    const on = State.mode === "expanded" && State.view === "lyrics";
    syncTimer(on && Spotify.state.playing);
    if (on !== held) {
      held = on;
      actions.holdOpen(on);
    }
  });

  return {
    el,
    sync() {
      const tr = Spotify.state.track;
      title.textContent = tr ? (isAd(tr) ? t("Advertisement") : tr.title) : t("Nothing is playing");
      sub.textContent = tr ? [tr.artist, playerName(Spotify.state)].filter(Boolean).join(" · ") : "";
      void load();
      follow();
    },
  };
}
