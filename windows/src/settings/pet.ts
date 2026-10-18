// Settings → General → Mochi: his hunger and the moonwalk's song in Spotify
// (core/pet.ts). Feeding itself happens in the island's Kitchen.

import { readPet, type PetPref, type SongPick } from "../core/pet";
import type { Settings } from "../core/state";
import { h } from "../views/dom";
import { t } from "../i18n/i18n";

type Toggle = (on: boolean, onChange: (v: boolean) => void) => HTMLElement;

/** `current`: the settings as they are now (the window replaces its copy on every save). */
export function petSection(current: () => Settings, makeToggle: Toggle, save: () => void): HTMLElement {
  // What the island wrote last (snacks eaten) is kept: only these fields change here.
  const store = (change: Partial<PetPref>) => {
    const s = current();
    s.pet = { ...readPet(s.pet), ...change };
    save();
  };
  const pet = readPet(current().pet);

  const song = h("select", {}) as HTMLSelectElement;
  for (const [value, label] of [
    ["billieJean", "Billie Jean"], ["smoothCriminal", "Smooth Criminal"], ["bad", "Bad"], ["beatIt", "Beat It"],
    ["random", t("A random one of these")],
  ] as [SongPick, string][]) song.append(h("option", { value, text: label }));
  song.value = pet.song;
  song.addEventListener("change", () => store({ song: song.value as SongPick }));

  const after = h("select", {}) as HTMLSelectElement;
  after.append(h("option", { value: "pause", text: t("Pause it") }), h("option", { value: "keep", text: t("Let it play") }));
  after.value = pet.after;
  after.addEventListener("change", () => store({ after: after.value === "keep" ? "keep" : "pause" }));

  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: "Mochi" })),
    h("div", { class: "row" },
      h("label", { text: t("Hunger") }),
      makeToggle(pet.hunger, (v) => store({ hunger: v })),
      h("span", { class: "hint", text: t("He slowly gets hungry; feed him from the onigiri in the island.") }),
    ),
    h("div", { class: "row" },
      h("label", { text: t("Moonwalk music") }),
      makeToggle(pet.music, (v) => store({ music: v })),
      h("span", { class: "hint", text: t("Plays Michael Jackson in Spotify while he moonwalks.") }),
    ),
    h("div", { class: "row" }, h("label", { text: t("Song") }), song),
    h("div", { class: "row" }, h("label", { text: t("After the dance") }), after),
  );
}
