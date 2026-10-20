// The beta chat in the island (src-tauri/src/betachat.rs): the panel shared
// with the chat's own window (chat/panel.ts), with a back button and ⧉ to open
// that window. The island stays open while it shows.

import { h, svg } from "./dom";
import { ICONS } from "./icons";
import { State } from "../core/state";
import { Bridge, emitToWindow } from "../core/bridge";
import { tl } from "../i18n/i18n";
import { createChatPanel } from "../chat/panel";
import { BetaChat } from "../island/betachat";
import type { ViewActions, ViewHost } from "./views";

export function buildBetaChat(actions: ViewActions): ViewHost {
  const back = h("button", { class: "ly-back", title: tl("Back"), onclick: () => actions.setView("overview") }, svg(ICONS.chevronLeft, 12, { stroke: 2.2 }));
  const popout = h("button", { class: "bc-link", title: tl("Open the chat in its own window"), text: tl("⧉ Window") });
  popout.addEventListener("click", () => {
    void Bridge.betaChatWindow();
    actions.setView(State.defaultView());
    actions.collapse();
  });
  const panel = createChatPanel({
    openUrl: (url) => actions.openUrl(url),
    onRead: () => {
      BetaChat.unread = 0;
    },
    lead: [back],
    tail: [popout],
    // The island is small: a photo opens in the chat's own window.
    photoClick: (f) => {
      void Bridge.betaChatWindow().then(() => emitToWindow("betachat", "beta-chat-photo", f.url));
      actions.setView(State.defaultView());
      actions.collapse();
    },
  });
  const el = h("div", { class: "view betachat" }, h("div", { class: "card bc-card" }, panel.el));

  let held = false;
  State.subscribe(() => {
    const on = State.mode === "expanded" && State.view === "betachat";
    if (on !== held) {
      held = on;
      actions.holdOpen(on);
      panel.shown(on);
    }
  });

  return {
    el,
    sync() {},
    focus() {
      panel.focus();
    },
  };
}
