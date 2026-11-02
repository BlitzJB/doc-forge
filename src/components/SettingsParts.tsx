"use client";

import type { ReactNode } from "react";

// ---------------------------------------------------------------------------
// Layout pieces shared by the settings sections.

export function Heading({ title, sub }: { title: string; sub: string }) {
  return (
    <header className="mb-6 pr-10">
      <h2 className="font-serif text-[24px] leading-tight text-ink">{title}</h2>
      <p className="mt-1 text-[13px] text-graphite">{sub}</p>
    </header>
  );
}

export function Row({ label, hint, children, stack }: { label: string; hint?: string; children: ReactNode; stack?: boolean }) {
  return (
    <div className={`flex gap-x-8 gap-y-3 border-t border-rule py-5 ${stack ? "flex-col" : "items-start max-[720px]:flex-col"}`}>
      <div className="w-[150px] shrink-0">
        <div className="text-[13px] font-medium text-ink">{label}</div>
        {hint && <div className="mt-0.5 text-[12px] leading-snug text-graphite">{hint}</div>}
      </div>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
