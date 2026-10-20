// The beta chat's own window (chat.html), opened with ⧉ from the island's chat:
// the same panel (./panel.ts), files may be dropped on it, and 📌 keeps it
// above the other windows. Closing it only hides it (betachat.rs).

import "./chat.css";
import { Bridge, onEvent } from "../core/bridge";
import type { Settings } from "../core/state";
import { h } from "../views/dom";
import { resolveLanguage, setLanguage, systemLanguages, t, tl } from "../i18n/i18n";
import { createChatPanel } from "./panel";

async function main() {
  const boot = await Bridge.boot();
  setLanguage(resolveLanguage(boot?.settings.language, systemLanguages()));
  void onEvent<Settings>("settings-changed", (s) => setLanguage(resolveLanguage(s.language, systemLanguages())));
  document.title = t("Coucou — Beta chat");

  let pinned = false;
  const pin = h("button", { class: "bc-link cw-pin", title: tl("Keep above other windows"), text: tl("📌 On top") });
  pin.addEventListener("click", () => {
    pinned = !pinned;
    pin.classList.toggle("on", pinned);
    void Bridge.betaChatPin(pinned);
  });
  const panel = createChatPanel({ openUrl: (url) => void Bridge.openUrl(url), acceptsDrops: true, tail: [pin] });
  document.getElementById("chat")!.append(panel.el);

  // Read while the window is on screen, not while it is hidden.
  const follow = () => panel.shown(document.visibilityState === "visible");
  document.addEventListener("visibilitychange", follow);
  void onEvent<string>("beta-chat-photo", (url) => {
    panel.shown(true);
    panel.openPhoto(url);
  });
  void onEvent<null>("beta-chat-shown", () => {
    panel.shown(true);
    panel.focus();
  });
  follow();
}

void main();
