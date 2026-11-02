"use client";

/**
 * Activity marks in the editor's ink, instead of a generic spinner.
 * Scribe: a nib writing a short cursive stroke (thinking, writing).
 * Proofread: an underline sweeping through lines of text (reading, searching).
 */

const STROKE = "M2 8 C4 3, 6 3, 7 7 S10 11, 11.5 6 S14.5 2, 16 6.5 S19.5 10, 21 5.5 S23.5 3, 25 5";

export function Scribe({ className = "" }: { className?: string }) {
  return (
    <span aria-hidden className={`scribe ${className}`}>
      <svg width="27" height="12" viewBox="0 0 27 12">
        <path d={STROKE} pathLength={1} />
      </svg>
      <span className="scribe-nib" style={{ offsetPath: `path("${STROKE}")` }} />
    </span>
  );
}

export function Proofread({ className = "" }: { className?: string }) {
  return (
    <span aria-hidden className={`proofread ${className}`}>
      {[0, 1, 2].map((i) => (
        <span key={i} className="proofread-line" style={{ width: i === 2 ? 10 : 16 }}>
          <span className="proofread-mark" style={{ animationDelay: `${i * 0.22}s` }} />
        </span>
      ))}
    </span>
  );
}

export function Activity({ kind, className }: { kind: "considering" | "writing" | "reading" | "searching"; className?: string }) {
  return kind === "reading" || kind === "searching" ? <Proofread className={className} /> : <Scribe className={className} />;
}
