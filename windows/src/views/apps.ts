// The island's app panel (src-tauri/src/apps): the programs the user pinned and
// the ones with a window open, like a dock in the island. A click brings the
// program's window forward or starts it; dragging a pinned one moves it; the
// last tile opens "appsEdit", where programs are pinned from the Start menu.
// Windows only: elsewhere the panel is never offered.

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import { State } from "../core/state";
import { Bridge, IS_TAURI, type AppItem, type AppsPanel } from "../core/bridge";
import { tl } from "../i18n/i18n";
import type { ViewActions, ViewHost } from "./views";

/** While the panel is on screen, how often the running programs are read again. */
const POLL_MS = 2000;
/** A press that moves farther than this is a drag, not a click. */
const DRAG_PX = 6;
/** A search is sent this long after the last key. */
const SEARCH_MS = 150;

/** What Rust last said, shared by both views and the island (State.appsWhenIdle). */
export const AppsStore = {
  panel: null as AppsPanel | null,
  async refresh(): Promise<AppsPanel | null> {
    if (!IS_TAURI) return null;
    const p = await Bridge.appsPanel();
    if (p) {
      AppsStore.panel = p;
      State.appsWhenIdle = p.supported && p.whenIdle;
    }
    return p;
  },
};

/** Icons by path: a data: URL, null when Windows has none, a promise while asked. */
const icons = new Map<string, string | null | Promise<string | null>>();

