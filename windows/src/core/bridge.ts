// Thin wrapper over the Tauri commands/events. Every call is a no-op when the
// page is opened in a plain browser, so the island can be iterated on with
// `npm run dev` alone.

import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Settings } from "./state";
import type { RecapHistory, RecapPrefs } from "../recap/summary";
import type { SpotifyState } from "./spotify";

export const IS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!IS_TAURI) return null;
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    console.error(`[coucou] ${cmd} failed`, err);
    return null;
  }
}

export type UpdateChannel = "stable" | "beta";
export type UpdateRefusal = "badSignature" | "badHash" | "network" | "unsupported" | "localBuild";

/** A newer build a channel offers (its signature already checked by Rust). */
export interface UpdateManifest {
  channel: UpdateChannel;
  build: number;
  version: string;
  notes: string;
}

export interface UpdateInfo {
  version: string;
  build: number;
  channel: UpdateChannel;
  betaAvailable: boolean;
  /** The signed-in GitHub account opens the beta: no password asked. */
  betaByAccount: boolean;
  canInstall: boolean;
  found: UpdateManifest | null;
  promotePage: string | null;
  /** A GitHub token is kept: "Release to everyone" runs the workflow from here. */
  releaseConnected: boolean;
  tokenPage: string;
}

export type ReleaseRefusal = "notConnected" | "badToken" | "wrongPassword" | "notBeta" | "network";

/** Settings → Account (src-tauri/src/account.rs). */
export type AccountError =
  | "notConfigured" | "denied" | "expired" | "network" | "notSignedIn" | "signedOut" | "notOwner"
  | "noReleaseToken" | "releaseTokenRefused" | "needsContents" | "noSaves" | "badLogin";

export interface AccountState {
  configured: boolean;
  signedIn: boolean;
  login: string;
  name: string;
  owner: boolean;
  tester: boolean;
  sync: boolean;
  parts: string[];
  lastSync: number;
  /** When and whom a build's review is asked of (the owner sets it). */
  survey: { days: number; who: "testers" | "everyone" };
}

/** Reviews of a build (src-tauri/src/reviews.rs). */
export type ReviewError = "notSignedIn" | "signInAgain" | "notAllowed" | "noStars" | "tooBig" | "network";

export interface Review {
  login: string;
  stars: number;
  add: string;
  remove: string;
  /** Unix seconds. */
  at: number;
}

/** The beta chat (src-tauri/src/betachat.rs). */
export interface ChatFile {
  name: string;
  mime: string;
  size: number;
  /** The raw Gist file, base64 text of the file. */
  url: string;
}

/** A file to send: its bytes as base64. */
export interface OutFile {
  name: string;
  mime: string;
  data: string;
}

export interface BetaChatPrefs {
  sound: "hee" | "chime" | "quack" | "knock" | "mochi" | "own" | "none";
  volume: number;
  quiet: boolean;
  ownName: string;
}

export interface ChatMessage {
  id: number;
  login: string;
  text: string;
  build: number | null;
  files: ChatFile[];
  /** Unix seconds. */
  at: number;
  owner: boolean;
  mine: boolean;
}

export interface ChatNews {
  unread: number;
  last: ChatMessage | null;
}

export interface BuildReviews {
  build: number;
  page: string;
  reviews: Review[];
}

export interface DeviceCode {
  userCode: string;
  verificationUri: string;
}

export interface AccountSaved {
  savedAt: number;
  parts: string[];
}

export interface Person {
  login: string;
  name: string;
}

export interface ReleaseRun {
  status: string;
  conclusion: string | null;
  url: string;
}

export interface BootInfo {
  settings: Settings;
  /** Logical screen rect of the monitor the island lives on. */
  screen: { x: number; y: number; width: number; height: number; scale: number };
  version: string;
  hookPath: string;
  /** False where the OS has no global cursor (Wayland): see Island.followPageCursor. */
  cursorPoll: boolean;
}

/** The phone link (src-tauri/src/phone_link): how it is, never its secrets. */
export interface PhoneLinkStatus {
  enabled: boolean;
  running: boolean;
  port: number;
  host: string;
  name: string;
  clients: number;
  error: string | null;
  /** The relay connection ("Away from home Wi-Fi"). */
  relayState: PhoneRelayState;
}

export type PhoneRelayState = "off" | "connecting" | "waiting" | "linked" | "accessRefused" | "unreachable" | "roomTaken" | "tooMany";

/** The relay switch and address. The access key is write-only: only whether one is stored comes back. */
export interface PhoneRelayStatus {
  enabled: boolean;
  url: string;
  hasAccess: boolean;
  state: PhoneRelayState;
}

