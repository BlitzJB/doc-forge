"use client";

import type { ReactNode } from "react";
import type { BlockType, DiffRow } from "@/lib/diff";

const TAG: Record<BlockType, string> = {
  h1: "h1",
  h2: "h2",
  h3: "h3",
  p: "p",
  li: "p",
  oli: "p",
  quote: "blockquote",
  code: "pre",
  hr: "hr",
};

const GLYPH = { added: "+", removed: "−", modified: "±", same: "" } as const;

export function DiffDocument({ rows, onlyChanges }: { rows: DiffRow[]; onlyChanges?: boolean }) {
  const visible = onlyChanges ? rows.filter((r) => r.kind !== "same") : rows;
  if (!visible.length) return <p className="text-graphite">No differences.</p>;
  return (
    <>
      {visible.map((r, i) => (
        <DiffBlock key={i} row={r} />
      ))}
    </>
  );
}

function DiffBlock({ row }: { row: DiffRow }) {
  if (row.type === "hr") return <hr />;
  const Tag = TAG[row.type] as "p";
  const listy = row.type === "li" || row.type === "oli";
  const content =
    row.kind === "modified"
      ? row.parts.map((p, i) =>
          p.added ? (
            <ins key={i} className="diff-ins">{p.value}</ins>
          ) : p.removed ? (
            <del key={i} className="diff-del">{p.value}</del>
          ) : (
            <span key={i}>{p.value}</span>
          ),
        )
      : row.text;

  return (
    <div className={`diff-row fade-in ${row.kind === "same" ? "diff-same" : `diff-block-${row.kind}`}`}>
      {row.kind !== "same" && <span aria-hidden className="diff-gutter">{GLYPH[row.kind]}</span>}
      <Tag className={listy ? "diff-li" : undefined}>{content}</Tag>
    </div>
  );
}

/** A short excerpt around the first change, for previews. */
export function FirstChange({ rows, max = 150 }: { rows: DiffRow[]; max?: number }) {
  const row = rows.find((r) => r.kind !== "same");
  if (!row) return <span className="text-graphite">No text changes.</span>;
  if (row.kind === "added") return <ins className="diff-ins">{clip(row.text, max)}</ins>;
  if (row.kind === "removed") return <del className="diff-del">{clip(row.text, max)}</del>;
  if (row.kind !== "modified") return null;

  const out: ReactNode[] = [];
  let budget = max;
  const first = row.parts.findIndex((p) => p.added || p.removed);
  row.parts.forEach((p, i) => {
    if (budget <= 0 || i < first - 1) return;
    let text = p.value;
    if (i === first - 1 && !p.added && !p.removed && text.length > 32) text = "…" + text.slice(text.length - 32);
    if (text.length > budget) text = text.slice(0, budget) + "…";
    budget -= text.length;
    out.push(
      p.added ? <ins key={i} className="diff-ins">{text}</ins> : p.removed ? <del key={i} className="diff-del">{text}</del> : <span key={i}>{text}</span>,
    );
  });
  return <>{out}</>;
}

const clip = (t: string, n: number) => (t.length > n ? t.slice(0, n) + "…" : t);
