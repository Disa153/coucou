// « OK Coucou »: say it, then ask anything. Turning it on, how Coucou answers
// out loud, and what leaves the computer. Port of the Mac's Settings → Voice.

import { h, clear } from "../../views/dom";
import { t } from "../../i18n/i18n";
import { Bridge } from "../../core/bridge";
import type { VoiceOutput } from "../../core/flow";
import type { HubContext } from "../shell";
import { button, card, divider, label, row, segmented, toggle } from "../ui";
import { modelsControl } from "./flowsettings";

const ELEVEN = "elevenlabs-api-key";
/** Where secrets.rs keeps the keys on this OS (same words as Notch → Chat). */
const KEY_STORE = navigator.userAgent.includes("Windows") ? "Windows Credential Manager" : "Secret Service (GNOME Keyring, KWallet)";

function elevenKey(): HTMLElement {
  const box = h("div", { style: "display:flex;align-items:center;gap:8px" });
  const draw = async () => {
    const present = (await Bridge.secretPresent(ELEVEN)) ?? false;
    clear(box);
    if (present) {
      box.append(
        h("span", { class: "hub-note", style: "color:var(--hub-green)", text: `✓ ${t("Key saved in the {store}.", { store: KEY_STORE })}` }),
        button(t("Remove"), () => void Bridge.secretClear(ELEVEN).then(draw), "", true),
      );
      return;
    }
    const input = h("input", { class: "hub-input", type: "password", placeholder: t("ElevenLabs key"), autocomplete: "off", spellcheck: "false", style: "width:220px" }) as HTMLInputElement;
    const save = button(t("Save key"), () => {
      const value = input.value.trim();
      if (!value) return;
      void Bridge.secretSet(ELEVEN, value).then(draw, () => input.classList.add("err"));
    }, "primary", true);
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") save.click();
    });
    box.append(input, save);
  };
  void draw();
  return box;
}

/** Settings → Voice: an ElevenLabs voice from the Voice Library, by its ID or its page's link. */
function elevenVoice(ctx: HubContext, current: string): HTMLElement {
  const input = h("input", { class: "hub-input", type: "text", value: current, placeholder: "rQOBu7YxCDxGiFdTm28w", spellcheck: "false", style: "width:220px" }) as HTMLInputElement;
  const listen = button(t("Listen"), () => void Bridge.voicePreview(t("Done. I passed the task to Claude, the tests are running. I'll tell you when they finish.")), "", true);
  const keep = () => {
    const raw = input.value.trim();
    const id = raw.replace(/\/+$/, "").split("/").pop()?.split(/[?#]/)[0] ?? "";
    input.value = id;
    void ctx.setPrefs((p) => (p.elevenVoice = id));
  };
  input.addEventListener("change", keep);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") input.blur();
  });
  return h("div", { style: "display:flex;align-items:center;gap:8px" }, input, listen);
}

/** Settings → Voice: the phrase that wakes the assistant, saved when the field is left. */
function wakePhrase(ctx: HubContext, current: string): HTMLElement {
  const input = h("input", { class: "hub-input", type: "text", value: current, placeholder: "Привет, Диса", spellcheck: "false", style: "width:220px" }) as HTMLInputElement;
  input.addEventListener("change", () => {
    const phrase = input.value.trim().replace(/\s+/g, " ");
    if (phrase.replace(/[^\p{L}\p{N}]/gu, "").length < 4) {
      input.classList.add("err");
      return;
    }
    input.classList.remove("err");
    void ctx.setPrefs((p) => (p.wakePhrase = phrase)).then(() => ctx.navigate("voice"));
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") input.blur();
  });
  return input;
}

export async function voicePage(page: HTMLElement, ctx: HubContext) {
  const status = await ctx.refresh();
  const prefs = status.prefs;
  const wake = prefs.wakePhrase || "Привет, Диса";
  page.append(ctx.hero("voice", wake, t("Say « {phrase} » (or « OK Coucou »), then what to do: it goes to the chat with Claude, and Coucou says the answer.", { phrase: wake })));

  const ready = status.models.parakeet.state === "ready";
  page.append(label(t("Listening")));
  const main = card(
    row(t("Listen for the wake phrase"), t("The microphone stays open; only what follows the wake phrase goes further."),
      toggle(prefs.voice, (on) => void ctx.setPrefs((p) => (p.voice = on)).then(() => ctx.navigate("voice")))),
    divider(),
    row(t("Wake phrase"), t("Two or three words, said the same way each time. A letter or two may differ."), wakePhrase(ctx, wake)),
    divider(),
    row(t("Send it to the chat with Claude"), t("What you say goes into the island's chat, like typed there: Claude Code answers, no key needed. Off: Coucou answers by itself with your Claude key."),
      toggle(prefs.voiceToChat, (on) => void ctx.setPrefs((p) => (p.voiceToChat = on)).then(() => ctx.navigate("voice")))),
    divider(),
    row(t("Parakeet, on your computer"), t("Hears « OK Coucou » here, without the internet."), modelsControl(ctx, () => ctx.navigate("voice"))),
  );
  if (prefs.voice && !prefs.voiceToChat && !status.hasClaudeKey) {
    main.append(h("div", { class: "hub-warn", text: t("Coucou answers with Claude: add your Claude key in Notch → Chat.") }));
  } else if (prefs.voice && !ready) {
    main.append(h("div", { class: "hub-warn", text: t("Download the voice model so Coucou can hear you.") }));
  }
  page.append(main);

  page.append(label(t("Answers")));
  const outputs: [VoiceOutput, string][] = [["system", t("System voice")], ["elevenlabs", "ElevenLabs"], ["off", t("Text only")]];
  const answers = card(
    row(t("Coucou answers out loud"), null, segmented(outputs, prefs.voiceOutput, (v) => void ctx.setPrefs((p) => (p.voiceOutput = v)).then(() => ctx.navigate("voice")))),
  );
  if (prefs.voiceOutput === "elevenlabs") {
    answers.append(
      divider(),
      row(t("Voice from the Voice Library"), t("Its ID or the link to its page on elevenlabs.io. Empty: one of your voices, by gender."), elevenVoice(ctx, prefs.elevenVoice)),
    );
    if (!prefs.elevenVoice) {
      answers.append(
        divider(),
        row(t("Voice"), null, segmented([["female", t("Female")], ["male", t("Male")]], prefs.voiceGender, (g) => void ctx.setPrefs((p) => (p.voiceGender = g)))),
      );
    }
    answers.append(divider(), row(t("ElevenLabs key"), null, elevenKey()));
  }
  page.append(answers);

  page.append(label(t("Privacy")));
  page.append(card(
    h("div", { class: "hub-note", text: t("Your voice never leaves this computer: Parakeet turns it into text here. Only the words after « OK Coucou » go to Anthropic with your Claude key, and to ElevenLabs if you picked it for the answers.") }),
    divider(),
    h("div", { class: "hub-note", text: t("An email is never sent by Coucou: your mail app opens with it written, and you send it.") }),
  ));
}
