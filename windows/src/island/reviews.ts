// When Mochi asks for a review of the build (views/review.ts): at the end of
// the launch greeting, when the island wakes, and once an hour — only if
// reviews.rs says the build is due and nobody is busy with the island.

import { Bridge, IS_TAURI } from "../core/bridge";
import type { IslandViewName } from "../core/layout";
import { State } from "../core/state";
import { ReviewAsk } from "../views/review";

export interface ReviewHost {
  alert(view: IslandViewName): void;
}

/** Views the card may replace; anything else is the user busy with something. */
const QUIET_VIEWS: ReadonlySet<IslandViewName> = new Set(["overview", "empty"]);
const EVERY_MS = 60 * 60 * 1000;

function quiet(): boolean {
  if (State.paused || State.pendingApproval) return false;
  return State.mode !== "expanded" || QUIET_VIEWS.has(State.view);
}

let checking = false;

export async function checkReview(host: ReviewHost) {
  if (!IS_TAURI || checking || !quiet()) return;
  checking = true;
  try {
    const due = await Bridge.reviewDue();
    if (!due || !quiet()) return;
    ReviewAsk.build = due.build;
    host.alert("review");
  } finally {
    checking = false;
  }
}

export function registerReviewChecks(host: ReviewHost) {
  window.setInterval(() => void checkReview(host), EVERY_MS);
}
