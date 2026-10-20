// The beta chat's messages and composer (src-tauri/src/betachat.rs), shared by
// the island's view (views/betachat.ts) and the chat's own window (./main.ts).
//
// A message shows at once with a grey ✓ and turns ✓✓ when GitHub has it, or
// says it did not go, with "Retry". Photos, videos and files are attached with
// 📎, by pasting (a screenshot from the clipboard) or, in the window, by
// dropping them; photos are made smaller first. While shown, the chat is read
// every 10 s — only what changed.

import { h, svg, clear } from "../views/dom";
import { ICONS } from "../views/icons";
import { Bridge, onEvent, type ChatFile, type ChatMessage, type OutFile } from "../core/bridge";
import { t, tl } from "../i18n/i18n";
import { reviewErrorText } from "../views/review";
import "./panel.css";

const POLL_MS = 10_000;
/** As in betachat.rs. */
const FILE_MAX = 7 * 1024 * 1024;
const FILES_MAX = 4;
/** Photos are sent at most this wide or tall. */
const PHOTO_SIDE = 1920;
const COLORS = ["#f9a8d4", "#93c5fd", "#86efac", "#fcd34d", "#c4b5fd", "#fdba74", "#5eead4"];

function color(login: string): string {
  let sum = 0;
  for (const c of login.toLowerCase()) sum += c.charCodeAt(0);
  return COLORS[sum % COLORS.length];
}

