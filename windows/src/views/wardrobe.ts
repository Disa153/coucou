// Wardrobe view — port of WardrobeView (IslandViewContent.swift). Opened with a
// right-click on Mochi or from the tray menu. Resting the pointer on a button
// tries the outfit on Mochi; a click keeps it.

import { h } from "./dom";
import { State } from "../core/state";
import { drawWardrobeIcon } from "../mochi/outfits";
import {
  OUTFIT_KEYS, OUTFIT_SELECTIONS, WARDROBE_STRINGS, parseOutfit, resolveOutfit, seasonalOutfit,
  wardrobeHeader, type Outfit, type OutfitSelection,
} from "../mochi/wardrobe";
import type { ViewActions, ViewHost } from "./views";
import { language, t, tl } from "../i18n/i18n";
import { BREEDS, BREED_KEYS, drawBreedIcon, isRealistic, parseBreed, type Breed } from "../mochi/breeds";

const ICON = 28;

export function buildWardrobe(actions: ViewActions): ViewHost {
  const note = h("span", { class: "wardrobe-note" });
  const grid = h("div", { class: "wardrobe-grid" });
  const breedGrid = h("div", { class: "wardrobe-grid" });
  const el = h(
    "div",
    { class: "view wardrobe" },
    h(
      "div",
      { class: "card" },
      h(
        "div",
        { class: "stack wardrobe-stack" },
        h("div", { class: "wardrobe-head" }, h("span", { class: "wardrobe-title", text: tl("Wardrobe") }), note),
        grid,
        h("div", { class: "wardrobe-head" }, h("span", { class: "wardrobe-title", text: tl("Breed") })),
        breedGrid,
      ),
    ),
  );

  let hovered: OutfitSelection | null = null;
  let drawnSeason: Outfit | null = null;
  let drawnLanguage = language();
  const items = new Map<OutfitSelection, { button: HTMLButtonElement; canvas: HTMLCanvasElement }>();

  const updateNote = () => {
    note.textContent = wardrobeHeader(hovered, parseOutfit(State.settings.mochiOutfit), new Date());
  };

  for (const sel of OUTFIT_SELECTIONS) {
    const canvas = h("canvas");
    const button = h(
      "button",
      { class: "wardrobe-item", title: tl(OUTFIT_KEYS[sel]), "aria-label": tl(OUTFIT_KEYS[sel]) },
      canvas,
    );
    button.addEventListener("mouseenter", () => {
      hovered = sel;
      actions.previewOutfit(resolveOutfit(sel, new Date()));
      updateNote();
    });
    button.addEventListener("mouseleave", () => {
      if (hovered !== sel) return;
      hovered = null;
      actions.previewOutfit(null);
      updateNote();
    });
    button.addEventListener("click", () => actions.chooseOutfit(sel));
    items.set(sel, { button, canvas });
    grid.append(button);
  }

  const breedItems = new Map<Breed, HTMLButtonElement>();
  let hoveredBreed: Breed | null = null;
  const breedNote = (b: Breed) =>
    isRealistic(b) ? `${t(BREED_KEYS[b])} · ${t("no outfits")}` : t(BREED_KEYS[b]);
  for (const b of BREEDS) {
    const canvas = h("canvas") as HTMLCanvasElement;
    const button = h(
      "button",
      { class: "wardrobe-item", title: tl(BREED_KEYS[b]), "aria-label": tl(BREED_KEYS[b]) },
      canvas,
    ) as HTMLButtonElement;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(ICON * dpr);
    canvas.height = Math.round(ICON * dpr);
    const ctx = canvas.getContext("2d");
    if (ctx) {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      drawBreedIcon(ctx, ICON, b);
    }
    button.addEventListener("mouseenter", () => {
      hoveredBreed = b;
      actions.previewBreed(b);
      note.textContent = breedNote(b);
    });
    button.addEventListener("mouseleave", () => {
      if (hoveredBreed !== b) return;
      hoveredBreed = null;
      actions.previewBreed(null);
      updateNote();
    });
    button.addEventListener("click", () => actions.chooseBreed(b));
    breedItems.set(b, button);
    breedGrid.append(button);
  }

  /** Icons are drawn once; only "auto" changes, with the season and its badge's language. */
  const drawIcons = () => {
    const season = seasonalOutfit(new Date());
    if (season === drawnSeason && drawnLanguage === language()) return;
    const first = drawnSeason == null;
    drawnSeason = season;
    drawnLanguage = language();
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    for (const [sel, { canvas }] of items) {
      if (!first && sel !== "auto") continue;
      canvas.width = Math.round(ICON * dpr);
      canvas.height = Math.round(ICON * dpr);
      const ctx = canvas.getContext("2d");
      if (!ctx) continue;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      drawWardrobeIcon(ctx, ICON, sel, season, WARDROBE_STRINGS.autoBadge);
    }
  };

  return {
    el,
    sync() {
      drawIcons();
      // The island drops the preview when the view closes: so does the hover.
      if (State.wardrobePreview == null) hovered = null;
      const current = parseOutfit(State.settings.mochiOutfit);
      for (const [sel, { button }] of items) button.classList.toggle("on", sel === current);
      const breed = parseBreed(State.settings.mochiBreed);
      for (const [b, button] of breedItems) button.classList.toggle("on", b === breed);
      if (State.breedPreview == null) hoveredBreed = null;
      if (hoveredBreed) note.textContent = breedNote(hoveredBreed);
      else updateNote();
    },
  };
}
