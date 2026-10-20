// The beta chat in the island (src-tauri/src/betachat.rs): the owner and the
// beta testers, signed in with GitHub. Read every 30 s while on screen; the
// island stays open meanwhile. Enter sends, Shift+Enter starts a new line.

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import { State } from "../core/state";
import { Bridge, type ChatMessage } from "../core/bridge";
import { t, tl } from "../i18n/i18n";
import { reviewErrorText } from "./review";
import { BetaChat } from "../island/betachat";
import type { ViewActions, ViewHost } from "./views";

const POLL_MS = 30_000;
const COLORS = ["#f9a8d4", "#93c5fd", "#86efac", "#fcd34d", "#c4b5fd", "#fdba74", "#5eead4"];

function color(login: string): string {
  let sum = 0;
  for (const c of login.toLowerCase()) sum += c.charCodeAt(0);
  return COLORS[sum % COLORS.length];
}

function clock(at: number): string {
  const d = new Date(at * 1000);
  const today = new Date().toDateString() === d.toDateString();
  return today
    ? d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

export function buildBetaChat(actions: ViewActions): ViewHost {
  const back = h("button", { class: "ly-back", title: tl("Back"), onclick: () => actions.setView("overview") }, svg(ICONS.chevronLeft, 12, { stroke: 2.2 }));
  const openGh = h("button", { class: "bc-link", text: tl("On GitHub") });
  const head = h("div", { class: "bc-head" }, back, h("b", { text: tl("Beta chat") }), h("span", { class: "bc-grow" }), openGh);
  const list = h("div", { class: "bc-list" });
  const note = h("div", { class: "bc-note" });
  const input = h("textarea", { class: "bc-input", rows: "1", maxlength: "2000" }) as HTMLTextAreaElement;
  input.placeholder = t("Write to the testers… (not passwords or keys: the chat is public on GitHub)");
  let withBuild = false;
  const buildChip = h("button", { class: "bc-chip" });
  const send = h("button", { class: "bc-send", title: tl("Send") }, svg(ICONS.arrowUp, 13, { stroke: 2.2 }));
  const composer = h("div", { class: "bc-composer" }, buildChip, input, send);
  const el = h("div", { class: "view betachat" }, h("div", { class: "card bc-card" }, head, list, note, composer));

  let page = "";
  let owner = false;
  let timer: number | null = null;
  let held = false;
  let lastId = 0;
  let build = 0;
  void Bridge.updateInfo().then((i) => {
    build = i?.build ?? 0;
    paintChip();
  });

  openGh.addEventListener("click", () => {
    if (page) actions.openUrl(page);
  });
  buildChip.addEventListener("click", () => {
    withBuild = !withBuild;
    paintChip();
  });
  function paintChip() {
    buildChip.style.display = build > 0 ? "" : "none";
    buildChip.classList.toggle("on", withBuild);
    buildChip.textContent = `🧪 ${build}`;
    buildChip.title = t("Attach your build number");
  }

  function paint(messages: ChatMessage[]) {
    const atBottom = list.scrollTop + list.clientHeight >= list.scrollHeight - 30;
    clear(list);
    if (!messages.length) list.append(h("div", { class: "bc-empty", text: t("No messages yet. Say hello to the testers!") }));
    for (const m of messages) {
      const av = h("span", { class: "bc-av", text: m.login.charAt(0).toUpperCase() });
      av.style.background = color(m.login);
      const who = h("div", { class: "bc-who" }, h("span", { text: `@${m.login}` }));
      if (m.owner) who.append(h("span", { class: "bc-owner", text: ` · ${t("owner")}` }));
      const bubble = h("div", { class: "bc-bubble" });
      if (m.build) bubble.append(h("span", { class: "bc-tag", text: t("🧪 build {build}", { build: m.build }) }));
      bubble.append(document.createTextNode(m.text));
      const meta = h("div", { class: "bc-time", text: m.at ? clock(m.at) : "" });
      if (m.mine || owner) {
        const del = h("button", { class: "bc-del", title: tl("Delete"), text: "×" });
        del.addEventListener("click", () => {
          del.setAttribute("disabled", "");
          Bridge.betaChatDelete(m.id)
            .then(() => void refresh())
            .catch((e: unknown) => {
              note.textContent = reviewErrorText(e);
            });
        });
        meta.append(del);
      }
      list.append(h("div", { class: m.mine ? "bc-msg mine" : "bc-msg" }, av, h("div", { class: "bc-body" }, who, bubble, meta)));
    }
    const newest = messages.at(-1)?.id ?? 0;
    if (atBottom || newest !== lastId) list.scrollTop = list.scrollHeight;
    lastId = newest;
  }

  async function refresh() {
    try {
      const p = await Bridge.betaChatMessages();
      page = p.page;
      owner = p.owner;
      openGh.style.display = page ? "" : "none";
      note.textContent = "";
      BetaChat.unread = 0;
      paint(p.messages);
    } catch (e) {
      note.textContent = reviewErrorText(e);
    }
  }

  function submit() {
    const text = input.value.trim();
    if (!text) return;
    send.setAttribute("disabled", "");
    note.textContent = t("Sending…");
    Bridge.betaChatSend(text, withBuild)
      .then(() => {
        input.value = "";
        withBuild = false;
        paintChip();
        void refresh();
      })
      .catch((e: unknown) => {
        note.textContent = reviewErrorText(e);
      })
      .finally(() => send.removeAttribute("disabled"));
  }
  send.addEventListener("click", submit);
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  });

  State.subscribe(() => {
    const on = State.mode === "expanded" && State.view === "betachat";
    if (on !== held) {
      held = on;
      actions.holdOpen(on);
      if (on) {
        void refresh();
        timer = window.setInterval(() => void refresh(), POLL_MS);
      } else if (timer != null) {
        window.clearInterval(timer);
        timer = null;
      }
    }
  });

  return {
    el,
    sync() {
      paintChip();
    },
    focus() {
      input.focus();
    },
  };
}