/** Session details for the phone: the switch (off by default). */
export interface PhoneDetailsStatus {
  enabled: boolean;
}

/** Chat from the phone: the switch and the models the phone may use ("provider/model"). */
export interface PhoneChatStatus {
  enabled: boolean;
  models: string[];
}

/** What the settings window shows to pair a phone. Only the settings window may ask. */
export interface PhoneLinkPairing {
  link: string;
  qrSvg: string;
  host: string;
  port: number;
  name: string;
  /** The relay's host when this link also carries the relay fields. */
  relay: string | null;
}

/** One session as the phone sees it; Rust stamps `updatedAt`. */
export interface PhoneLinkSession {
  pillId: string;
  agent: string;
  state: string;
  statusText: string;
  stepIndex: number;
  stepCount: number;
  /** Details, for a phone that has the capability (the user's switch). */
  steps?: string[];
  finalLine?: string;
  /** A folder name, never a path. */
  project?: string;
  color?: string;
  /** The files this session changed, for a phone that has `diffs` (Rust cuts them to a name and a count). */
  files?: PhoneLinkFile[];
}

export interface PhoneLinkFile {
  id: number;
  /** A file name; Rust keeps only its last segment. */
  name: string;
  added: number;
  removed: number;
  tooLarge?: boolean;
  isNew?: boolean;
}

/** One file's diff for the phone that asked: lines as [kind, text], kind "+", "-", " " or "@" (a hunk's first line). */
export interface PhoneLinkDiff {
  pillId: string;
  fileId: number;
  name: string;
  added: number;
  removed: number;
  tooLarge: boolean;
  gone: boolean;
  lines: [string, string][];
}

/** One plan window for the phone: used percentage (as the island has it) and when it resets (epoch ms). */
export interface PhoneLinkWindow {
  usedPct: number;
  resetsAt: number;
}

export interface PhoneLinkPlan {
  fiveHour?: PhoneLinkWindow;
  sevenDay?: PhoneLinkWindow;
  resetCredits?: number;
  planType?: string;
  updatedAt: number;
}

export interface PhoneLinkUsage {
  claude: PhoneLinkPlan | null;
  codex: PhoneLinkPlan | null;
}

/** What one service pill says, for a phone whose user allowed it (Rust cuts every text to size). */
export interface PhoneLinkService {
  id: string;
  title: string;
  headline: string;
  reason?: string;
  items: { label: string; detail: string }[];
}

export interface PhoneServicesStatus {
  /** The services allowed for the phone. */
  services: string[];
}

export interface PhoneUsageStatus {
  enabled: boolean;
}

export interface PhoneDiffsStatus {
  enabled: boolean;
}

export interface PhoneLinkApproval {
  requestId: string;
  sessionId: string;
  pillId: string;
  tool: string;
  command: string;
}

/** The question Claude Code is waiting on, for a phone that may answer it (phone_link/hub.rs). */
export interface PhoneLinkQuestion {
  requestId: string;
  sessionId: string;
  pillId: string;
  questions: { question: string; options: { label: string; description: string }[]; multiSelect: boolean }[];
}

export interface PhoneAnswersStatus {
  enabled: boolean;
}

