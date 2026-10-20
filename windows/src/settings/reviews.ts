// Settings → Reviews (src-tauri/src/reviews.rs): what the testers said about
// each build — the average, how many of each star, and every review — for the
// beta testers and the owner; and, for the owner, when and whom Coucou asks.

import { Bridge, onEvent, type AccountState, type BuildReviews } from "../core/bridge";
import { h, clear } from "../views/dom";
import { t, tn } from "../i18n/i18n";
import { avatar, errorText } from "./account";
import { reviewErrorText } from "../views/review";

const DAYS = [3, 7, 14];

function seg<T extends string | number>(choices: { value: T; label: string }[], current: T, pick: (v: T) => void): HTMLElement {
  const el = h("div", { class: "rvs-seg" });
  for (const c of choices) {
    const b = h("button", { class: c.value === current ? "on" : "", text: c.label });
    b.addEventListener("click", () => {
      for (const x of Array.from(el.children)) x.classList.remove("on");
      b.classList.add("on");
      pick(c.value);
    });
    el.append(b);
  }
  return el;
}

export function reviewsSections(): HTMLElement[] {
  const survey = h("section", {});
  const reviews = h("section", {});
  let state: AccountState | null = null;
  let data: BuildReviews[] | null = null;
  let picked = 0;
  let loading = false;

  async function load() {
    state = await Bridge.accountState();
    drawSurvey();
    if (state?.signedIn && (state.owner || state.tester)) void fetchReviews();
    else drawReviews();
  }
  void onEvent<AccountState>("account-changed", (s) => {
    const before = state?.signedIn && (state.owner || state.tester);
    state = s;
    drawSurvey();
    if (!before && s.signedIn && (s.owner || s.tester)) void fetchReviews();
    else drawReviews();
  });

  // ── The survey (owner) ──────────────────────────────────────────────────────

  function drawSurvey() {
    clear(survey);
    survey.style.display = state?.signedIn && state.owner ? "" : "none";
    if (!state?.signedIn || !state.owner) return;
    const s = state.survey;
    const note = h("div", { class: "hint" });
    const store = (days: number, who: string) => {
      note.textContent = t("Saving…");
      Bridge.accountSetSurvey(days, who)
        .then((next) => {
          state = next;
          note.textContent = t("Saved.");
        })
        .catch((e: unknown) => {
          note.textContent = errorText(e);
        });
    };
    survey.append(
      h("h2", {}, h("span", { text: t("Survey") }), h("span", { class: "tag", text: t("Only you see this") })),
      h("div", { class: "row" },
        h("label", { text: t("Ask after") }),
        seg(DAYS.map((d) => ({ value: d, label: tn("{count} day", "{count} days", d) })), s.days, (d) => store(d, state!.survey.who)),
      ),
      h("div", { class: "row" },
        h("label", { text: t("Who is asked") }),
        seg([
          { value: "testers", label: t("Beta testers") },
          { value: "everyone", label: t("Everyone signed in") },
        ], s.who, (who) => store(state!.survey.days, who)),
      ),
      h("div", { class: "hint", text: t("Each build is asked about once, that long after it first starts. The reviews are comments on the build's issue in your repository.") }),
      note,
    );
  }

  // ── The reviews ─────────────────────────────────────────────────────────────

  async function fetchReviews() {
    if (loading) return;
    loading = true;
    drawReviews();
    try {
      data = await Bridge.reviewList();
      if (!data.some((b) => b.build === picked)) picked = data.find((b) => b.reviews.length)?.build ?? data[0]?.build ?? 0;
      loading = false;
      drawReviews();
    } catch (e) {
      loading = false;
      drawReviews(reviewErrorText(e));
    }
  }

  function drawReviews(error?: string) {
    clear(reviews);
    const head = h("h2", {}, h("span", { text: t("Reviews of the builds") }));
    reviews.append(head);
    if (!state?.signedIn) {
      reviews.append(h("div", { class: "hint", text: t("Sign in with GitHub in Account to see the reviews.") }));
      return;
    }
    if (!state.owner && !state.tester) {
      reviews.append(h("div", { class: "hint", text: t("Only the beta testers and Coucou's owner see the reviews.") }));
      return;
    }
    const refresh = h("button", { text: t("Refresh") });
    refresh.addEventListener("click", () => void fetchReviews());
    head.append(h("span", { class: "spacer" }), refresh);
    if (error) {
      reviews.append(h("div", { class: "notice err", text: error }));
      return;
    }
    if (loading || !data) {
      reviews.append(h("div", { class: "hint", text: t("Loading…") }));
      return;
    }
    if (!data.length) {
      reviews.append(h("div", { class: "hint", text: t("No reviews yet.") }));
      return;
    }
    reviews.append(
      h("div", { class: "row" },
        h("label", { text: t("Build") }),
        seg(data.map((b) => ({ value: b.build, label: `${b.build} · ${b.reviews.length}` })), picked, (b) => {
          picked = b;
          drawReviews();
        }),
      ),
    );
    const build = data.find((b) => b.build === picked) ?? data[0];
    const list = build.reviews;
    if (!list.length) {
      reviews.append(h("div", { class: "hint", text: t("No reviews of build {build} yet.", { build: build.build }) }));
    } else {
      const avg = list.reduce((a, r) => a + r.stars, 0) / list.length;
      const bars = h("div", { class: "rvs-bars" });
      for (let s = 5; s >= 1; s--) {
        const n = list.filter((r) => r.stars === s).length;
        const fill = h("b", {});
        fill.style.width = `${(n / list.length) * 100}%`;
        bars.append(h("div", { class: "rvs-bar" }, h("span", { text: `${s}★` }), h("i", {}, fill), h("span", { text: String(n) })));
      }
      reviews.append(
        h("div", { class: "rvs-sum" },
          h("div", { class: "rvs-avg" },
            h("b", { text: avg.toFixed(1) }),
            h("span", { class: "hint", text: `★ · ${tn("{count} review", "{count} reviews", list.length)}` }),
          ),
          bars,
        ),
      );
      for (const r of list) {
        const starsText = "★".repeat(r.stars) + "☆".repeat(5 - r.stars);
        const item = h("div", { class: "rvs-item" },
          h("div", { class: "row" },
            avatar(r.login, r.login),
            h("div", { class: "acc-grow" },
              h("b", { text: `@${r.login}` }),
              h("div", { class: "hint", text: r.at ? new Date(r.at * 1000).toLocaleDateString() : "" }),
            ),
            h("span", { class: "rvs-stars", text: starsText }),
          ),
        );
        if (r.add) item.append(h("div", { class: "rvs-line" }, h("span", { class: "rvs-add", text: t("➕ Add:") }), document.createTextNode(` ${r.add}`)));
        if (r.remove) item.append(h("div", { class: "rvs-line" }, h("span", { class: "rvs-rm", text: t("➖ Take away:") }), document.createTextNode(` ${r.remove}`)));
        reviews.append(item);
      }
    }
    if (build.page) {
      const open = h("button", { text: t("Open on GitHub") });
      open.addEventListener("click", () => void Bridge.openUrl(build.page));
      reviews.append(h("div", { class: "row" }, open));
    }
  }

  survey.style.display = "none";
  reviews.append(h("h2", {}, h("span", { text: t("Reviews of the builds") })));
  void load();
  return [reviews, survey];
}
