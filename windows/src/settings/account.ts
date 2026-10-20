// Settings → Account (src-tauri/src/account.rs): sign in with GitHub, the
// saves kept in the user's own secret Gist, and — for the owner only — the
// beta testers list, searched and edited here.

import { Bridge, onEvent, type AccountError, type AccountSaved, type AccountState, type DeviceCode, type Person } from "../core/bridge";
import type { Settings } from "../core/state";
import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";

type Toggle = (on: boolean, onChange: (v: boolean) => void) => HTMLElement;

/** The saved parts, as account.rs names them. */
const PARTS: { id: string; label: () => string }[] = [
  { id: "general", label: () => t("Settings and look") },
  { id: "mochi", label: () => t("Mochi: how full he is, his dances") },
  { id: "outfits", label: () => t("Mochi's outfit") },
  { id: "glowSound", label: () => t("Glow and sounds") },
];

const AVATAR_COLORS = ["#f9a8d4", "#93c5fd", "#86efac", "#fcd34d", "#c4b5fd", "#fdba74", "#5eead4"];

/** A round letter in a colour of the login's own (no picture is fetched). */
export function avatar(login: string, name: string, big = false): HTMLElement {
  let sum = 0;
  for (const c of login.toLowerCase()) sum += c.charCodeAt(0);
  const letter = (name.trim() || login).charAt(0).toUpperCase();
  const el = h("span", { class: big ? "acc-av big" : "acc-av", text: letter });
  el.style.background = AVATAR_COLORS[sum % AVATAR_COLORS.length];
  return el;
}

export function errorText(e: unknown): string {
  const code = (typeof e === "string" ? e : "network") as AccountError;
  return ({
    notConfigured: t("Signing in with GitHub is not set up in this build."),
    denied: t("Sign-in was cancelled."),
    expired: t("The code ran out. Try again."),
    network: t("GitHub could not be reached. Try again later."),
    notSignedIn: t("Sign in with GitHub first."),
    signedOut: t("GitHub no longer accepts this sign-in. Sign in again."),
    notOwner: t("Only Coucou's owner can do this."),
    noReleaseToken: t("Connect GitHub in Updates first (Release to everyone…)."),
    needsContents: t("The GitHub token can't edit the list. Create it again with Contents: Read and write."),
    noSaves: t("No saves on GitHub yet."),
    badLogin: t("That is not a GitHub login."),
  } as Record<AccountError, string>)[code] ?? t("GitHub could not be reached. Try again later.");
}

