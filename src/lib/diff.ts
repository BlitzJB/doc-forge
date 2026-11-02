import { diffArrays, diffWordsWithSpace, type ChangeObject } from "diff";

export type BlockType = "h1" | "h2" | "h3" | "p" | "li" | "oli" | "quote" | "code" | "hr";

export interface Block {
  type: BlockType;
  text: string;
}

export type DiffRow =
  | { kind: "same" | "added" | "removed"; type: BlockType; text: string }
  | { kind: "modified"; type: BlockType; parts: ChangeObject<string>[] };

export function htmlToBlocks(html: string): Block[] {
  if (typeof window === "undefined") return [];
  const dom = new DOMParser().parseFromString(html, "text/html");
  const blocks: Block[] = [];
  dom.body.querySelectorAll("h1,h2,h3,p,pre,hr").forEach((el) => {
    const tag = el.tagName.toLowerCase();
    const text = (el.textContent ?? "").replace(/\s+/g, " ").trim();
    if (tag === "hr") return blocks.push({ type: "hr", text: "" });
    if (tag === "pre") return blocks.push({ type: "code", text });
    if (tag === "p") {
      if (el.closest("li")) return blocks.push({ type: el.closest("ol") ? "oli" : "li", text });
      if (el.closest("blockquote")) return blocks.push({ type: "quote", text });
      if (!text) return;
      return blocks.push({ type: "p", text });
    }
    blocks.push({ type: tag as BlockType, text });
  });
  return blocks;
}

export function diffBlocks(oldBlocks: Block[], newBlocks: Block[]): DiffRow[] {
  const key = (b: Block) => `${b.type}\u0000${b.text}`;
  const changes = diffArrays(oldBlocks.map(key), newBlocks.map(key));
  const rows: DiffRow[] = [];
  const parse = (k: string): Block => {
    const [type, text] = k.split("\u0000");
    return { type: type as BlockType, text };
  };

  for (let i = 0; i < changes.length; i++) {
    const c = changes[i];
    if (c.removed && changes[i + 1]?.added) {
      const removed = c.value.map(parse);
      const added = changes[i + 1].value.map(parse);
      const pairs = Math.min(removed.length, added.length);
      for (let j = 0; j < pairs; j++) {
        rows.push({
          kind: "modified",
          type: added[j].type,
          parts: coalesce(diffWordsWithSpace(removed[j].text, added[j].text)),
        });
      }
      removed.slice(pairs).forEach((b) => rows.push({ kind: "removed", ...b }));
      added.slice(pairs).forEach((b) => rows.push({ kind: "added", ...b }));
      i++;
      continue;
    }
    const kind = c.added ? "added" : c.removed ? "removed" : "same";
    c.value.map(parse).forEach((b) => rows.push({ kind, ...b }));
  }
  return rows;
}

const words = (s: string) => s.split(/\s+/).filter(Boolean).length;

export function diffStats(rows: DiffRow[]) {
  let added = 0;
  let removed = 0;
  for (const r of rows) {
    if (r.kind === "added") added += words(r.text);
    else if (r.kind === "removed") removed += words(r.text);
    else if (r.kind === "modified")
      for (const p of r.parts) {
        if (p.added) added += words(p.value);
        else if (p.removed) removed += words(p.value);
      }
  }
  return { added, removed };
}

/**
 * Word diffs of rewritten sentences interleave badly ("~~have~~ are ~~a~~ drowning").
 * Merge change runs separated by whitespace or up to two short words into one
 * deletion + one insertion, so rewrites read as phrases.
 */
function coalesce(parts: ChangeObject<string>[]): ChangeObject<string>[] {
  const out: ChangeObject<string>[] = [];
  let del = "";
  let ins = "";
  let open = false;
  const flush = () => {
    if (del) out.push({ value: del, removed: true, added: false, count: 1 });
    if (ins) out.push({ value: ins, added: true, removed: false, count: 1 });
    del = ins = "";
    open = false;
  };
  parts.forEach((part, i) => {
    if (part.added || part.removed) {
      if (part.added) ins += part.value;
      else del += part.value;
      open = true;
      return;
    }
    const nextIsChange = parts[i + 1] && (parts[i + 1].added || parts[i + 1].removed);
    // Bridge over up to two short words so a rewritten clause reads as one phrase.
    const bridge = part.value.length <= 14 && part.value.trim().split(/\s+/).length <= 2 && !/[.;:!?]/.test(part.value);
    if (open && nextIsChange && bridge) {
      del += part.value;
      ins += part.value;
      return;
    }
    flush();
    out.push(part);
  });
  flush();
  return out;
}
