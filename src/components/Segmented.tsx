"use client";

import { useLayoutEffect, useRef, useState } from "react";

/** Text radio group whose vermilion underline slides to the chosen option. */
export function Segmented<T extends string>({
  value,
  onChange,
  options,
  label,
  className = "",
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string }[];
  label: string;
  className?: string;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const [line, setLine] = useState<{ x: number; w: number } | null>(null);

  useLayoutEffect(() => {
    const measure = () => {
      const el = wrap.current?.querySelector<HTMLElement>(`[data-value="${value}"]`);
      if (el) setLine({ x: el.offsetLeft, w: el.offsetWidth });
    };
    measure();
    // Re-measure when fonts load or the group becomes visible (e.g. inside a closed dialog).
    document.fonts?.ready.then(measure);
    const ro = new ResizeObserver(measure);
    if (wrap.current) ro.observe(wrap.current);
    return () => ro.disconnect();
  }, [value]);

  return (
    <div ref={wrap} role="radiogroup" aria-label={label} className={`relative flex items-center gap-3 ${className}`}>
      {options.map((o) => (
        <button
          key={o.value}
          data-value={o.value}
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={`press ${value === o.value ? "text-ink" : "text-graphite hover:text-ink"}`}
        >
          {o.label}
        </button>
      ))}
      {line && (
        <span aria-hidden className="slide-line" style={{ transform: `translateX(${line.x}px) scaleX(${line.w})` }} />
      )}
    </div>
  );
}
