// Settings → Account → Beta chat (src-tauri/src/betachat.rs): the sound of a new
// message — Coucou's own or the user's file, tried with ▶ — its volume, quiet
// while busy, and the chat's own window. For the owner and the beta testers.

import { Bridge, onEvent, type AccountState, type BetaChatPrefs } from "../core/bridge";
import { CHAT_SOUNDS, playChatSound, type ChatSound } from "../core/chatsound";
import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";
import { reviewErrorText } from "../views/review";

type Toggle = (on: boolean, onChange: (v: boolean) => void) => HTMLElement;

const SOUND_MAX = 2 * 1024 * 1024;

const LABELS: Record<ChatSound, () => [string, string, string]> = {
  hee: () => ["🕺", t("“Hee-hee!”"), t("In the spirit of Michael Jackson, a sound of Coucou's own")],
  chime: () => ["📱", t("Three notes"), t("Like a message on a phone")],
  quack: () => ["🦆", t("Quack"), t("A duck")],
  knock: () => ["🚪", t("Knock on the door"), t("Knock, knock, knock")],
  mochi: () => ["🍙", "Mochi", t("Coucou's blip")],
  own: () => ["📂", t("Your own sound…"), t("A WAV, MP3 or OGG file, a few seconds long")],
  none: () => ["🔕", t("No sound"), ""],
};

export function betaChatSection(makeToggle: Toggle): HTMLElement {
  const section = h("section", {});
  section.style.display = "none";
  let prefs: BetaChatPrefs | null = null;
  const note = h("div", { class: "hint" });
  const picker = h("input", { type: "file", accept: "audio/*", style: "display:none" }) as HTMLInputElement;

  const store = (change: Partial<BetaChatPrefs>) => {
    if (!prefs) return;
    void Bridge.betaChatSetPrefs({ ...prefs, ...change }).then((p) => {
      if (p) prefs = p;
      draw();
    });
  };

  picker.addEventListener("change", () => {
    const f = picker.files?.[0];
    picker.value = "";
    if (!f) return;
    if (f.size > SOUND_MAX) {
      note.textContent = t("That sound is over 2 MB: pick a shorter one.");
      return;
    }
    void f.arrayBuffer().then((buf) => {
      const bytes = new Uint8Array(buf);
      let s = "";
      for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      Bridge.betaChatSetOwnSound(f.name, btoa(s))
        .then((p) => {
          prefs = p;
          note.textContent = "";
          draw();
          void playChatSound("own", p.volume, p.ownName);
        })
        .catch((e: unknown) => (note.textContent = reviewErrorText(e)));
    });
  });

  function draw() {
    clear(section);
    if (!prefs) return;
    const p = prefs;
    const list = h("div", { class: "bcs-list" });
    for (const id of [...CHAT_SOUNDS, "none" as const]) {
      const [icon, name, sub] = LABELS[id]();
      const subText = id === "own" && p.ownName ? p.ownName : sub;
      const play = h("button", { class: "bcs-play", title: t("Play"), text: "▶" });
      play.addEventListener("click", (e) => {
        e.stopPropagation();
        if (id === "own" && !p.ownName) picker.click();
        else void playChatSound(id, p.volume, p.ownName);
      });
      const item = h("div", { class: id === p.sound ? "bcs-item on" : "bcs-item" },
        h("span", { class: "bcs-radio" }),
        h("span", { class: "bcs-icon", text: icon }),
        h("div", { class: "acc-grow" }, h("b", { text: name }), subText ? h("div", { class: "hint", text: subText }) : null),
        id === "none" ? null : play,
      );
      item.addEventListener("click", () => {
        if (id === "own" && !p.ownName) picker.click();
        else store({ sound: id });
      });
      list.append(item);
    }
    const volume = h("input", { type: "range", min: "0", max: "100", value: String(Math.round(p.volume * 100)) }) as HTMLInputElement;
    volume.addEventListener("change", () => store({ volume: Number(volume.value) / 100 }));
    const pickOwn = h("button", { text: p.ownName ? t("Another file…") : t("Choose a file…") });
    pickOwn.addEventListener("click", () => picker.click());
    const win = h("button", { text: t("Open the chat window") });
    win.addEventListener("click", () => void Bridge.betaChatWindow());
    section.append(
      h("h2", {}, h("span", { text: t("Beta chat") })),
      h("div", { class: "subhead", text: t("Sound of a new message") }),
      list,
      h("div", { class: "row" }, pickOwn, picker),
      h("div", { class: "row" }, h("label", { text: t("Volume") }), volume),
      h("div", { class: "row" },
        h("label", { text: t("Quiet while busy") }),
        makeToggle(p.quiet, (on) => store({ quiet: on })),
        h("span", { class: "hint", text: t("No sound while an agent works, a question waits or music plays") }),
      ),
      h("div", { class: "row" }, win),
      note,
      h("div", { class: "hint", text: t("Coucou's real sounds are its own: the voice of a famous song or a phone's ringtone cannot ship with it. Your own file stays on this computer.") }),
    );
  }

  const show = (s: AccountState | null) => {
    const on = !!s?.signedIn && (s.owner || s.tester);
    section.style.display = on ? "" : "none";
    if (on && !prefs) {
      void Bridge.betaChatPrefs().then((p) => {
        prefs = p;
        draw();
      });
    }
  };
  void Bridge.accountState().then(show);
  void onEvent<AccountState>("account-changed", show);
  void onEvent<BetaChatPrefs>("beta-chat-prefs", (p) => {
    prefs = p;
    draw();
  });
  return section;
}