/** The same colour for the same name, for a program without an icon. */
function tint(name: string): string {
  let n = 0;
  for (const c of name) n = (n * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${n % 360} 55% 42%)`;
}

/** A program's picture: its icon once Windows gives it, its first letter until then. */
function appIcon(item: AppItem, size: number): HTMLElement {
  const box = h("span", { class: "ap-icon" });
  box.style.width = box.style.height = `${size}px`;
  const letter = () => {
    clear(box);
    box.classList.add("letter");
    box.style.background = tint(item.name);
    box.textContent = (item.name.trim()[0] ?? "?").toUpperCase();
  };
  const show = (url: string | null) => {
    if (!url) return letter();
    clear(box);
    box.classList.remove("letter");
    box.style.background = "";
    box.append(h("img", { src: url, alt: "", draggable: "false" }));
  };
  const known = icons.get(item.path);
  if (typeof known === "string" || known === null) {
    show(known);
  } else {
    letter();
    const asked = known ?? Bridge.appsIcon(item.path);
    icons.set(item.path, asked);
    void asked.then((url) => {
      icons.set(item.path, url ?? null);
      show(url ?? null);
    });
  }
  return box;
}

// ── The dock ──────────────────────────────────────────────────────────────────

export function buildApps(actions: ViewActions): ViewHost {
  // Read once at start, so the island knows what to open on (State.appsWhenIdle).
  void AppsStore.refresh().then(() => State.notify());
  const row = h("div", { class: "ap-row" });
  const empty = h("div", { class: "ap-empty", text: tl("Pin the programs you use: they open from here.") });
  const el = h("div", { class: "view apps" }, h("div", { class: "card ap-card" }, row, empty));
  let timer: number | null = null;
  let drawn = "";
  /** Set by a drag, so the click that ends it opens nothing. */
  let dragged = false;

  row.addEventListener(
    "wheel",
    (e) => {
      if (row.scrollWidth <= row.clientWidth) return;
      e.preventDefault();
      row.scrollLeft += e.deltaY + e.deltaX;
    },
    { passive: false },
  );

  function open(item: AppItem) {
    if (dragged) return;
    actions.blip();
    void Bridge.appsOpen(item.path, item.exe).then(() => actions.collapse());
  }

  function tile(item: AppItem, index: number): HTMLElement {
    const btn = h(
      "button",
      { class: "ap-app", title: item.name, "aria-label": item.name },
      appIcon(item, 40),
      h("span", { class: "ap-name", text: item.name }),
      item.running ? h("span", { class: "ap-dot" }) : null,
    );
    btn.addEventListener("click", () => open(item));
    btn.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      if (item.pinned) {
        actions.setView("appsEdit");
      } else {
        // A running program that is not pinned: a right-click keeps it.
        void Bridge.appsPin(item.name, item.path, item.exe).then(() => paintSoon());
      }
    });
    if (item.pinned) dragToMove(btn, item, index);
    return btn;
  }

  /** Dragging a pinned tile sideways: the others make room, the drop saves the order. */
  function dragToMove(btn: HTMLElement, item: AppItem, index: number) {
    btn.addEventListener("pointerdown", (down) => {
      if (down.button !== 0) return;
      dragged = false;
      const startX = down.clientX;
      const tiles = [...row.querySelectorAll<HTMLElement>(".ap-app.pinned")];
      const centres = tiles.map((t) => t.getBoundingClientRect().left + t.offsetWidth / 2);
      let target = index;
      const move = (e: PointerEvent) => {
        const dx = e.clientX - startX;
        if (!dragged && Math.abs(dx) < DRAG_PX) return;
        if (!dragged) {
          dragged = true;
          btn.setPointerCapture(down.pointerId);
          btn.classList.add("dragging");
        }
        btn.style.transform = `translateX(${dx}px)`;
        const x = centres[index] + dx;
        target = centres.filter((c, i) => i !== index && c < x).length;
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        btn.classList.remove("dragging");
        btn.style.transform = "";
        if (dragged && target !== index) {
          void Bridge.appsMove(item.path, target).then(() => paintSoon());
        }
        // The click event comes right after the pointerup: let it see `dragged`.
        window.setTimeout(() => (dragged = false), 0);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    });
  }

  function paint(p: AppsPanel) {
    const key = JSON.stringify([p.pinned, p.others]);
    if (key === drawn) return;
    drawn = key;
    clear(row);
    p.pinned.forEach((item, i) => row.append(tile(item, i)));
    row.querySelectorAll(".ap-app").forEach((b) => b.classList.add("pinned"));
    if (p.others.length) {
      row.append(h("span", { class: "ap-sep" }));
      p.others.forEach((item, i) => row.append(tile(item, i)));
    }
    const add = h(
      "button",
      { class: "ap-app ap-add", title: tl("Add a program"), "aria-label": tl("Add a program") },
      h("span", { class: "ap-icon add" }, svg(ICONS.plus, 18)),
      h("span", { class: "ap-name", text: tl("Add") }),
    );
    add.addEventListener("click", () => {
      actions.blip();
      actions.setView("appsEdit");
    });
    row.append(add);
    empty.style.display = p.pinned.length || p.others.length ? "none" : "";
  }

  function paintSoon() {
    void AppsStore.refresh().then((p) => p && paint(p));
  }

  return {
    el,
    sync() {
      const on = State.mode === "expanded" && State.view === "apps";
      if (on && timer === null) {
        if (AppsStore.panel) paint(AppsStore.panel);
        paintSoon();
        timer = window.setInterval(paintSoon, POLL_MS);
      } else if (!on && timer !== null) {
        window.clearInterval(timer);
        timer = null;
      }
    },
  };
}

// ── Pinning from the Start menu ───────────────────────────────────────────────

export function buildAppsEdit(actions: ViewActions): ViewHost {
  const input = h("input", { class: "ap-search", type: "text", placeholder: tl("Search the Start menu"), spellcheck: "false" }) as HTMLInputElement;
  const idle = h("input", { type: "checkbox" }) as HTMLInputElement;
  const done = h("button", { class: "btn primary ap-done" }, h("span", { text: tl("Done") }));
  const pinnedRow = h("div", { class: "ap-chips" });
  const list = h("div", { class: "ap-list" });
  const el = h(
    "div",
    { class: "view appsEdit" },
    h(
      "div",
      { class: "card ap-edit" },
      h(
        "div",
        { class: "ap-edit-head" },
        h("span", { class: "ap-title", text: tl("App panel") }),
        h("label", { class: "ap-idle" }, idle, h("span", { text: tl("Show it when no agent is working") })),
        done,
      ),
      pinnedRow,
      input,
      list,
    ),
  );
  let shown = false;
  let searchTimer: number | null = null;
  let asked = 0;

  done.addEventListener("click", () => {
    actions.blip();
    actions.setView("apps");
  });
  idle.addEventListener("change", () => {
    void Bridge.appsSetWhenIdle(idle.checked).then(() => AppsStore.refresh());
  });
  input.addEventListener("input", () => {
    if (searchTimer !== null) window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(search, SEARCH_MS);
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      actions.setView("apps");
    }
  });

  function chips(p: AppsPanel) {
    clear(pinnedRow);
    for (const item of p.pinned) {
      const x = h("button", { class: "ap-chip-x", title: tl("Unpin"), "aria-label": tl("Unpin") }, svg(ICONS.xmark, 10));
      x.addEventListener("click", () => {
        void Bridge.appsUnpin(item.path).then(refresh);
      });
      pinnedRow.append(h("span", { class: "ap-chip" }, appIcon(item, 18), h("span", { text: item.name }), x));
    }
    if (!p.pinned.length) pinnedRow.append(h("span", { class: "ap-hint", text: tl("Nothing pinned yet.") }));
  }

  async function search() {
    const ask = ++asked;
    const found = (await Bridge.appsCatalog(input.value)) ?? [];
    if (ask !== asked) return;
    clear(list);
    for (const item of found) {
      const add = h("button", { class: item.pinned ? "ap-pin on" : "ap-pin", title: item.pinned ? tl("Unpin") : tl("Pin") }, svg(item.pinned ? ICONS.check : ICONS.plus, 12));
      add.addEventListener("click", () => {
        const done = item.pinned ? Bridge.appsUnpin(item.path) : Bridge.appsPin(item.name, item.path, item.exe);
        void done.then(refresh);
      });
      list.append(h("div", { class: "ap-line" }, appIcon(item, 22), h("span", { class: "ap-line-name", text: item.name }), add));
    }
    if (!found.length) list.append(h("div", { class: "ap-hint", text: tl("No program by that name in the Start menu.") }));
  }

  async function refresh() {
    const p = await AppsStore.refresh();
    if (!p) return;
    idle.checked = p.whenIdle;
    chips(p);
    void search();
  }

  return {
    el,
    focus() {
      input.focus();
      input.select();
    },
    sync() {
      const on = State.mode === "expanded" && State.view === "appsEdit";
      if (on && !shown) void refresh();
      shown = on;
    },
  };
}
