/**
 * Appearance settings. Applied as attributes and CSS variables on <html>, so the
 * whole UI (and the pre-paint boot script) share one source of truth.
 */

export type ThemePref = "system" | "light" | "dark";
export type FontId = "literata" | "source" | "garamond" | "plexSerif" | "grotesk" | "typewriter";
export type InkId = "vermilion" | "oxblood" | "ochre" | "graphite";
export type Leading = "compact" | "comfortable" | "airy";
export type Measure = "narrow" | "medium" | "wide";

export interface Settings {
  theme: ThemePref;
  font: FontId;
  size: number;
  leading: Leading;
  measure: Measure;
  ink: InkId;
}

export const DEFAULTS: Settings = {
  theme: "system",
  font: "literata",
  size: 18.5,
  leading: "comfortable",
  measure: "medium",
  ink: "vermilion",
};

export const FONTS: Record<FontId, { name: string; kind: string; stack: string; scale: number }> = {
  literata: { name: "Literata", kind: "Book", stack: "var(--font-text), Georgia, serif", scale: 1 },
  source: { name: "Source Serif", kind: "Transitional", stack: "var(--font-source), Georgia, serif", scale: 1 },
  garamond: { name: "EB Garamond", kind: "Old style", stack: "var(--font-garamond), Garamond, serif", scale: 1.08 },
  plexSerif: { name: "Plex Serif", kind: "Contemporary", stack: "var(--font-plex-serif), Georgia, serif", scale: 0.97 },
  grotesk: { name: "Plex Sans", kind: "Grotesk", stack: "var(--font-ui), system-ui, sans-serif", scale: 0.95 },
  typewriter: { name: "Plex Mono", kind: "Typewriter", stack: "var(--font-mono-ui), ui-monospace, monospace", scale: 0.86 },
};

/** Hue, chroma and a lightness offset; the light and dark palettes derive every tint from these. */
export const INKS: Record<InkId, { name: string; h: number; c: number; dl: number }> = {
  vermilion: { name: "Vermilion", h: 34, c: 0.165, dl: 0 },
  oxblood: { name: "Oxblood", h: 20, c: 0.14, dl: -0.09 },
  ochre: { name: "Ochre", h: 66, c: 0.13, dl: 0.03 },
  graphite: { name: "Graphite", h: 70, c: 0.012, dl: -0.1 },
};

export const LEADING: Record<Leading, number> = { compact: 1.55, comfortable: 1.72, airy: 1.9 };
export const MEASURE: Record<Measure, number> = { narrow: 620, medium: 720, wide: 840 };

export const SIZE_MIN = 15;
export const SIZE_MAX = 22;
export const STORAGE_KEY = "forge:settings";

const MAPS = { fonts: FONTS, inks: INKS, leading: LEADING, measure: MEASURE };

/**
 * Self-contained on purpose: it is also serialised into the pre-paint boot script,
 * so it may only use its arguments and browser globals.
 */
function applyToRoot(s: Settings, M: typeof MAPS) {
  const r = document.documentElement;
  const dark = s.theme === "dark" || (s.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  r.dataset.theme = dark ? "dark" : "light";
  const ink = M.inks[s.ink] || M.inks.vermilion;
  r.style.setProperty("--ink-h", String(ink.h));
  r.style.setProperty("--ink-c", String(ink.c));
  r.style.setProperty("--ink-dl", String(ink.dl));
  const f = M.fonts[s.font] || M.fonts.literata;
  r.style.setProperty("--doc-font", f.stack);
  r.style.setProperty("--doc-size", Math.round(s.size * f.scale * 10) / 10 + "px");
  r.style.setProperty("--doc-leading", String(M.leading[s.leading] || 1.72));
  r.style.setProperty("--doc-measure", (M.measure[s.measure] || 720) + "px");
}

export function applySettings(s: Settings) {
  applyToRoot(s, MAPS);
}

export function loadSettings(): Settings {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(STORAGE_KEY) || "{}") };
  } catch {
    return DEFAULTS;
  }
}

export function saveSettings(s: Settings) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {}
}

/** Runs before first paint (inlined in <head>) so a saved dark theme never flashes light. */
export const BOOT_SCRIPT = `(function(){try{var s=Object.assign(${JSON.stringify(DEFAULTS)},JSON.parse(localStorage.getItem(${JSON.stringify(
  STORAGE_KEY,
)})||"{}"));(${applyToRoot.toString()})(s,${JSON.stringify(MAPS)});}catch(e){}})();`;