export const Bridge = {
  boot: () => call<BootInfo>("boot"),
  /** The system's languages as the webview sees them, for Rust's own texts (i18n.rs). */
  setSystemLanguages: (languages: string[]) => call<void>("set_system_languages", { languages }),

  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),

  /** Shrink the window down to the invisible wake strip (hidden) or back to full. */
  setCollapsed: (collapsed: boolean) => call<void>("set_collapsed", { collapsed }),

  /**
   * Pushes the island shape in window coordinates. Rust flips click-through from
   * its own cursor poll, so the flag is never a frame behind a click.
   */
  /** Windows: the cursor poll carries the island until the button goes up (false elsewhere). */
  islandDragBegin: () => call<boolean>("island_drag_begin"),
  setIslandRect: (x: number, y: number, width: number, height: number) =>
    call<void>("set_island_rect", { x, y, width, height }),

  /** Give the window keyboard focus (chat field) and take it away again. */
  focusWindow: (focused: boolean) => call<void>("focus_window", { focused }),

  reposition: () => call<void>("reposition"),
  fitZoom: (dpr: number) => call<boolean>("fit_zoom", { dpr }),

  /** Displays the island can be pinned to: `key` is what `settings.screen` stores. */
  listMonitors: () => call<{ key: string; label: string }[]>("list_monitors"),

  openUrl: (url: string) => call<void>("open_url", { url }),

  /** Settings → Account (src-tauri/src/account.rs). The throwing ones reject with an AccountError. */
  accountState: () => call<AccountState>("account_state"),
  accountBegin: () => callOrThrow<DeviceCode>("account_begin"),
  accountFinish: () => callOrThrow<AccountState>("account_finish"),
  accountCancel: () => call<void>("account_cancel"),
  accountSignOut: () => call<AccountState>("account_sign_out"),
  accountSetSync: (sync: boolean, parts: string[]) => call<AccountState>("account_set_sync", { sync, parts }),
  accountSyncNow: () => callOrThrow<AccountState>("account_sync_now"),
  accountSaved: () => callOrThrow<AccountSaved | null>("account_saved"),
  accountRestore: (parts: string[]) => callOrThrow<Settings>("account_restore", { parts }),
  accountTesters: () => callOrThrow<Person[]>("account_testers"),
  accountTesterAdd: (login: string, name: string) => callOrThrow<Person[]>("account_tester_add", { login, name }),
  accountTesterRemove: (login: string) => callOrThrow<Person[]>("account_tester_remove", { login }),
  accountSearch: (query: string) => callOrThrow<Person[]>("account_search", { query }),
  accountSetSurvey: (days: number, who: string) => callOrThrow<AccountState>("account_set_survey", { days, who }),
  /** Reviews (src-tauri/src/reviews.rs). The throwing ones reject with a ReviewError. */
  reviewDue: () => call<{ build: number } | null>("review_due"),
  reviewLater: (build: number) => call<void>("review_later", { build }),
  reviewSkip: (build: number) => call<void>("review_skip", { build }),
  reviewSend: (build: number, stars: number, add: string, remove: string) =>
    callOrThrow<void>("review_send", { build, stars, add, remove }),
  reviewList: () => callOrThrow<BuildReviews[]>("review_list"),
  /** The beta chat (src-tauri/src/betachat.rs); rejects with a ReviewError. */
  betaChatMessages: (full = false) =>
    callOrThrow<{ messages: ChatMessage[]; page: string; owner: boolean; build: number }>("beta_chat_messages", { full }),
  betaChatUnread: () => callOrThrow<ChatNews>("beta_chat_unread"),
  betaChatSend: (text: string, withBuild: boolean, files: OutFile[] = []) =>
    callOrThrow<ChatMessage>("beta_chat_send", { text, withBuild, files }),
  betaChatDelete: (id: number) => callOrThrow<void>("beta_chat_delete", { id }),
  /** An attachment's bytes, base64. */
  betaChatFile: (url: string) => callOrThrow<string>("beta_chat_file", { url }),
  /** Saves an attachment in Downloads; resolves with where it went. */
  betaChatSave: (url: string, name: string) => callOrThrow<string>("beta_chat_save", { url, name }),
  betaChatPrefs: () => call<BetaChatPrefs>("beta_chat_prefs"),
  betaChatSetPrefs: (prefs: BetaChatPrefs) => call<BetaChatPrefs>("beta_chat_set_prefs", { prefs }),
  betaChatSetOwnSound: (name: string, data: string) => callOrThrow<BetaChatPrefs>("beta_chat_set_own_sound", { name, data }),
  betaChatOwnSound: () => call<string | null>("beta_chat_own_sound"),
  betaChatWindow: () => call<void>("beta_chat_window"),
  betaChatPin: (on: boolean) => call<void>("beta_chat_pin", { on }),

  /** In-app updates (src-tauri/src/updater.rs). */
  updateInfo: () => call<UpdateInfo>("update_info"),
  updateCheck: () => call<{ info: UpdateInfo; refusal: UpdateRefusal | null }>("update_check"),
  /** False when the beta password is wrong. */
  updateSetChannel: (channel: UpdateChannel, password?: string) =>
    call<boolean>("update_set_channel", { channel, password: password ?? null }),
  updateInstall: (build: number) => callOrThrow<void>("update_install", { build }),
  /** Rejects with a ReleaseRefusal. */
  updateReleaseConnect: (token: string) => callOrThrow<void>("update_release_connect", { token }),
  updateReleaseDisconnect: () => call<void>("update_release_disconnect"),
  updateRelease: (password: string) => callOrThrow<ReleaseRun>("update_release", { password }),
  updateReleaseStatus: () => callOrThrow<ReleaseRun | null>("update_release_status"),

  /** Which of these sounds have a file of the user's own in the sounds folder. */
  customSounds: (names: readonly string[]) => call<string[]>("custom_sounds", { names }),
  /** The user's file for one sound, as raw bytes. */
  customSound: (name: string) => call<ArrayBuffer>("custom_sound", { name }),
  revealSoundsFolder: () => call<void>("reveal_sounds_folder"),
  /** Settings → Saved chats: opens the folder (empty: the default one). */
  revealChats: (dir: string) => call<void>("chats_reveal", { dir }),
  /** Where chats go when no folder is set. */
  chatsDefaultDir: () => call<string>("chats_default_dir"),
  /** Tells the island to read the sounds folder again ("sounds-changed"). */
  reloadSounds: () => call<void>("reload_sounds"),

  /** "Open terminal" → opens the folder in VS Code when `code` is on PATH. */
  openInVSCode: (path: string | null) => call<boolean>("open_in_vscode", { path }),

  /**
   * "Open terminal": the window the session runs in when Rust found it
   * (Windows; Linux on X11 and KDE Plasma), else the folder in VS Code.
   */
  openSession: (sessionId: string | null, path: string | null) =>
    call<boolean>("open_session", { sessionId, path }),

  /** The Claude desktop app, for the Claude Desktop pill (Windows only). */
  openClaudeDesktop: () => call<boolean>("open_claude_desktop"),

  /** The diff card's ↗: an existing file, in VS Code; never launched by its type. */
  openFileInVSCode: (path: string) => call<boolean>("open_file_in_vscode", { path }),

  quit: () => call<void>("quit_app"),

  openSettingsWindow: () => call<void>("open_settings_window"),

  /** Writes to %LOCALAPPDATA%\Coucou\coucou.log, next to the Rust lines. */
  log: (message: string) => call<void>("log_line", { message }),

  // ── Claude Code hooks ─────────────────────────────────────────────────────
  hooksStatus: () => call<HookStatus>("hooks_status"),
  /** Pill ID → whether that agent's hooks reach Coucou (read-only, Mac #183). */
  agentHooksStatus: () => call<Record<string, boolean>>("agent_hooks_status"),
  /** Diff to show before anything is written. `install: false` previews removal. */
  hooksPreview: (install: boolean) => callOrThrow<HookPreview>("hooks_preview", { install }),
  /**
   * Writes ~/.claude/settings.json — only ever after an explicit click, and only
   * when the file still matches the preview the user looked at.
   */
  hooksApply: (install: boolean, fingerprint: string) =>
    callOrThrow<string>("hooks_apply", { install, fingerprint }),

  // ── Other agents (Gemini CLI, Codex, Cursor…) ─────────────────────────────
  agentHooksList: () => call<AgentHookStatus[]>("agent_hooks_list"),
  /** Diff to show before anything is written. `install: false` previews removal. */
  agentHooksPreview: (agent: string, install: boolean) =>
    callOrThrow<AgentHookPlan>("agent_hooks_preview", { agent, install }),
  /**
   * Writes the agent's config — only ever after an explicit click, and only when
   * it still matches the preview the user looked at. Returns the backups taken.
   */
  agentHooksApply: (agent: string, install: boolean, fingerprint: string) =>
    callOrThrow<string>("agent_hooks_apply", { agent, install, fingerprint }),
  // ── Plan usage: the status line relay, installed apart from the hooks ──────
  /** Diff of the status line change. `install: false` previews taking the relay out. */
  statusLinePreview: (install: boolean) => callOrThrow<HookPreview>("status_line_preview", { install }),
  /** Same rules as hooksApply: an explicit click, and only for the diff that was shown. */
  statusLineApply: (install: boolean, fingerprint: string) =>
    callOrThrow<string>("status_line_apply", { install, fingerprint }),
  /**
   * Asks the Codex CLI (`codex app-server`) for its plan limits, as Codex's
   * /status does. The raw `account/rateLimits/read` result, or null when Codex
   * is missing, not signed in or slow (15 s).
   */
  codexPlanUsage: () => call<unknown>("codex_plan_usage"),

  approvalDecision: (requestId: string, decision: "allow" | "deny") =>
    call<void>("approval_decision", { requestId, decision }),
  /** "The card is up" — until this lands the relay only waits a moment. */
  approvalAck: (requestId: string) => call<void>("approval_ack", { requestId }),
  /** Answers a question Claude Code asked: question text → chosen label. */
  approvalAnswer: (requestId: string, answers: Record<string, string | string[]>) =>
    call<void>("approval_answer", { requestId, answers }),

  /** "Nobody can act on this" — Claude Code asks in the terminal right away. */
  approvalDecline: (requestId: string) => call<void>("approval_decline", { requestId }),

  // ── Phone link (Coucou for Android) ───────────────────────────────────────
  phoneLinkStatus: () => call<PhoneLinkStatus>("phone_link_status"),
  /** Only ever from the switch in Settings. Throws with the reason if it cannot start. */
  phoneLinkSetEnabled: (enabled: boolean) => callOrThrow<PhoneLinkStatus>("phone_link_set_enabled", { enabled }),
  phoneLinkPairing: () => callOrThrow<PhoneLinkPairing>("phone_link_pairing"),
  /** A new code: the phone that had the old one is disconnected. */
  phoneLinkNewPairing: () => callOrThrow<PhoneLinkPairing>("phone_link_new_pairing"),
  phoneDetailsStatus: () => call<PhoneDetailsStatus>("phone_details_status"),
  /** Off by default; only from the switch in Settings. Phones are reconnected so they learn about it. */
  phoneDetailsSetEnabled: (enabled: boolean) => callOrThrow<PhoneDetailsStatus>("phone_details_set_enabled", { enabled }),
  phoneAnswersStatus: () => call<PhoneAnswersStatus>("phone_answers_status"),
  /** Off by default; only from the switch in Settings. Phones are reconnected so they learn about it. */
  phoneAnswersSetEnabled: (enabled: boolean) => callOrThrow<PhoneAnswersStatus>("phone_answers_set_enabled", { enabled }),
  phoneDiffsStatus: () => call<PhoneDiffsStatus>("phone_diffs_status"),
  /** Off by default; only from the switch in Settings. Phones are reconnected so they learn about it. */
  phoneDiffsSetEnabled: (enabled: boolean) => callOrThrow<PhoneDiffsStatus>("phone_diffs_set_enabled", { enabled }),
  /** The answer to a phone's request for a file's diff (the "phone-link-getdiff" event); `conn` says which phone. */
  phoneLinkSendDiff: (conn: number, diff: PhoneLinkDiff) => call<void>("phone_link_send_diff", { conn, diff }),
  phoneServicesStatus: () => call<PhoneServicesStatus>("phone_services_status"),
  /** None by default; only from the ticks in Settings. Phones are reconnected so they learn about it. */
  phoneServicesSet: (services: string[]) => callOrThrow<PhoneServicesStatus>("phone_services_set", { services }),
  phoneLinkPublishServices: (services: PhoneLinkService[]) => call<void>("phone_link_publish_services", { services }),
  phoneRelayStatus: () => call<PhoneRelayStatus>("phone_relay_status"),
  /** Off by default; needs a valid wss:// address and a stored access key. */
  phoneRelaySet: (enabled: boolean, url: string) => callOrThrow<PhoneRelayStatus>("phone_relay_set", { enabled, url }),
  /** Write-only: stored in the OS keystore, never sent back. An empty value clears it. */
  phoneRelaySetAccess: (access: string) => callOrThrow<PhoneRelayStatus>("phone_relay_set_access", { access }),
  phoneUsageStatus: () => call<PhoneUsageStatus>("phone_usage_status"),
  /** Off by default; only from the switch in Settings. Phones are reconnected so they learn about it. */
  phoneUsageSetEnabled: (enabled: boolean) => callOrThrow<PhoneUsageStatus>("phone_usage_set_enabled", { enabled }),
  phoneLinkPublishUsage: (usage: PhoneLinkUsage) => call<void>("phone_link_publish_usage", { usage }),
  phoneChatStatus: () => call<PhoneChatStatus>("phone_chat_status"),
  /** Off by default; only from the switch in Settings. Phones are reconnected so they learn about it. */
  phoneChatSetEnabled: (enabled: boolean) => callOrThrow<PhoneChatStatus>("phone_chat_set_enabled", { enabled }),
  phoneChatSetModels: (models: string[]) => callOrThrow<PhoneChatStatus>("phone_chat_set_models", { models }),
  phoneLinkPublish: (sessions: PhoneLinkSession[], approval: PhoneLinkApproval | null) =>
    call<void>("phone_link_publish", { sessions, approval }),
  phoneLinkPublishQuestion: (question: PhoneLinkQuestion | null) => call<void>("phone_link_publish_question", { question }),
  /** What Mochi wears ("auto" or an outfit), for phones that asked for it. */
  phoneLinkPublishPrefs: (outfit: string) => call<void>("phone_link_publish_prefs", { outfit }),

  // ── Chat, files, secrets ──────────────────────────────────────────────────
  /** One chat turn. The API key and any file bytes never leave Rust. */
  chatSend: (query: string, context: ChatContext | null) =>
    callOrThrow<{ text: string }>("chat_send", { query, context }),
  chatReset: () => call<void>("chat_reset"),
  /**
   * The models a provider offers, for the picker in the chat view. Rust asks
   * the provider only when it has a key (or a server address).
   */
  chatModels: (provider: string) => callOrThrow<ModelInfo[]>("chat_models", { provider }),
  /** Settings → Local models → Connect: does the server answer, and with which models? */
  /** The custom server's key, bound to the address it is entered for. */
  localSetKey: (url: string, key: string) => call<void>("local_set_key", { url, key }),

  localConnect: (provider: "ollama" | "lmstudio" | "custom", url: string) =>
    callOrThrow<LocalServer>("local_connect", { provider, url }),
  /** Copies a dropped file into the inbox. */
  ingestFile: (path: string) => callOrThrow<DroppedFile>("ingest_file", { path }),
  /** Only ever tells you whether a key exists — never its value. */
  secretPresent: (key: string) => call<boolean>("secret_present", { key }),
  secretSet: (key: string, value: string) => callOrThrow<void>("secret_set", { key, value }),
  secretClear: (key: string) => callOrThrow<void>("secret_clear", { key }),

  // ── Integrations ──────────────────────────────────────────────────────────
  refreshIntegration: (id: string) => call<void>("refresh_integration", { id }),
  /** The GitHub card is on screen: refetch that part if it is stale. */
  githubRefresh: (section: "pulse" | "activity") => call<void>("github_refresh", { section }),
  /** Opens the configured n8n instance in the browser. */
  openN8n: () => call<void>("open_n8n"),

  /** Tray → Pause. Stops the integration pollers, not just the island. */
  setPaused: (paused: boolean) => call<void>("set_paused", { paused }),

  // ── Global shortcuts ──────────────────────────────────────────────────────
  /** How each global shortcut went when Rust last registered them. */
  shortcutsStatus: () => call<ShortcutsReport>("shortcuts_status"),
  /** Lets go of every global shortcut while Settings records a new one. */
  shortcutsSuspend: (suspended: boolean) => call<void>("shortcuts_suspend", { suspended }),

  // ── Weekly recap ──────────────────────────────────────────────────────────
  /** Turns and decisions from `since` (Unix seconds) on, plus the recap prefs. */
  recapHistory: (since: number) => call<RecapHistory>("recap_history", { since: Math.floor(since) }),
  recapPrefs: () => call<RecapPrefs>("recap_prefs"),
  recapSetEnabled: (enabled: boolean) => call<void>("recap_set_enabled", { enabled }),
  recapSetHideProjects: (hide: boolean) => call<void>("recap_set_hide_projects", { hide }),
  /** `week` is the Monday (YYYY-MM-DD) the recap opened on its own for. */
  recapMarkShown: (week: string) => call<void>("recap_mark_shown", { week }),
  recapClear: () => call<void>("recap_clear"),
  /** Writes the PNG (a data URL) into Pictures or Downloads; returns its path. */
  recapSavePng: (data: string, week: string) => callOrThrow<string>("recap_save_png", { data, week }),
  /** Opens the folder of the image saved last. */
  recapRevealSaved: () => call<void>("recap_reveal_saved"),

  // ── Mochi on the desktop (src-tauri/src/desktop.rs) ───────────────────────
  desktopInfo: () => call<DesktopInfo>("desktop_mochi_info"),
  /** Dragged out of the island: (x, y) is the pointer in island-window coordinates. */
  desktopPickUp: (x: number, y: number) => call<boolean>("desktop_mochi_pick_up", { x, y }),
  /** Linux: the pointer moved during that drag (Windows carries him from Rust). */
  desktopCarry: (x: number, y: number) => call<void>("desktop_mochi_carry", { x, y }),
  desktopCarryEnd: (x: number, y: number) => call<void>("desktop_mochi_carry_end", { x, y }),
  /** A drag started on the desktop Mochi; resolves to his top-left corner. */
  desktopDragBegin: () => call<[number, number] | null>("desktop_mochi_drag_begin"),
  /** X11: top-left corner, physical pixels. */
  desktopDragMove: (x: number, y: number) => call<void>("desktop_mochi_drag_move", { x, y }),
  desktopDragEnd: (x: number, y: number) => call<void>("desktop_mochi_drag_end", { x, y }),
  /** From the island to his spot. False: no spot on any connected display.
   *  `anywhere`: the first-visit corner then, rather than staying home. */
  desktopFlyOut: (anywhere = false) => call<boolean>("desktop_mochi_fly_out", { anywhere }),
  /** To the island, then hidden. `forget`: he lives in the island again. */
  desktopFlyHome: (forget: boolean) => call<boolean>("desktop_mochi_fly_home", { forget }),
  /** Asleep, the cursor poll stops. */
  desktopSetAsleep: (asleep: boolean) => call<void>("desktop_mochi_set_asleep", { asleep }),

  // ── Spotify (src-tauri/src/spotify.rs) ────────────────────────────────────
  /** Reads the player again (the card came on screen) and reports it. */
  spotifyRefresh: () => call<SpotifyState | null>("spotify_refresh"),
  /** True when Spotify took it. `value`: seconds for seek, 0/1 for shuffle and repeat, 0…100 for volume. */
  spotifyControl: (action: SpotifyAction, value?: number) =>
    call<boolean>("spotify_control", { action, value: value ?? null }),
  /** Brings Spotify forward or starts it; without it, its download page. */
  spotifyOpen: () => call<boolean>("spotify_open"),
  /** Whether there is a Spotify to launch (Settings). */
  spotifyInstalled: () => call<boolean>("spotify_installed"),
  /** Mochi's moonwalk song: "playing", "opened" (shown but not started) or "unavailable". */
  spotifyPlayTrack: (id: string, title: string) => call<string>("spotify_play_track", { id, title }),
  /** The playing song's lyrics (LRCLIB, kept on this computer). Rejects with "off" until the user turns them on. */
  lyricsGet: (title: string, artist: string, album: string, duration: number) =>
    callOrThrow<LyricsAnswer>("lyrics_get", { title, artist, album, duration }),

  // ── Sound mixer (src-tauri/src/mixer.rs) ──────────────────────────────────
  /** The outputs, the speakers' level and every program with sound. */
  mixerState: () => call<MixerState>("mixer_state"),
  /** Levels 0…100; null leaves that part as it is. */
  mixerSetMaster: (volume: number | null, muted: boolean | null) =>
    call<boolean>("mixer_set_master", { volume, muted }),
  mixerSetApp: (key: string, volume: number | null, muted: boolean | null) =>
    call<boolean>("mixer_set_app", { key, volume, muted }),
  /** Makes this output Windows' default one. */
  mixerSetDevice: (id: string) => call<boolean>("mixer_set_device", { id }),
  /** Windows' own sound settings. */
  mixerOpenSystem: () => call<boolean>("mixer_open_system"),
};

