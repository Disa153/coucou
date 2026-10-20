// The island's review card (src-tauri/src/reviews.rs): a while after a build
// first started, Mochi asks how it is — one to five stars, what to add, what to
// take away. Sent as the signed-in GitHub account; read in Settings → Reviews.

import { h, clear } from "./dom";
import { State } from "../core/state";
import { Bridge, type ReviewError } from "../core/bridge";
import { t, tl } from "../i18n/i18n";
import type { ViewActions, ViewHost } from "./views";

/** The build being asked about (island/reviews.ts sets it before opening the card). */
export const ReviewAsk = { build: 0 };

const STAR_WORDS = [() => t("Poor"), () => t("Not great"), () => t("OK"), () => t("Good"), () => t("Great")];

export function reviewErrorText(e: unknown): string {
  const code = (typeof e === "string" ? e : "network") as ReviewError;
  return ({
    notSignedIn: t("Sign in with GitHub first."),
    signInAgain: t("Sign in again in Settings → Account so Coucou may send reviews."),
    notAllowed: t("Only the beta testers and Coucou's owner see the reviews."),
    noStars: t("Pick a number of stars first."),
    tooBig: t("Too big for the chat: at most 4 attachments of 7 MB each."),
    network: t("GitHub could not be reached. Try again later."),
  } as Record<ReviewError, string>)[code] ?? t("GitHub could not be reached. Try again later.");
}

export function buildReview(actions: ViewActions): ViewHost {
  const title = h("b", { class: "rv-title" });
  let stars = 0;
  const starRow = h("div", { class: "rv-stars" });
  const word = h("span", { class: "rv-word" });
  const starButtons: HTMLElement[] = [];
  for (let i = 1; i <= 5; i++) {
    const b = h("button", { class: "rv-star", "aria-label": tl("Stars: {count}", { count: i }), text: "★" });
    b.addEventListener("click", () => {
      stars = i;
      paintStars();
    });
    starButtons.push(b);
    starRow.append(b);
  }
  const add = h("textarea", { class: "rv-text", maxlength: "1000" }) as HTMLTextAreaElement;
  add.placeholder = t("➕ What to add: what is missing");
  const remove = h("textarea", { class: "rv-text", maxlength: "1000" }) as HTMLTextAreaElement;
  remove.placeholder = t("➖ What to take away or change: what gets in the way");
  const note = h("span", { class: "rv-note" });
  const later = h("button", { class: "btn secondary" }, h("span", { text: tl("Later") }));
  const send = h("button", { class: "btn primary" }, h("span", { text: tl("Send") }));
  const form = h(
    "div",
    { class: "rv-form" },
    title,
    h("div", { class: "rv-row" }, starRow, word),
    add,
    remove,
    h("div", { class: "rv-row" }, note, h("span", { class: "rv-grow" }), later, send),
  );
  const thanks = h("div", { class: "rv-thanks" });
  const panel = h("div", { class: "rv-panel" }, form, thanks);
  const el = h("div", { class: "view review" }, panel);

  function paintStars() {
    starButtons.forEach((b, i) => b.classList.toggle("on", i < stars));
    word.textContent = stars ? STAR_WORDS[stars - 1]() : t("Tap a star");
    (send as HTMLButtonElement).disabled = stars === 0;
  }

  later.addEventListener("click", () => {
    void Bridge.reviewLater(ReviewAsk.build);
    actions.setView(State.defaultView());
    actions.collapse();
  });
  send.addEventListener("click", () => {
    if (!stars) return;
    (send as HTMLButtonElement).disabled = true;
    note.textContent = t("Sending…");
    Bridge.reviewSend(ReviewAsk.build, stars, add.value, remove.value)
      .then(() => {
        form.style.display = "none";
        clear(thanks);
        thanks.append(
          h("b", { text: t("Thank you! 🍙") }),
          h("span", { text: t("Mochi passed it on. The reviews are in Settings → Reviews.") }),
        );
        thanks.style.display = "";
        window.setTimeout(() => {
          if (State.view === "review") {
            actions.setView(State.defaultView());
            actions.collapse();
          }
        }, 3500);
      })
      .catch((e: unknown) => {
        (send as HTMLButtonElement).disabled = false;
        note.textContent = reviewErrorText(e);
      });
  });

  // Typing must not let the island fold under the user.
  let held = false;
  let shownFor = -1;
  State.subscribe(() => {
    const on = State.mode === "expanded" && State.view === "review";
    if (on !== held) {
      held = on;
      actions.holdOpen(on);
    }
  });

  return {
    el,
    sync() {
      if (shownFor !== ReviewAsk.build) {
        shownFor = ReviewAsk.build;
        stars = 0;
        add.value = "";
        remove.value = "";
        note.textContent = "";
        form.style.display = "";
        thanks.style.display = "none";
      }
      title.textContent = t("How is build {build}?", { build: ReviewAsk.build });
      paintStars();
    },
    focus() {
      starButtons[0]?.focus();
    },
  };
}