function clock(at: number): string {
  const d = new Date(at * 1000);
  const today = new Date().toDateString() === d.toDateString();
  return today
    ? d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

export function sizeText(bytes: number): string {
  return bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function toBase64(bytes: Uint8Array): string {
  let s = "";
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) s += String.fromCharCode(...bytes.subarray(i, i + step));
  return btoa(s);
}

function fromBase64(data: string): Uint8Array {
  const s = atob(data);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

/** A photo made smaller (JPEG) when it is big; anything else as it is. */
async function prepare(file: File): Promise<{ blob: Blob; name: string; mime: string }> {
  const keep = { blob: file as Blob, name: file.name || "file", mime: file.type || "application/octet-stream" };
  if (!file.type.startsWith("image/") || file.type === "image/gif" || file.type === "image/svg+xml") return keep;
  try {
    const bmp = await createImageBitmap(file);
    const k = Math.min(1, PHOTO_SIDE / Math.max(bmp.width, bmp.height));
    if (k === 1 && file.size < 1.5 * 1024 * 1024) return keep;
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(bmp.width * k);
    canvas.height = Math.round(bmp.height * k);
    canvas.getContext("2d")!.drawImage(bmp, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise<Blob | null>((ok) => canvas.toBlob(ok, "image/jpeg", 0.85));
    if (!blob) return keep;
    const base = (file.name || "photo").replace(/\.[^.]+$/, "");
    return { blob, name: `${base}.jpg`, mime: "image/jpeg" };
  } catch {
    return keep;
  }
}

/** Attachments fetched once per address (they never change). */
const blobs = new Map<string, Promise<string>>();
function blobUrl(f: ChatFile): Promise<string> {
  let p = blobs.get(f.url);
  if (!p) {
    p = Bridge.betaChatFile(f.url).then((data) => URL.createObjectURL(new Blob([fromBase64(data) as BlobPart], { type: f.mime })));
    p.catch(() => blobs.delete(f.url));
    blobs.set(f.url, p);
  }
  return p;
}

interface Pending {
  key: number;
  text: string;
  build: number | null;
  files: { name: string; mime: string; size: number }[];
  failed: boolean;
  retry: () => void;
}

export interface ChatPanel {
  el: HTMLElement;
  /** On screen or not: reading every 10 s only while shown. */
  shown(on: boolean): void;
  focus(): void;
}

export interface PanelOptions {
  openUrl(url: string): void;
  /** Read again: a new message arrived (the island's count goes back to 0). */
  onRead?(): void;
  /** The window takes dropped files; the island leaves drops to the upload flow. */
  acceptsDrops?: boolean;
  /** Extra buttons for the head (the island's back, the window's pin). */
  lead?: HTMLElement[];
  tail?: HTMLElement[];
}

export function createChatPanel(opts: PanelOptions): ChatPanel {
  const openGh = h("button", { class: "bc-link", text: tl("On GitHub") });
  const head = h("div", { class: "bc-head" }, ...(opts.lead ?? []), h("b", { text: tl("Beta chat") }), h("span", { class: "bc-grow" }), ...(opts.tail ?? []), openGh);
  const list = h("div", { class: "bc-list" });
  const note = h("div", { class: "bc-note" });
  const tray = h("div", { class: "bc-tray" });
  const input = h("textarea", { class: "bc-input", rows: "1", maxlength: "2000" }) as HTMLTextAreaElement;
  input.placeholder = t("Write to the testers… (not passwords or keys: the chat is public on GitHub)");
  const picker = h("input", { type: "file", multiple: "", style: "display:none" }) as HTMLInputElement;
  const clip = h("button", { class: "bc-clip", title: tl("Attach a photo, a video or a file") }, svg(ICONS.plus, 13, { stroke: 2.2 }));
  const buildChip = h("button", { class: "bc-chip" });
  const send = h("button", { class: "bc-send", title: tl("Send") }, svg(ICONS.arrowUp, 13, { stroke: 2.2 }));
  const composer = h("div", { class: "bc-composer" }, clip, buildChip, input, send, picker);
  const el = h("div", { class: "bc-panel" }, head, list, note, tray, composer);

  let page = "";
  let owner = false;
  let build = 0;
  let withBuild = false;
  let messages: ChatMessage[] = [];
  const pending: Pending[] = [];
  let attached: { blob: Blob; name: string; mime: string; preview: string }[] = [];
  let timer: number | null = null;
  let isShown = false;
  let lastId = 0;
  let keys = 0;

  openGh.addEventListener("click", () => {
    if (page) opts.openUrl(page);
  });
  buildChip.addEventListener("click", () => {
    withBuild = !withBuild;
    paintChip();
  });
  function paintChip() {
    buildChip.style.display = build > 0 ? "" : "none";
    buildChip.classList.toggle("on", withBuild);
    buildChip.textContent = `🧪 ${build}`;
    buildChip.title = t("Attach your build number");
  }

  // ── Attachments ─────────────────────────────────────────────────────────────

  async function attach(files: File[]) {
    for (const file of files) {
      if (attached.length >= FILES_MAX) {
        note.textContent = t("At most {count} attachments in one message.", { count: FILES_MAX });
        break;
      }
      const ready = await prepare(file);
      if (ready.blob.size > FILE_MAX) {
        note.textContent = t("{name} is over 7 MB: the chat cannot take it.", { name: ready.name });
        continue;
      }
      const preview = ready.mime.startsWith("image/") ? URL.createObjectURL(ready.blob) : "";
      attached.push({ ...ready, preview });
    }
    paintTray();
  }
  function paintTray() {
    clear(tray);
    tray.style.display = attached.length ? "" : "none";
    attached.forEach((a, i) => {
      const x = h("button", { class: "bc-x", title: tl("Remove"), text: "×" });
      x.addEventListener("click", () => {
        if (a.preview) URL.revokeObjectURL(a.preview);
        attached.splice(i, 1);
        paintTray();
      });
      const look = a.preview
        ? h("img", { class: "bc-thumb", src: a.preview, alt: a.name })
        : h("span", { class: "bc-fileicon", text: a.mime.startsWith("video/") ? "🎬" : "📄" });
      tray.append(h("div", { class: "bc-att" }, look, h("span", { class: "bc-attname", text: `${a.name} · ${sizeText(a.blob.size)}` }), x));
    });
  }
  clip.addEventListener("click", () => picker.click());
  picker.addEventListener("change", () => {
    void attach(Array.from(picker.files ?? []));
    picker.value = "";
  });
  input.addEventListener("paste", (e) => {
    const files = Array.from(e.clipboardData?.items ?? [])
      .filter((i) => i.kind === "file")
      .map((i) => i.getAsFile())
      .filter((f): f is File => !!f)
      .map((f) => (f.name ? f : new File([f], `clipboard-${Date.now()}.png`, { type: f.type })));
    if (files.length) {
      e.preventDefault();
      void attach(files);
    }
  });
  if (opts.acceptsDrops) {
    el.addEventListener("dragover", (e) => {
      e.preventDefault();
      el.classList.add("drop");
    });
    el.addEventListener("dragleave", () => el.classList.remove("drop"));
    el.addEventListener("drop", (e) => {
      e.preventDefault();
      el.classList.remove("drop");
      void attach(Array.from(e.dataTransfer?.files ?? []));
    });
  }

  // ── Messages ────────────────────────────────────────────────────────────────

  function fileView(f: ChatFile): HTMLElement {
    const box = h("div", { class: "bc-file" });
    if (f.mime.startsWith("image/")) {
      const img = h("img", { class: "bc-img", alt: f.name }) as HTMLImageElement;
      box.append(img);
      void blobUrl(f).then((u) => (img.src = u)).catch(() => box.append(h("span", { class: "bc-time", text: t("Could not load {name}.", { name: f.name }) })));
      img.addEventListener("click", () => box.classList.toggle("big"));
      return box;
    }
    if (f.mime.startsWith("video/")) {
      const play = h("button", { class: "bc-filebtn", text: `▶ ${f.name} · ${sizeText(f.size)}` });
      play.addEventListener("click", () => {
        play.textContent = t("Loading…");
        void blobUrl(f)
          .then((u) => {
            const video = h("video", { class: "bc-video", src: u, controls: "", autoplay: "" });
            box.replaceChildren(video);
          })
          .catch(() => (play.textContent = t("Could not load {name}.", { name: f.name })));
      });
      box.append(play);
      return box;
    }
    const save = h("button", { class: "bc-filebtn", text: `📄 ${f.name} · ${sizeText(f.size)}` });
    save.addEventListener("click", () => {
      save.textContent = t("Saving…");
      Bridge.betaChatSave(f.url, f.name)
        .then((path) => (save.textContent = t("Saved in {path}", { path })))
        .catch((e: unknown) => (save.textContent = reviewErrorText(e)));
    });
    box.append(save);
    return box;
  }

  function row(m: { login: string; owner: boolean; mine: boolean; text: string; build: number | null }, body: HTMLElement[], meta: HTMLElement): HTMLElement {
    const av = h("span", { class: "bc-av", text: m.login.charAt(0).toUpperCase() });
    av.style.background = color(m.login);
    const who = h("div", { class: "bc-who" }, h("span", { text: `@${m.login}` }));
    if (m.owner) who.append(h("span", { class: "bc-owner", text: ` · ${t("owner")}` }));
    const bubble = h("div", { class: "bc-bubble" });
    if (m.build) bubble.append(h("span", { class: "bc-tag", text: t("🧪 build {build}", { build: m.build }) }));
    if (m.text) bubble.append(document.createTextNode(m.text));
    bubble.append(...body);
    return h("div", { class: m.mine ? "bc-msg mine" : "bc-msg" }, av, h("div", { class: "bc-body" }, who, bubble, meta));
  }

  function paint() {
    const atBottom = list.scrollTop + list.clientHeight >= list.scrollHeight - 30;
    clear(list);
    if (!messages.length && !pending.length) list.append(h("div", { class: "bc-empty", text: t("No messages yet. Say hello to the testers!") }));
    for (const m of messages) {
      const meta = h("div", { class: "bc-time" }, h("span", { text: m.at ? clock(m.at) : "" }));
      if (m.mine) meta.append(h("span", { class: "bc-tick ok", text: "✓✓" }));
      if (m.mine || owner) {
        const del = h("button", { class: "bc-del", title: tl("Delete"), text: "×" });
        del.addEventListener("click", () => {
          del.setAttribute("disabled", "");
          messages = messages.filter((x) => x.id !== m.id);
          paint();
          Bridge.betaChatDelete(m.id).catch((e: unknown) => {
            note.textContent = reviewErrorText(e);
            void refresh(true);
          });
        });
        meta.append(del);
      }
      list.append(row(m, m.files.map(fileView), meta));
    }
    const me = messages.find((m) => m.mine)?.login ?? "";
    for (const p of pending) {
      const meta = h("div", { class: "bc-time" });
      if (p.failed) {
        const retry = h("button", { class: "bc-retry", text: t("Not sent · Retry") });
        retry.addEventListener("click", p.retry);
        meta.append(retry);
      } else {
        meta.append(h("span", { text: t("Sending…") }), h("span", { class: "bc-tick", text: "✓" }));
      }
      const files = p.files.map((f) => h("div", { class: "bc-file" }, h("span", { class: "bc-filebtn", text: `📎 ${f.name} · ${sizeText(f.size)}` })));
      list.append(row({ login: me || "…", owner, mine: true, text: p.text, build: p.build }, files, meta));
    }
    const newest = messages.at(-1)?.id ?? 0;
    if (atBottom || newest !== lastId || pending.length) list.scrollTop = list.scrollHeight;
    lastId = newest;
  }

  let reading = false;
  async function refresh(full = false) {
    if (reading) return;
    reading = true;
    try {
      const p = await Bridge.betaChatMessages(full);
      page = p.page;
      owner = p.owner;
      build = p.build;
      paintChip();
      openGh.style.display = page ? "" : "none";
      if (note.dataset.kind === "read") note.textContent = "";
      messages = p.messages;
      opts.onRead?.();
      paint();
    } catch (e) {
      note.dataset.kind = "read";
      note.textContent = reviewErrorText(e);
    } finally {
      reading = false;
    }
  }

  async function submit() {
    const text = input.value.trim();
    if (!text && !attached.length) return;
    const files = attached;
    attached = [];
    paintTray();
    input.value = "";
    const p: Pending = {
      key: ++keys,
      text,
      build: withBuild && build > 0 ? build : null,
      files: files.map((f) => ({ name: f.name, mime: f.mime, size: f.blob.size })),
      failed: false,
      retry: () => {
        p.failed = false;
        paint();
        void go();
      },
    };
    const sendBuild = withBuild;
    withBuild = false;
    paintChip();
    pending.push(p);
    paint();
    let out: OutFile[] | null = null;
    const go = async () => {
      try {
        out ??= await Promise.all(files.map(async (f) => ({ name: f.name, mime: f.mime, data: toBase64(new Uint8Array(await f.blob.arrayBuffer())) })));
        const sent = await Bridge.betaChatSend(text, sendBuild, out);
        pending.splice(pending.indexOf(p), 1);
        for (const f of files) if (f.preview) URL.revokeObjectURL(f.preview);
        if (!messages.some((m) => m.id === sent.id)) messages = [...messages, sent];
        note.textContent = "";
        paint();
      } catch (e) {
        p.failed = true;
        note.dataset.kind = "send";
        note.textContent = reviewErrorText(e);
        paint();
      }
    };
    await go();
  }
  send.addEventListener("click", () => void submit());
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  });
  // Sent or deleted in the other window (or from the phone): read again now.
  void onEvent<null>("beta-chat-changed", () => {
    if (isShown) void refresh();
  });

  paintTray();
  return {
    el,
    shown(on) {
      if (on === isShown) return;
      isShown = on;
      if (on) {
        void refresh(true);
        timer = window.setInterval(() => void refresh(), POLL_MS);
      } else if (timer != null) {
        window.clearInterval(timer);
        timer = null;
      }
    },
    focus() {
      input.focus();
    },
  };
}
