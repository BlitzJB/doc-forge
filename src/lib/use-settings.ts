"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { applySettings, loadSettings, saveSettings, type Settings } from "./settings";

export function useSettings() {
  const [settings, setSettings] = useState<Settings>(loadSettings);
  const current = useRef(settings);

  useEffect(() => {
    current.current = settings;
    applySettings(settings);
    if (settings.theme !== "system") return;
    // Follow the OS while on "System".
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applySettings(settings);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [settings]);

  const update = useCallback((patch: Partial<Settings>) => {
    const next = { ...current.current, ...patch };
    current.current = next;
    saveSettings(next);
    // Crossfade the whole page when the theme flips, instead of a hard cut.
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (patch.theme && !reduce && "startViewTransition" in document) {
      document.startViewTransition(() => applySettings(next));
    }
    setSettings(next);
  }, []);

  return { settings, update };
}