export interface LyricsAnswer {
  /** LRC: "[mm:ss.xx] words" lines. */
  synced: string | null;
  plain: string | null;
  instrumental: boolean;
}

export interface MixerDevice {
  id: string;
  name: string;
  current: boolean;
}

export interface MixerApp {
  /** The executable's name, or "system" for the system sounds. */
  key: string;
  name: string;
  volume: number;
  muted: boolean;
  /** Coucou itself (Mochi's sounds). */
  own: boolean;
}

export interface MixerState {
  /** False off Windows. */
  supported: boolean;
  devices: MixerDevice[];
  master: { volume: number; muted: boolean } | null;
  apps: MixerApp[];
}

export type SpotifyAction = "playPause" | "next" | "previous" | "seek" | "shuffle" | "repeat" | "volume";

export type ShortcutStatus =
  | "active" | "off" | "inUse" | "duplicate" | "invalid"
  | "typesCharacter" | "unsupported" | "notPorted"
  /** Wayland: handed to the desktop's portal, no answer yet / left out by it. */
  | "pending" | "refused";

export interface ShortcutsReport {
  /** `trigger`: on Wayland, the keys the desktop says run it, in its words. */
  actions: { id: string; status: ShortcutStatus; typed?: string; trigger?: string }[];
  /** "wayland" or "no-display" when no key grab can be made. */
  blocked: string | null;
  /** `<executable> --shortcut`: append an action id for a desktop shortcut. */
  command: string;
  /** Wayland: the shortcuts went to the desktop's GlobalShortcuts portal. */
  portal?: "pending" | "active" | null;
}

