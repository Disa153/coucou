// The card after an update (src-tauri/src/promo.rs): a friend's project, Custom
// UI by sergey1029, a macOS-style shell for Windows 11. Short at first; "More"
// tells what it does. The link opens only on a click; nothing is fetched.

import { h, clear } from "./dom";
import { State } from "../core/state";
import { Bridge, IS_TAURI } from "../core/bridge";
import type { IslandViewName } from "../core/layout";
import { t, tl } from "../i18n/i18n";
import type { ViewActions, ViewHost } from "./views";
// Custom UI's own icon (its assets/icon.ico, MIT), bundled: nothing is fetched.
import iconUrl from "./customui.png";

export const PROMO_URL = "https://github.com/sergey1029/custom-ui";

/** What "More" lists. */
const DETAILS: (() => string)[] = [
  () => t("A menu bar with the app's menus, tray icons and a clock"),
  () => t("A Dock with your apps, growing under the pointer"),
  () => t("Spotlight: apps, files, the web and a calculator (Win or Alt+Space)"),
  () => t("Launchpad, a Control Center, a calendar and widgets"),
  () => t("Your windows stay as they are: only what is around them changes"),
  () => t("Free and open source (MIT), no admin rights to install. Not made by or linked to Apple."),
];

export interface PromoHost {
  alert(view: IslandViewName): void;
}

/** After the greeting: the card, once per new build, if nobody is busy with the island. */
export async function checkPromo(host: PromoHost) {
  if (!IS_TAURI || State.paused || State.pendingApproval) return;
  if (State.mode === "expanded" && State.view !== "overview" && State.view !== "empty") return;
  if (await Bridge.promoDue()) host.alert("promo");
}

export function buildPromo(actions: ViewActions): ViewHost {
  const title = h("b", { class: "pr-title", text: tl("Custom UI, by sergey1029") });
  const line = h("div", { class: "pr-line", text: tl("Windows 11 in the style of macOS: a menu bar, a Dock, Spotlight and Launchpad.") });
  const more = h("div", { class: "pr-more" });
  const moreBtn = h("button", { class: "btn secondary" }, h("span", { text: tl("More") }));
  const open = h("button", { class: "btn primary" }, h("span", { text: tl("Open on GitHub") }));
  const later = h("button", { class: "btn secondary" }, h("span", { text: tl("Not now") }));
  const never = h("button", { class: "pr-never", text: tl("Don't show again") });
  const panel = h(
    "div",
    { class: "rv-panel pr-panel" },
    h("div", { class: "pr-head" },
      h("img", { class: "pr-icon", src: iconUrl, alt: "" }),
      h("div", {}, h("div", { class: "pr-tag", text: tl("A friend's project") }), title),
    ),
    line,
    more,
    h("div", { class: "rv-row" }, never, h("span", { class: "rv-grow" }), moreBtn, later, open),
  );
  const el = h("div", { class: "view review promo" }, panel);
  let expanded = false;

  function paint() {
    clear(more);
    more.style.display = expanded ? "" : "none";
    line.style.display = expanded ? "none" : "";
    if (expanded) {
      const list = h("ul", {});
      for (const d of DETAILS) list.append(h("li", { text: d() }));
      more.append(list);
    }
    moreBtn.style.display = expanded ? "none" : "";
  }

  const close = (forGood: boolean) => {
    void Bridge.promoDone(forGood);
    actions.setView(State.defaultView());
    actions.collapse();
  };
  moreBtn.addEventListener("click", () => {
    expanded = true;
    paint();
  });
  open.addEventListener("click", () => {
    actions.openUrl(PROMO_URL);
    close(false);
  });
  later.addEventListener("click", () => close(false));
  never.addEventListener("click", () => close(true));

  // Read, not clicked at once: the island stays open while it shows.
  let held = false;
  State.subscribe(() => {
    const on = State.mode === "expanded" && State.view === "promo";
    if (on !== held) {
      held = on;
      actions.holdOpen(on);
      // Seen: it does not come back for this build, even if the island folds.
      if (on) void Bridge.promoDone(false);
      else {
        expanded = false;
        paint();
      }
    }
  });

  paint();
  return { el, sync() {} };
}
