// Who sees the beta chat's button (the owner and the signed-in testers), its
// unread badge, and the island opening on a new message when nobody is busy
// with it (src-tauri/src/betachat.rs tells us).

import { Bridge, IS_TAURI, onEvent, type AccountState, type ChatNews } from "../core/bridge";
import type { IslandViewName } from "../core/layout";
import { State } from "../core/state";
import { agentBusy } from "../core/glow";
import { Spotify } from "../core/spotify";
import { playChatSound } from "../core/chatsound";

export const BetaChat = { allowed: false, unread: 0 };

export interface BetaChatHost {
  alert(view: IslandViewName): void;
}

const QUIET_VIEWS: ReadonlySet<IslandViewName> = new Set(["overview", "empty"]);

function quiet(): boolean {
  if (State.paused || State.pendingApproval) return false;
  return State.mode !== "expanded" || QUIET_VIEWS.has(State.view);
}

function apply(s: AccountState | null) {
  const allowed = !!s?.signedIn && (s.owner || s.tester);
  if (allowed === BetaChat.allowed) return;
  BetaChat.allowed = allowed;
  if (!allowed) BetaChat.unread = 0;
  State.notify();
  if (allowed) {
    void Bridge.betaChatUnread()
      .then((n) => {
        BetaChat.unread = n.unread;
        State.notify();
      })
      .catch(() => undefined);
  }
}

/** The sound picked for a new message, unless "quiet" and the user is busy. */
async function ring() {
  const p = await Bridge.betaChatPrefs();
  if (!p || State.paused) return;
  const busy = agentBusy(State.tasks.map((t) => t.state)) || !!State.pendingApproval || Spotify.state.playing;
  if (p.quiet && busy) return;
  await playChatSound(p.sound, p.volume, p.ownName);
}

export function registerBetaChat(host: BetaChatHost) {
  if (!IS_TAURI) return;
  void Bridge.accountState().then(apply);
  void onEvent<AccountState>("account-changed", apply);
  void onEvent<ChatNews>("beta-chat", (n) => {
    if (!BetaChat.allowed) return;
    BetaChat.unread = n.unread;
    void ring();
    if (State.mode === "expanded" && State.view === "betachat") return;
    if (n.unread > 0 && quiet()) host.alert("betachat");
    else State.notify();
  });
}
