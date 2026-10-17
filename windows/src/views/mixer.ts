// The island's sound mixer, opened from its button next to the gear: the music
// that plays (with its player's own level), the speakers (which output, their
// level, mute) and every program that has sound, as Windows' volume mixer
// shows them. src-tauri/src/mixer.rs reads and sets the levels.

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import { State } from "../core/state";
import { Bridge, type MixerApp, type MixerState } from "../core/bridge";
import { Spotify, currentArtwork, isAd, playerName, volumeLevel } from "../core/spotify";
import { setVolume as setMusicVolume, togglePlay } from "./spotify";
import { t, tl } from "../i18n/i18n";
import type { ViewHost } from "./views";

/** While the mixer is on screen, how often the levels are read again. */
const POLL_MS = 1000;
/** A slider just moved keeps its own value this long, so a read in flight can't pull it back. */
const HOLD_MS = 1500;
/** One wheel notch. */
const WHEEL_STEP = 5;
/** At most one level sent this often while a slider is dragged. */
const SEND_MS = 60;

function volumeIcon(volume: number, muted: boolean, size = 12): SVGSVGElement {
  const level = muted ? 0 : volumeLevel(volume);
  return svg([ICONS.volume0, ICONS.volume1, ICONS.volume2, ICONS.volume3][level], size, { stroke: 2.2 });
}

/** A level row: slider, number and mute, kept in place across reads. */
class Channel {
  readonly el: HTMLElement;
  private readonly slider: HTMLInputElement;
  private readonly value: HTMLElement;
  private readonly mute: HTMLButtonElement;
  private volume = 0;
  private muted = false;
  private heldUntil = 0;
  private dragging = false;
  private pending: number | null = null;
  private timer: number | null = null;

  constructor(
    head: HTMLElement,
    private readonly send: (volume: number | null, muted: boolean | null) => void,
  ) {
    this.slider = h("input", { type: "range", min: "0", max: "100", step: "1" }) as HTMLInputElement;
    this.value = h("span", { class: "mx-val" });
    this.mute = h("button", { class: "mx-mute", title: tl("Mute") }) as HTMLButtonElement;
    this.el = h("div", { class: "mx-ch" }, head, this.slider, this.value, this.mute);

    this.slider.addEventListener("pointerdown", () => (this.dragging = true));
    window.addEventListener("pointerup", () => (this.dragging = false));
    this.slider.addEventListener("input", () => this.change(Number(this.slider.value)));
    this.slider.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        this.change(this.volume + (e.deltaY < 0 ? WHEEL_STEP : -WHEEL_STEP));
      },
      { passive: false },
    );
    this.mute.addEventListener("click", () => {
      this.hold();
      this.muted = !this.muted;
      this.paint();
      this.send(null, this.muted);
    });
  }

  /** A read from Windows; ignored while the user is moving this one. */
  update(volume: number, muted: boolean) {
    if (this.dragging || Date.now() < this.heldUntil) return;
    this.volume = volume;
    this.muted = muted;
    this.paint();
  }

  private hold() {
    this.heldUntil = Date.now() + HOLD_MS;
  }

  private change(v: number) {
    this.hold();
    this.volume = Math.min(100, Math.max(0, Math.round(v)));
    // A level set by hand unmutes, as Windows' mixer does.
    this.muted = false;
    this.paint();
    this.pending = this.volume;
    if (this.timer != null) return;
    const flush = () => {
      const next = this.pending;
      this.pending = null;
      if (next == null) {
        this.timer = null;
        return;
      }
      this.send(next, null);
      this.timer = window.setTimeout(flush, SEND_MS);
    };
    flush();
  }

  private paint() {
    if (!this.dragging) this.slider.value = String(this.volume);
    this.slider.style.setProperty("--p", `${this.volume}%`);
    this.el.classList.toggle("muted", this.muted);
    this.value.textContent = this.muted ? "—" : String(this.volume);
    this.mute.title = this.muted ? t("Unmute") : t("Mute");
    clear(this.mute);
    this.mute.append(volumeIcon(this.volume, this.muted));
  }
}

