"use client";

import { CliSetting } from "./CliInstall";
import { isDesktop } from "@/lib/desktop";
import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import {
  DEFAULTS,
  FONTS,
  INKS,
  SIZE_MAX,
  SIZE_MIN,
  type FontId,
  type InkId,
  type Leading,
  type Measure,
  type Settings,
  type ThemePref,
} from "@/lib/settings";
import type { EditMode } from "@/lib/types";
import { Segmented } from "./Segmented";
import { Heading, Row } from "./SettingsParts";
import { ModelsSettings } from "./ModelsSettings";

type Section = "appearance" | "models" | "agent";

interface Props {
  open: boolean;
  onClose: () => void;
  settings: Settings;
  update: (patch: Partial<Settings>) => void;
  mode: EditMode;
  setMode: (m: EditMode) => void;
  webSearch: boolean;
  setWebSearch: (b: boolean) => void;
}

const SECTIONS: { id: Section; label: string; hint: string }[] = [
  { id: "appearance", label: "Appearance", hint: "Theme, type and ink" },
  { id: "models", label: "Models", hint: "Providers and keys" },
  { id: "agent", label: "Editor", hint: "How Forge edits" },
];

export function SettingsDialog(p: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const [section, setSection] = useState<Section>("appearance");

  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (p.open && !d.open) {
      d.showModal();
      // Focus the dialog itself rather than its first control, so no focus ring flashes on open.
      d.focus();
    }
    if (!p.open && d.open) d.close();
  }, [p.open]);

  return (
    <dialog
      ref={ref}
      className="settings"
      aria-label="Settings"
      tabIndex={-1}
      onClose={p.onClose}
      // Esc is handled here so inner layers (like the model picker) can consume it first.
      onCancel={(e) => e.preventDefault()}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !e.defaultPrevented) {
          e.preventDefault();
          p.onClose();
        }
      }}
      onClick={(e) => e.target === ref.current && p.onClose()}
    >
      <div className="flex h-[min(640px,88vh)] w-[min(880px,94vw)]">
        <nav className="flex w-[200px] shrink-0 flex-col border-r border-rule bg-margin px-3 py-5 max-[640px]:w-[150px]">
          <div className="label mb-3 px-3">Settings</div>
          <ul className="flex flex-col gap-0.5">
            {SECTIONS.map((s) => {
              const on = section === s.id;
              return (
                <li key={s.id}>
                  <button
                    onClick={() => setSection(s.id)}
                    aria-current={on}
                    className={`press w-full rounded-[4px] px-3 py-2 text-left transition-colors duration-150 ${on ? "bg-paper shadow-[0_0_0_1px_var(--rule)]" : "hover:bg-paper/60"}`}
                  >
                    <span className={`block text-[13px] ${on ? "font-medium text-ink" : "text-ink-2"}`}>{s.label}</span>
                    <span className="block text-[11.5px] text-graphite">{s.hint}</span>
                  </button>
                </li>
              );
            })}
          </ul>
          <span className="num mt-auto px-3 text-[10.5px] text-faint">esc to close</span>
        </nav>

        <section className="relative min-w-0 flex-1 overflow-y-auto">
          <button
            onClick={p.onClose}
            aria-label="Close settings"
            className="press absolute right-4 top-4 z-10 grid size-7 place-items-center rounded-[3px] text-graphite hover:bg-well hover:text-ink"
          >
            <X size={15} />
          </button>
          <div key={section} className="swap-fade px-9 pb-10 pt-7">
            {section === "appearance" ? <Appearance {...p} /> : section === "models" ? <ModelsSettings /> : <AgentPrefs {...p} />}
          </div>
        </section>
      </div>
    </dialog>
  );
}

// ---------------------------------------------------------------------------

