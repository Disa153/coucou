// The Kitchen, opened from the onigiri next to the mixer: snacks to drag onto
// Mochi (he opens his mouth, chews and blushes), how full he is, and the
// moonwalk he learns after a few snacks (core/pet.ts). The island does the
// eating and the dancing (island.ts snackOver / eatSnack / moonwalk).

import { h } from "./dom";
import { State } from "../core/state";
import { SNACKS, MOONWALK_TREATS, canMoonwalk, fullNow, hungry, readPet, treatsLeft } from "../core/pet";
import { t, tl } from "../i18n/i18n";
import type { ViewActions, ViewHost } from "./views";

/** While the Kitchen is on screen, how often the fullness is read again (it drops slowly). */
const REFRESH_MS = 30_000;

export function buildKitchen(actions: ViewActions): ViewHost {
  const fullBar = h("i", {});
  const fullText = h("span", {});
  const mood = h("div", { class: "kt-mood" });

  const snacks = h("div", { class: "kt-snacks" });
  for (const snack of SNACKS) {
    const el = h("button", { class: "kt-snack", "aria-label": tl("Snack"), text: snack });
    el.addEventListener("pointerdown", (e) => drag(e, el, snack));
    snacks.append(el);
  }

  const dots = h("span", { class: "kt-dots" });
  for (let i = 0; i < MOONWALK_TREATS; i++) dots.append(h("i", {}));
  const dance = h("button", { class: "btn primary kt-dance", onclick: () => actions.moonwalk() }, h("span", { text: tl("Dance") }));
  const skillHint = h("span", { class: "kt-hint" });

  const panel = h(
    "div",
    { class: "kt-panel" },
    h("div", { class: "kt-sec" }, h("span", { text: tl("Full") }), fullText),
    h("div", { class: "kt-bar" }, fullBar),
    mood,
    h("div", { class: "kt-sec" }, h("span", { text: tl("Drag a snack onto Mochi") })),
    snacks,
    h("div", { class: "kt-skill" }, h("span", { class: "kt-name", text: tl("🕺 Moonwalk") }), dots),
    h("div", { class: "kt-row" }, dance, skillHint),
  );
  const el = h("div", { class: "view kitchen" }, panel);

  /** A snack follows the pointer; dropped on Mochi, he eats it. */
  function drag(e: PointerEvent, from: HTMLElement, snack: string) {
    e.preventDefault();
    e.stopPropagation();
    from.setPointerCapture(e.pointerId);
    const ghost = h("div", { class: "kt-ghost", text: snack });
    document.body.append(ghost);
    let over = false;
    const move = (ev: PointerEvent) => {
      ghost.style.left = `${ev.clientX}px`;
      ghost.style.top = `${ev.clientY}px`;
      over = actions.snackOver(ev.clientX, ev.clientY);
      ghost.classList.toggle("over", over);
    };
    const up = () => {
      from.removeEventListener("pointermove", move);
      from.removeEventListener("pointerup", up);
      from.removeEventListener("pointercancel", up);
      ghost.remove();
      actions.snackOver(-1e6, -1e6);
      if (over) actions.eatSnack();
    };
    from.addEventListener("pointermove", move);
    from.addEventListener("pointerup", up);
    from.addEventListener("pointercancel", up);
    move(e);
  }

  let timer: number | null = null;
  function syncTimer() {
    const on = State.mode === "expanded" && State.view === "kitchen";
    if (on && timer == null) timer = window.setInterval(() => State.notify(), REFRESH_MS);
    else if (!on && timer != null) {
      window.clearInterval(timer);
      timer = null;
    }
  }
  State.subscribe(syncTimer);

  return {
    el,
    sync() {
      syncTimer();
      const pet = readPet(State.settings.pet);
      const now = Date.now();
      const full = Math.round(fullNow(pet, now));
      fullBar.style.width = `${full}%`;
      fullText.textContent = `${full} %`;
      mood.textContent = hungry(pet, now) ? t("He's hungry!") : full >= 95 ? t("Full up. Thank you!") : "";
      const eaten = Math.min(MOONWALK_TREATS, pet.treats);
      Array.from(dots.children).forEach((d, i) => d.classList.toggle("on", i < eaten));
      const learned = canMoonwalk(pet);
      (dance as HTMLButtonElement).disabled = !learned || actions.isMoonwalking();
      const left = treatsLeft(pet);
      skillHint.textContent = learned
        ? pet.music ? t("With Michael Jackson on Spotify") : t("He knows it!")
        : t("Snacks before he learns it: {count}", { count: left });
    },
  };
}