export function buildMixer(): ViewHost {
  // ── Music ──
  const art = h("div", { class: "mx-art" });
  const title = h("b", {});
  const artist = h("small", {});
  const playBtn = h("button", { title: tl("Play"), onclick: () => togglePlay() });
  const music = h(
    "div",
    { class: "mx-music" },
    art,
    h("div", { class: "mx-track" }, title, artist),
    h(
      "div",
      { class: "mx-transport" },
      h("button", { title: tl("Previous"), onclick: () => void Bridge.spotifyControl("previous") }, svg(ICONS.backward, 12)),
      playBtn,
      h("button", { title: tl("Next"), onclick: () => void Bridge.spotifyControl("next") }, svg(ICONS.forward, 12)),
    ),
  );
  let musicMuteFrom = 50;
  const musicChannel = new Channel(h("span", { class: "mx-name", text: tl("Volume") }), (volume, muted) => {
    // The player's level has no mute of its own: muting is 0, unmuting goes back.
    if (volume != null) setMusicVolume(volume);
    else if (muted) {
      musicMuteFrom = Spotify.state.volume || musicMuteFrom;
      setMusicVolume(0);
    } else setMusicVolume(musicMuteFrom);
  });
  const musicBlock = h(
    "div",
    { class: "mx-block" },
    h("div", { class: "mx-sec", text: tl("Music") }),
    music,
    musicChannel.el,
  );

  // ── Speakers ──
  const devices = h("select", { class: "mx-device", title: tl("Speakers") }) as HTMLSelectElement;
  devices.addEventListener("change", () => {
    const id = devices.value;
    void Bridge.mixerSetDevice(id).then(() => read());
  });
  const master = new Channel(
    h("span", { class: "mx-name" }, svg(ICONS.speakerOn, 12), h("span", { text: tl("Speakers") })),
    (volume, muted) => void Bridge.mixerSetMaster(volume, muted),
  );
  const speakersBlock = h(
    "div",
    { class: "mx-block" },
    h("div", { class: "mx-sec" }, h("span", { text: tl("Speakers") }), h("div", { class: "grow" }), devices),
    master.el,
  );

  // ── Apps ──
  const appList = h("div", { class: "mx-apps" });
  const appsEmpty = h("div", { class: "mx-empty", text: tl("No app is making sound right now.") });
  const appRows = new Map<string, { channel: Channel; name: HTMLElement }>();
  const appsBlock = h(
    "div",
    { class: "mx-block mx-apps-block" },
    h(
      "div",
      { class: "mx-sec" },
      h("span", { text: tl("Apps") }),
      h("div", { class: "grow" }),
      h("button", { class: "link-btn mx-system", text: tl("Sound settings…"), onclick: () => void Bridge.mixerOpenSystem() }),
    ),
    appList,
    appsEmpty,
  );

  const unsupported = h("div", { class: "mx-empty", text: tl("The mixer works on Windows.") });

  const body = h("div", { class: "mx" }, musicBlock, speakersBlock, appsBlock, unsupported);
  const el = h("div", { class: "view" }, h("div", { class: "card" }, body));

  // ── Reading Windows ──
  let mixer: MixerState | null = null;
  let timer: number | null = null;
  let reading = false;

  async function read() {
    if (reading) return;
    reading = true;
    try {
      mixer = (await Bridge.mixerState()) ?? null;
    } finally {
      reading = false;
    }
    paintMixer();
  }

  function appName(app: MixerApp): string {
    if (app.key === "system") return t("System sounds");
    return app.name;
  }

  function paintMixer() {
    const s = mixer;
    const supported = s?.supported ?? false;
    speakersBlock.style.display = supported ? "" : "none";
    appsBlock.style.display = supported ? "" : "none";
    unsupported.style.display = s && !supported ? "" : "none";
    if (!s || !supported) return;

    // The outputs, rebuilt only when the list changes (an open list stays open).
    const key = s.devices.map((d) => `${d.id}\u0000${d.name}`).join("\u0001");
    if (devices.dataset.key !== key) {
      devices.dataset.key = key;
      clear(devices);
      for (const d of s.devices) devices.append(h("option", { value: d.id, text: d.name }));
    }
    const current = s.devices.find((d) => d.current);
    if (current && devices.value !== current.id && document.activeElement !== devices) devices.value = current.id;
    devices.style.display = s.devices.length > 1 ? "" : "none";
    devices.title = current?.name ?? t("Speakers");

    master.el.style.display = s.master ? "" : "none";
    if (s.master) master.update(s.master.volume, s.master.muted);

    // A program keeps its row while it has sound; rows come and go with it.
    const seen = new Set<string>();
    for (const app of s.apps) {
      seen.add(app.key);
      let row = appRows.get(app.key);
      if (!row) {
        const name = h("span", { class: "mx-name" });
        const appKey = app.key;
        const channel = new Channel(name, (volume, muted) => void Bridge.mixerSetApp(appKey, volume, muted));
        row = { channel, name };
        appRows.set(app.key, row);
      }
      const label = appName(app);
      if (row.name.textContent !== label) {
        row.name.textContent = label;
        row.name.title = label;
      }
      row.channel.el.classList.toggle("own", app.own);
      row.channel.update(app.volume, app.muted);
    }
    for (const [k, row] of appRows) {
      if (!seen.has(k)) {
        row.channel.el.remove();
        appRows.delete(k);
      }
    }
    // In the order Rust sorted them.
    s.apps.forEach((app, i) => {
      const node = appRows.get(app.key)!.channel.el;
      if (appList.children[i] !== node) appList.insertBefore(node, appList.children[i] ?? null);
    });
    appsEmpty.style.display = s.apps.length ? "none" : "";
  }

  function paintMusic() {
    const s = Spotify.state;
    const track = s.track;
    const shown = s.running && track != null;
    musicBlock.style.display = shown ? "" : "none";
    if (!shown || !track) return;
    const name = isAd(track) ? t("Advertisement") : track.title;
    if (title.textContent !== name) title.textContent = name;
    const who = [track.artist, playerName(s)].filter(Boolean).join(" · ");
    if (artist.textContent !== who) artist.textContent = who;
    const cover = currentArtwork(s);
    const bg = cover ? `url("${cover}")` : "";
    if (art.style.backgroundImage !== bg) art.style.backgroundImage = bg;
    art.classList.toggle("empty", !cover);
    clear(playBtn);
    playBtn.append(svg(s.playing ? ICONS.pause : ICONS.play, 12));
    playBtn.title = s.playing ? t("Pause") : t("Play");
    musicChannel.el.style.display = s.volumeSupported === false ? "none" : "";
    musicChannel.update(s.volume, s.volume === 0);
  }

  function syncPolling() {
    const on = State.mode === "expanded" && State.view === "mixer";
    if (on && timer == null) {
      void read();
      timer = window.setInterval(() => void read(), POLL_MS);
    } else if (!on && timer != null) {
      window.clearInterval(timer);
      timer = null;
    }
  }

  // Only the view on screen is synced: leaving it or folding the island stops the reads.
  State.subscribe(syncPolling);
  paintMixer();
  return {
    el,
    sync() {
      syncPolling();
      paintMusic();
    },
  };
}