/** "Saved 5 minutes ago"-style, but plain: the date and time. */
function when(ms: number): string {
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** `restore`: the page saves the settings that came back, so the island follows as for any change. */
export function accountSections(makeToggle: Toggle, restore: (s: Settings) => Promise<void>): HTMLElement[] {
  const account = h("section", {});
  const saves = h("section", {});
  const testers = h("section", {});
  let state: AccountState | null = null;

  async function load() {
    state = await Bridge.accountState();
    draw();
  }
  void onEvent<AccountState>("account-changed", (s) => {
    state = s;
    draw();
  });

  function draw() {
    drawAccount();
    drawSaves();
    drawTesters();
  }

  // ── Account ─────────────────────────────────────────────────────────────────

  function drawAccount() {
    clear(account);
    account.append(h("h2", {}, h("span", { text: t("Account") })));
    if (!state) return;
    if (!state.configured) {
      account.append(h("div", { class: "hint", text: t("Signing in with GitHub is not set up in this build.") }));
      return;
    }
    if (!state.signedIn) {
      const go = h("button", { class: "primary", text: t("Sign in with GitHub") });
      const note = h("div", { class: "hint" });
      go.addEventListener("click", () => void signIn(go, note));
      account.append(
        h("div", { class: "row" },
          h("div", { class: "acc-grow" },
            h("b", { text: t("Sign in with GitHub") }),
            h("div", { class: "hint", text: t("So your saves follow you to any computer, and so you get beta builds if you were invited.") }),
          ),
          go,
        ),
        note,
      );
      return;
    }
    const role = state.owner ? t("Coucou's owner") : state.tester ? t("Beta tester") : "";
    const out = h("button", { text: t("Sign out") });
    out.addEventListener("click", () => void Bridge.accountSignOut().then((s) => {
      state = s;
      draw();
    }));
    account.append(
      h("div", { class: "row" },
        avatar(state.login, state.name, true),
        h("div", { class: "acc-grow" },
          h("b", { text: state.name || state.login }),
          h("div", { class: "hint", text: role ? `@${state.login} · ${role}` : `@${state.login}` }),
        ),
        out,
      ),
      h("div", {
        class: "hint",
        text: t("Coucou only knows your GitHub name and keeps your saves in a secret Gist of yours. It never sees your GitHub password."),
      }),
    );
  }

  async function signIn(go: HTMLElement, note: HTMLElement) {
    go.setAttribute("disabled", "");
    note.textContent = "";
    let code: DeviceCode;
    try {
      code = await Bridge.accountBegin();
    } catch (e) {
      go.removeAttribute("disabled");
      note.textContent = errorText(e);
      return;
    }
    clear(account);
    const copy = h("button", { text: t("Copy") });
    copy.addEventListener("click", () => void navigator.clipboard?.writeText(code.userCode).then(() => {
      copy.textContent = t("Copied");
    }));
    const open = h("button", { text: t("Open on GitHub") });
    open.addEventListener("click", () => void Bridge.openUrl(code.verificationUri));
    const cancel = h("button", { text: t("Cancel") });
    cancel.addEventListener("click", () => void Bridge.accountCancel());
    account.append(
      h("h2", {}, h("span", { text: t("Account") })),
      h("div", { class: "hint", text: t("GitHub's page is open in your browser. Type this code there and approve Coucou:") }),
      h("div", { class: "row" }, h("span", { class: "acc-code", text: code.userCode }), copy, open),
      h("div", { class: "row" }, h("span", { class: "hint", text: t("Waiting for GitHub…") }), cancel),
    );
    try {
      state = await Bridge.accountFinish();
      draw();
      void offerRestore();
    } catch (e) {
      draw();
      const why = h("div", { class: "notice err", text: errorText(e) });
      account.append(why);
    }
  }

  // ── Saves ───────────────────────────────────────────────────────────────────

  const restoreBox = h("div", { class: "acc-stack" });

  async function offerRestore(asked = false) {
    clear(restoreBox);
    let saved: AccountSaved | null;
    try {
      saved = await Bridge.accountSaved();
    } catch (e) {
      if (asked) restoreBox.append(h("div", { class: "notice err", text: errorText(e) }));
      return;
    }
    if (!saved) {
      if (asked) restoreBox.append(h("div", { class: "hint", text: t("No saves on GitHub yet.") }));
      return;
    }
    const picks = new Set(saved.parts);
    const boxes = PARTS.filter((p) => saved.parts.includes(p.id)).map((p) => {
      const box = h("input", { type: "checkbox" }) as HTMLInputElement;
      box.checked = true;
      box.addEventListener("change", () => (box.checked ? picks.add(p.id) : picks.delete(p.id)));
      return h("label", { class: "acc-check" }, box, h("span", { text: p.label() }));
    });
    const go = h("button", { class: "primary", text: t("Restore") });
    const no = h("button", { text: t("Not now") });
    no.addEventListener("click", () => clear(restoreBox));
    go.addEventListener("click", () => {
      go.setAttribute("disabled", "");
      Bridge.accountRestore([...picks])
        .then(async (s) => {
          await restore(s);
          clear(restoreBox);
          restoreBox.append(h("div", { class: "notice ok", text: t("Your saves are back.") }));
        })
        .catch((e: unknown) => {
          go.removeAttribute("disabled");
          restoreBox.append(h("div", { class: "notice err", text: errorText(e) }));
        });
    });
    restoreBox.append(
      h("b", { text: t("Saves from {when} are on GitHub. Bring them here?", { when: when(saved.savedAt) }) }),
      h("div", { class: "acc-checks" }, ...boxes),
      h("div", { class: "hint", text: t("What you pick replaces the same settings on this computer.") }),
      h("div", { class: "row" }, go, no),
    );
  }

  function drawSaves() {
    clear(saves);
    saves.style.display = state?.signedIn ? "" : "none";
    if (!state?.signedIn) return;
    const s = state;
    const store = (sync: boolean, parts: string[]) =>
      void Bridge.accountSetSync(sync, parts).then((next) => {
        state = next;
        drawSaves();
      });
    const checks = PARTS.map((p) => {
      const box = h("input", { type: "checkbox" }) as HTMLInputElement;
      box.checked = s.parts.includes(p.id);
      box.disabled = !s.sync;
      box.addEventListener("change", () => {
        const parts = PARTS.map((q) => q.id).filter((id) => (id === p.id ? box.checked : s.parts.includes(id)));
        store(s.sync, parts);
      });
      return h("label", { class: "acc-check" }, box, h("span", { text: p.label() }));
    });
    const status = h("span", {
      class: "hint",
      text: s.lastSync > 0 ? t("Saved {when}", { when: when(s.lastSync) }) : t("Not saved yet"),
    });
    const now = h("button", { text: t("Save now") });
    now.addEventListener("click", () => {
      now.setAttribute("disabled", "");
      status.textContent = t("Saving…");
      Bridge.accountSyncNow()
        .then((next) => {
          state = next;
          drawSaves();
        })
        .catch((e: unknown) => {
          now.removeAttribute("disabled");
          status.textContent = errorText(e);
        });
    });
    const back = h("button", { text: t("Restore…") });
    back.addEventListener("click", () => void offerRestore(true));
    saves.append(
      h("h2", {}, h("span", { text: t("Saves") })),
      h("div", { class: "row" },
        h("label", { text: t("Sync") }),
        makeToggle(s.sync, (on) => store(on, s.parts)),
        h("span", { class: "hint", text: t("Keep these in your GitHub and bring them to your other computers") }),
      ),
      h("div", { class: "acc-checks" }, ...checks),
      h("div", { class: "row" }, status, h("span", { class: "spacer" }), now, back),
      restoreBox,
      h("div", {
        class: "hint",
        text: t("API keys and tokens never leave this computer's credential store, and nor do folders, local servers or the phone link."),
      }),
    );
  }

  // ── Beta testers (owner) ────────────────────────────────────────────────────

  const search = h("input", { type: "text", autocomplete: "off", style: "flex:1 1 auto;min-width:0" }) as HTMLInputElement;
  search.placeholder = t("Find someone on GitHub by name or login");
  const results = h("div", { class: "acc-list" });
  const list = h("div", { class: "acc-list" });
  const testersNote = h("div", { class: "hint" });
  let current_: Person[] = [];
  let timer: number | null = null;
  let asked = 0;

  function person(p: Person, action: HTMLElement): HTMLElement {
    return h("div", { class: "acc-person" },
      avatar(p.login, p.name),
      h("div", { class: "acc-grow" },
        h("b", { text: p.name || p.login }),
        h("div", { class: "hint", text: `@${p.login}` }),
      ),
      action,
    );
  }

  function drawList() {
    clear(list);
    if (!current_.length) {
      list.append(h("div", { class: "hint", text: t("Nobody yet. Find someone above and click Add.") }));
      return;
    }
    for (const p of current_) {
      const out = h("button", { class: "danger", text: t("Remove") });
      out.addEventListener("click", () => {
        out.setAttribute("disabled", "");
        Bridge.accountTesterRemove(p.login)
          .then((next) => {
            current_ = next;
            testersNote.textContent = t("{name} is off the beta.", { name: p.name || p.login });
            drawList();
            void find();
          })
          .catch((e: unknown) => {
            out.removeAttribute("disabled");
            testersNote.textContent = errorText(e);
          });
      });
      list.append(person(p, out));
    }
  }

  async function find() {
    const q = search.value.trim();
    const mine = ++asked;
    clear(results);
    if (!q) return;
    let found: Person[];
    try {
      found = await Bridge.accountSearch(q);
    } catch (e) {
      if (mine === asked) results.append(h("div", { class: "hint", text: errorText(e) }));
      return;
    }
    if (mine !== asked) return;
    if (!found.length) {
      results.append(h("div", { class: "hint", text: t("Nobody by that name on GitHub.") }));
      return;
    }
    for (const p of found) {
      const already = current_.some((c) => c.login.toLowerCase() === p.login.toLowerCase());
      let action: HTMLElement;
      if (already) action = h("span", { class: "tag", text: t("In the beta") });
      else {
        const add = h("button", { class: "primary", text: t("Add") });
        add.addEventListener("click", () => {
          add.setAttribute("disabled", "");
          Bridge.accountTesterAdd(p.login, p.name)
            .then((next) => {
              current_ = next;
              testersNote.textContent = t("{name} is in the beta. They get it the next time Coucou starts.", { name: p.name || p.login });
              drawList();
              void find();
            })
            .catch((e: unknown) => {
              add.removeAttribute("disabled");
              testersNote.textContent = errorText(e);
            });
        });
        action = add;
      }
      results.append(person(p, action));
    }
  }
  search.addEventListener("input", () => {
    if (timer != null) window.clearTimeout(timer);
    timer = window.setTimeout(() => void find(), 400);
  });

  let testersLoaded = false;
  function drawTesters() {
    const show = !!state?.signedIn && state.owner;
    testers.style.display = show ? "" : "none";
    if (!show) testersLoaded = false;
    if (!show || testersLoaded) return;
    testersLoaded = true;
    clear(testers);
    testers.append(
      h("h2", {}, h("span", { text: t("Beta testers") }), h("span", { class: "tag", text: t("Only you see this") })),
      h("div", { class: "row" }, search),
      results,
      h("div", { class: "subhead", text: t("In the beta") }),
      list,
      testersNote,
      h("div", {
        class: "hint",
        text: t("The list is the testers.json file of your repository. People on it see Beta in Updates once signed in; people taken off go back to stable."),
      }),
    );
    list.append(h("div", { class: "hint", text: t("Loading…") }));
    Bridge.accountTesters()
      .then((l) => {
        current_ = l;
        drawList();
      })
      .catch((e: unknown) => {
        clear(list);
        testersNote.textContent = errorText(e);
      });
  }

  account.append(h("h2", {}, h("span", { text: t("Account") })));
  saves.style.display = "none";
  testers.style.display = "none";
  void load();
  return [account, saves, testers];
}