/** How the desktop Mochi's window works here (platform::DesktopMode). */
export type DesktopMode = "poll" | "window" | "layer" | "off";

export interface DesktopInfo {
  mode: DesktopMode;
  /** He was on the desktop when the app last quit. */
  onDesktop: boolean;
}

/** An event for one window only (island ⇄ desktop Mochi). Never throws. */
export async function emitToWindow(label: string, event: string, payload?: unknown) {
  if (!IS_TAURI) return;
  try {
    await emitTo(label, event, payload);
  } catch (err) {
    console.error(`[coucou] emit ${event} failed`, err);
  }
}

export interface IntegrationUpdate {
  id: string;
  data: Record<string, unknown>;
  error: string | null;
  event: { success: boolean; label: string; detail: string | null } | null;
}

export type ChatContext =
  | { kind: "file"; name: string; path: string }
  | { kind: "window"; appName: string; title: string; url?: string };

export interface ModelInfo {
  id: string;
  label: string;
}

export interface LocalServer {
  /** The address as it is stored. */
  url: string;
  models: string[];
  /** The address is this machine. */
  loopback: boolean;
}

export interface DroppedFile {
  name: string;
  path: string;
  size: number;
}

export interface HookStatus {
  installed: boolean;
  /** Coucou's status line relay (plan usage) is the status line in settings.json. */
  planRelayInstalled: boolean;
  settingsPath: string;
  hookPath: string;
  hookReady: boolean;
}

