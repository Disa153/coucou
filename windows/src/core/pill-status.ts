// What a pill on the right of the overview says while its agent is busy: the
// step it is on ("Bash · npm test", "main.ts +3 −1") instead of its name, as in
// the notch-companion concepts. Once the agent stops, the name comes back.
// No DOM, so it can be tested on its own (tests/pill-status.test.mjs).

import type { AgentTask } from "./state";
import type { BotStateName } from "./layout";
import { parseDiffStep } from "./diff";

/** States in which an agent is doing something worth showing. */
const LIVE_STATES: ReadonlySet<BotStateName> = new Set(["working", "thinking", "searching"]);

/** Longest status kept; the pill cuts it with an ellipsis well before this anyway. */
const MAX_LENGTH = 80;

/** The step to show in the pill, or null when the pill should show its name. */
export function pillStatus(task: Pick<AgentTask, "state" | "steps">): string | null {
  if (!LIVE_STATES.has(task.state)) return null;
  const step = task.steps[task.steps.length - 1];
  if (!step) return null;
  const diff = parseDiffStep(step);
  const text = diff ? `${diff.filename} +${diff.added} −${diff.removed}` : step;
  const line = text.replace(/\s+/g, " ").trim();
  if (!line) return null;
  return line.length > MAX_LENGTH ? `${line.slice(0, MAX_LENGTH - 1)}…` : line;
}