function Appearance({ settings: s, update }: Props) {
  const fill = ((s.size - SIZE_MIN) / (SIZE_MAX - SIZE_MIN)) * 100;
  const isDefault = JSON.stringify(s) === JSON.stringify(DEFAULTS);

  return (
    <>
      <Heading title="Appearance" sub="How the manuscript and the margin look. Changes apply as you make them." />

      <Preview />

      <Row label="Theme" hint="System follows your OS.">
        <div role="radiogroup" aria-label="Theme" className="grid grid-cols-3 gap-3">
          {(["light", "dark", "system"] as ThemePref[]).map((t) => (
            <button
              key={t}
              role="radio"
              aria-checked={s.theme === t}
              onClick={() => update({ theme: t })}
              className="choice group rounded-[5px] border border-rule p-1.5 text-left"
            >
              <ThemeSwatch kind={t} />
              <span className="mt-2 block px-1 pb-0.5 text-[12.5px] capitalize text-ink">{t}</span>
            </button>
          ))}
        </div>
      </Row>

      <Row label="Typeface" hint="Used for the manuscript and your notes to the editor.">
        <div role="radiogroup" aria-label="Typeface" className="grid grid-cols-3 gap-2.5 max-[860px]:grid-cols-2">
          {(Object.keys(FONTS) as FontId[]).map((id) => {
            const f = FONTS[id];
            return (
              <button
                key={id}
                role="radio"
                aria-checked={s.font === id}
                onClick={() => update({ font: id })}
                className="choice rounded-[5px] border border-rule px-3 pb-2.5 pt-2 text-left"
              >
                <span className="block text-[30px] leading-[1.15] text-ink" style={{ fontFamily: f.stack }}>
                  Aa<span className="text-[17px] italic text-graphite"> qg</span>
                </span>
                <span className="mt-1.5 block text-[12.5px] text-ink">{f.name}</span>
                <span className="label block text-[9.5px]">{f.kind}</span>
              </button>
            );
          })}
        </div>
      </Row>

      <Row label="Text size" hint="Of the manuscript.">
        <div className="flex items-center gap-4">
          <span className="font-serif text-[13px] text-graphite">A</span>
          <input
            type="range"
            className="range min-w-0 flex-1"
            min={SIZE_MIN}
            max={SIZE_MAX}
            step={0.5}
            value={s.size}
            aria-label="Text size"
            style={{ ["--fill" as string]: `${fill}%` }}
            onChange={(e) => update({ size: Number(e.target.value) })}
          />
          <span className="font-serif text-[20px] leading-none text-graphite">A</span>
          <span className="num w-14 text-right text-[12px] text-ink-2">{s.size.toFixed(1)} px</span>
        </div>
      </Row>

      <Row label="Line spacing">
        <Segmented<Leading>
          label="Line spacing"
          value={s.leading}
          onChange={(v) => update({ leading: v })}
          options={[
            { value: "compact", label: "Compact" },
            { value: "comfortable", label: "Comfortable" },
            { value: "airy", label: "Airy" },
          ]}
          className="text-[13px]"
        />
      </Row>

      <Row label="Line length" hint="Width of the text column.">
        <Segmented<Measure>
          label="Line length"
          value={s.measure}
          onChange={(v) => update({ measure: v })}
          options={[
            { value: "narrow", label: "Narrow" },
            { value: "medium", label: "Medium" },
            { value: "wide", label: "Wide" },
          ]}
          className="text-[13px]"
        />
      </Row>

      <Row label="Editor's ink" hint="Colour of Forge's marks, numbers and strike-throughs.">
        <div role="radiogroup" aria-label="Editor's ink" className="flex flex-wrap gap-5">
          {(Object.keys(INKS) as InkId[]).map((id) => {
            const ink = INKS[id];
            const on = s.ink === id;
            return (
              <button
                key={id}
                role="radio"
                aria-checked={on}
                onClick={() => update({ ink: id })}
                className="press group flex flex-col items-center gap-1.5"
              >
                <span
                  className="grid size-8 place-items-center rounded-full transition-shadow duration-150"
                  style={{
                    background: `oklch(${0.55 + ink.dl} ${ink.c} ${ink.h})`,
                    boxShadow: on ? "0 0 0 2px var(--paper), 0 0 0 3.5px var(--ink)" : "none",
                  }}
                >
                  <span className="check-dot size-1.5 rounded-full bg-paper" data-on={on} />
                </span>
                <span className={`text-[11.5px] ${on ? "text-ink" : "text-graphite group-hover:text-ink"}`}>{ink.name}</span>
              </button>
            );
          })}
        </div>
      </Row>

      <div className="flex justify-end border-t border-rule pt-5">
        <button onClick={() => update(DEFAULTS)} disabled={isDefault} className="tbtn text-[12.5px]">
          Reset to defaults
        </button>
      </div>
    </>
  );
}