/** One agent other than Claude Code, as agents.rs reports it. */
export interface AgentHookStatus {
  /** The `--agent` name; its pill is `agent_<id>`. */
  id: string;
  name: string;
  installed: boolean;
  /** The file (or files, one per line) Coucou writes. */
  path: string;
  hookReady: boolean;
  /** The island can allow or deny this agent's permission requests. */
  approvals: boolean;
  /** What to do once it is written. */
  note: string;
}

export interface AgentHookPlan {
  diff: string;
  /** Where each existing file is copied first, one per line; "" when none. */
  backup: string;
  path: string;
  fingerprint: string;
}

export interface HookPreview {
  diff: string;
  backup: string;
  settingsPath: string;
  /** Hand back to hooksApply so only the reviewed diff is ever written. */
  fingerprint: string;
}

/** Same as `call`, but surfaces the error so the UI can show what went wrong. */
async function callOrThrow<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) throw new Error("not running inside Coucou");
  return invoke<T>(cmd, args);
}

export type BridgeEvent =
  | { name: "cursor"; payload: { x: number; y: number } }
  | { name: "tray"; payload: string }
  | { name: "hook"; payload: Record<string, unknown> }
  | { name: "screen-changed"; payload: null };

export interface DragDropPayload {
  type: "enter" | "over" | "drop" | "leave";
  paths?: string[];
}