/** A live specimen of the manuscript with one proof mark, in the current settings. */
function Preview() {
  return (
    <div className="mb-2 overflow-hidden rounded-[5px] border border-rule bg-paper">
      <div className="label border-b border-rule bg-margin px-4 py-2 text-[9.5px]">Preview</div>
      <div className="px-6 pb-6 pt-5" style={{ fontFamily: "var(--doc-font)", fontSize: "var(--doc-size)", lineHeight: "var(--doc-leading)" }}>
        <div className="mb-2 font-sans text-[0.62em] font-medium uppercase tracking-[0.12em] text-graphite">Launch strategy</div>
        <p className="text-ink-2" style={{ maxWidth: "min(100%, calc(var(--doc-measure) * 0.62))" }}>
          After the beta we will open a public waitlist. We expect word of mouth to be our{" "}
          <del className="diff-del">biggest</del> <ins className="diff-ins">strongest acquisition</ins>
          <sup className="cite">1</sup> channel, and <em>Product Hunt</em> to follow.
        </p>
      </div>
    </div>
  );
}

function ThemeSwatch({ kind }: { kind: ThemePref }) {
  const light = { paper: "oklch(0.986 0.003 75)", ink: "oklch(0.2 0.01 60)", rule: "oklch(0.86 0.006 75)", margin: "oklch(0.955 0.004 75)" };
  const dark = { paper: "oklch(0.19 0.006 70)", ink: "oklch(0.94 0.006 80)", rule: "oklch(0.36 0.007 70)", margin: "oklch(0.215 0.006 70)" };
  const page = (c: typeof light) => (
    <svg viewBox="0 0 120 72" className="block h-full w-full">
      <rect width="120" height="72" fill={c.paper} />
      <rect x="86" width="34" height="72" fill={c.margin} />
      <line x1="86" y1="0" x2="86" y2="72" stroke={c.rule} />
      <rect x="12" y="12" width="44" height="5" rx="1" fill={c.ink} />
      {[26, 34, 42].map((y, i) => (
        <rect key={y} x="12" y={y} width={i === 2 ? 40 : 62} height="2.5" rx="1" fill={c.rule} />
      ))}
      <rect x="12" y="50" width="22" height="2.5" rx="1" fill="var(--pencil)" opacity="0.85" />
      {[14, 22].map((y) => (
        <rect key={y} x="92" y={y} width="20" height="2" rx="1" fill={c.rule} />
      ))}
    </svg>
  );
  return (
    <span className="relative block aspect-[5/3] overflow-hidden rounded-[3px] border border-rule">
      {kind === "light" && page(light)}
      {kind === "dark" && page(dark)}
      {kind === "system" && (
        <>
          <span className="absolute inset-0">{page(light)}</span>
          <span className="absolute inset-0" style={{ clipPath: "polygon(100% 0, 100% 100%, 0 100%)" }}>
            {page(dark)}
          </span>
        </>
      )}
    </span>
  );
}

function AgentPrefs({ mode, setMode, webSearch, setWebSearch }: Props) {
  return (
    <>
      <Heading title="Editor" sub="How Forge works on your manuscript. The same controls sit under the note field." />
      <Row label="Edits" hint="Suggest leaves every change for your review.">
        <Segmented<EditMode>
          label="Edit mode"
          value={mode}
          onChange={setMode}
          options={[
            { value: "suggest", label: "Suggest" },
            { value: "auto", label: "Apply directly" },
          ]}
          className="text-[13px]"
        />
      </Row>
      {isDesktop() && (
        <Row label="Terminal command" hint="Open files from Terminal, like code for VS Code.">
          <CliSetting />
        </Row>
      )}
      <Row label="Web search" hint="Let Forge look up sources and cite them.">
        <button
          role="switch"
          aria-checked={webSearch}
          onClick={() => setWebSearch(!webSearch)}
          className="press flex items-center gap-2.5 text-[13px] text-ink-2"
        >
          <span className={`relative h-[18px] w-8 rounded-full transition-colors duration-200 ${webSearch ? "bg-ink" : "bg-rule-2"}`}>
            <span
              className="absolute left-[2px] top-[2px] size-[14px] rounded-full bg-paper transition-transform duration-200 ease-[var(--ease-out)]"
              style={{ transform: webSearch ? "translateX(14px)" : "none" }}
            />
          </span>
          {webSearch ? "On" : "Off"}
        </button>
      </Row>
    </>
  );
}