interface WebView2Bridge {
  postMessageWithAdditionalObjects(message: unknown, objects: ArrayLike<unknown>): void;
}

/**
 * Files dragged onto the island. Only reaches us when the window takes the mouse.
 *
 * On Windows WebView2 takes the drop itself, as in Edge — Tauri's drop handling
 * never sees drags from the classic Explorer folder view (see
 * src-tauri/src/webview_drop.rs). Enter/over/leave come straight from the page;
 * the drop hands the File objects to Rust, which answers with their real paths
 * as a `file-drag` event. Elsewhere (Linux) Tauri's own drag events still do it.
 */
export async function onDragDrop(handler: (e: DragDropPayload) => void) {
  if (!IS_TAURI) return () => {};
  const webview = (window as unknown as { chrome?: { webview?: WebView2Bridge } }).chrome?.webview;
  if (!webview) {
    return getCurrentWebview().onDragDropEvent((event) => {
      handler(event.payload as DragDropPayload);
    });
  }
  const hasFiles = (e: DragEvent) => e.dataTransfer?.types.includes("Files") ?? false;
  // dragenter/dragleave fire for every element crossed; only the outermost pair counts.
  let depth = 0;

  const onEnter = (e: DragEvent) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (depth++ === 0) handler({ type: "enter" });
  };
  const onOver = (e: DragEvent) => {
    if (!hasFiles(e)) return;
    // Without this WebView2 refuses the drop — or navigates to the file.
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    handler({ type: "over" });
  };
  const onLeave = (e: DragEvent) => {
    if (!hasFiles(e) || depth === 0) return;
    if (--depth === 0) handler({ type: "leave" });
  };
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    depth = 0;
    const files = e.dataTransfer?.files;
    if (!files || files.length === 0) {
      handler({ type: "drop", paths: [] });
      return;
    }
    webview.postMessageWithAdditionalObjects("coucou-file-drop", files);
  };

  window.addEventListener("dragenter", onEnter);
  window.addEventListener("dragover", onOver);
  window.addEventListener("dragleave", onLeave);
  window.addEventListener("drop", onDrop);
  const unlisten = await listen<DragDropPayload>("file-drag", (e) => handler(e.payload));
  return () => {
    window.removeEventListener("dragenter", onEnter);
    window.removeEventListener("dragover", onOver);
    window.removeEventListener("dragleave", onLeave);
    window.removeEventListener("drop", onDrop);
    unlisten();
  };
}

export async function onEvent<T>(name: string, handler: (payload: T) => void) {
  if (!IS_TAURI) return () => {};
  return listen<T>(name, (e) => handler(e.payload));
}
