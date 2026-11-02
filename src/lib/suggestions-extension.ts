import { Extension } from "@tiptap/core";
import type { Node as PMNode } from "@tiptap/pm/model";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";

export interface SuggestionItem {
  id: string;
  /** Reference number shared with the note in the margin. */
  n: number;
  /** When the mark was proposed; only fresh marks play their entrance. */
  bornAt?: number;
  find: string;
  replace: string;
  /** "insert": `replace` holds new paragraphs to add after the block containing `find`. */
  kind?: "replace" | "insert";
}

export interface SuggestionHandlers {
  accept: (id: string) => void;
  reject: (id: string) => void;
  focus: (id: string) => void;
}

interface Flash {
  id: string;
  from: number;
  to: number;
}

interface PluginState {
  items: SuggestionItem[];
  activeId: string | null;
  leaving: string[];
  flashes: Flash[];
  deco: DecorationSet;
}

export interface SuggestionMeta {
  items?: SuggestionItem[];
  activeId?: string | null;
  /** Marks fading out before they are removed. */
  leaving?: string[];
  /** Briefly tint freshly accepted text. */
  flash?: Flash;
  unflash?: string;
}

export const suggestionsKey = new PluginKey<PluginState>("suggestions");

/** Find `needle` inside a single textblock; returns a document range or null. */
export function findTextRange(doc: PMNode, needle: string): { from: number; to: number } | null {
  if (!needle) return null;
  let found: { from: number; to: number } | null = null;
  doc.descendants((node, pos) => {
    if (found) return false;
    if (!node.isTextblock) return true;
    let text = "";
    const map: number[] = [];
    node.forEach((child, offset) => {
      if (child.isText && child.text) {
        for (let i = 0; i < child.text.length; i++) map.push(pos + 1 + offset + i);
        text += child.text;
      } else {
        map.push(pos + 1 + offset);
        text += "￼";
      }
    });
    const idx = text.indexOf(needle);
    if (idx >= 0) found = { from: map[idx], to: map[idx + needle.length - 1] + 1 };
    return false;
  });
  return found;
}

/**
 * Where an inserted paragraph goes: just after the top-level block that contains `anchor`
 * (so additions after a list item land after the whole list).
 */
export function insertionPoint(doc: PMNode, anchor: string): number | null {
  const range = findTextRange(doc, anchor);
  if (!range) return null;
  const $pos = doc.resolve(range.from);
  return $pos.depth >= 1 ? $pos.after(1) : null;
}

/**
 * Trim the common prefix/suffix (snapped to word boundaries) so that only the
 * part that actually changes is shown as a deletion + insertion.
 */
export function minimalChange(find: string, replace: string) {
  let p = 0;
  while (p < find.length && p < replace.length && find[p] === replace[p]) p++;
  // Only snap back to a word boundary when the mismatch lands mid-word.
  if (p < find.length && p < replace.length) {
    const sp = find.lastIndexOf(" ", p - 1);
    p = sp >= 0 ? sp + 1 : 0;
  }
  let s = 0;
  while (
    s < find.length - p &&
    s < replace.length - p &&
    find[find.length - 1 - s] === replace[replace.length - 1 - s]
  )
    s++;
  if (s > 0 && s < find.length - p && s < replace.length - p) {
    const tail = find.slice(find.length - s);
    const sp = tail.indexOf(" ");
    s = sp >= 0 ? s - sp : 0;
  }
  return {
    prefix: p,
    suffix: s,
    deleted: find.slice(p, find.length - s),
    inserted: replace.slice(p, replace.length - s),
  };
}

/** "## Pricing" → a level-2 heading; anything else is a paragraph. */
export function insertedBlock(text: string): { level: 0 | 1 | 2 | 3; text: string } {
  const m = /^(#{1,3})\s+(.+)$/.exec(text.trim());
  return m ? { level: m[1].length as 1 | 2 | 3, text: m[2].trim() } : { level: 0, text: text.trim() };
}

/** A proposed new paragraph (or several), shown as a block in the insertion ink. */
function makeInsertWidget(item: SuggestionItem, index: number, handlers: () => SuggestionHandlers | null) {
  const wrap = document.createElement("div");
  wrap.className = "sg-widget sg-insert";
  wrap.dataset.sid = item.id;
  wrap.style.setProperty("--i", String(index));
  wrap.contentEditable = "false";
  const paras = item.replace.split(/\n{2,}/).filter((p) => p.trim());
  paras.forEach((raw, i) => {
    const { level, text } = insertedBlock(raw);
    const p = document.createElement(level ? `h${level}` : "p");
    p.className = "sg-ins-block";
    p.textContent = text;
    if (i === paras.length - 1) {
      const n = document.createElement("sup");
      n.className = "sg-n";
      n.textContent = String(item.n);
      p.appendChild(n);
    }
    wrap.appendChild(p);
  });
  wrap.appendChild(makeActions(item, handlers));
  wrap.addEventListener("mouseenter", () => handlers()?.focus(item.id));
  return wrap;
}

function makeActions(item: SuggestionItem, handlers: () => SuggestionHandlers | null) {
  const actions = document.createElement("span");
  actions.className = "sg-actions";
  const mk = (cls: string, label: string, fn: () => void) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = cls;
    b.textContent = label;
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      fn();
    });
    return b;
  };
  actions.appendChild(mk("sg-btn sg-accept", "Accept", () => handlers()?.accept(item.id)));
  actions.appendChild(mk("sg-btn sg-reject", "Decline", () => handlers()?.reject(item.id)));
  return actions;
}

function makeWidget(
  item: SuggestionItem,
  inserted: string,
  index: number,
  handlers: () => SuggestionHandlers | null,
) {
  const wrap = document.createElement("span");
  wrap.className = "sg-widget";
  wrap.dataset.sid = item.id;
  wrap.style.setProperty("--i", String(index));
  wrap.contentEditable = "false";

  if (inserted) {
    const ins = document.createElement("span");
    ins.className = "sg-ins";
    ins.textContent = inserted;
    wrap.appendChild(ins);
  }

  const n = document.createElement("sup");
  n.className = "sg-n";
  n.textContent = String(item.n);
  wrap.appendChild(n);

  wrap.appendChild(makeActions(item, handlers));
  // Widgets stop PM events, so they announce hover themselves.
  wrap.addEventListener("mouseenter", () => handlers()?.focus(item.id));
  return wrap;
}

export const SuggestionsExtension = Extension.create<{
  getHandlers: () => SuggestionHandlers | null;
}>({
  name: "suggestions",

  addOptions() {
    return { getHandlers: () => null };
  },

  addProseMirrorPlugins() {
    const handlers = this.options.getHandlers;

    const build = (doc: PMNode, items: SuggestionItem[], activeId: string | null, leaving: string[], flashes: Flash[]) => {
      const decos: Decoration[] = [];
      items.forEach((item, i) => {
        if (item.kind === "insert") {
          const at = insertionPoint(doc, item.find);
          if (at === null) return;
          decos.push(
            Decoration.widget(at, () => makeInsertWidget(item, i, handlers), {
              side: -1,
              key: `${item.id}:${item.n}:insert`,
              ignoreSelection: true,
              stopEvent: () => true,
            }),
          );
          return;
        }
        const range = findTextRange(doc, item.find);
        if (!range) return;
        const { prefix, suffix, deleted, inserted } = minimalChange(item.find, item.replace);
        const from = range.from + prefix;
        const to = range.to - suffix;
        // ProseMirror may re-create this element when its class changes (hover, leaving),
        // so the strike only animates while the mark is fresh. Later rebuilds render it drawn.
        const fresh = item.bornAt !== undefined && Date.now() - item.bornAt < 700;
        const cls = [
          "sg-del",
          fresh && "sg-fresh",
          item.id === activeId && "sg-active",
          leaving.includes(item.id) && "sg-leaving",
        ]
          .filter(Boolean)
          .join(" ");
        if (deleted && to > from) {
          decos.push(Decoration.inline(from, to, { class: cls, "data-sid": item.id, style: `--i:${i}` }));
        }
        // Stable key: the widget DOM survives hover/active changes, so its entrance plays once.
        decos.push(
          Decoration.widget(to, () => makeWidget(item, inserted, i, handlers), {
            side: 1,
            key: `${item.id}:${item.n}`,
            ignoreSelection: true,
            stopEvent: () => true,
          }),
        );
      });
      for (const f of flashes) {
        if (f.to > f.from) decos.push(Decoration.inline(f.from, f.to, { class: "sg-settle" }));
      }
      return DecorationSet.create(doc, decos);
    };

    // Widgets are outside ProseMirror's mutation tracking, so their state is toggled in place.
    const syncWidgets = (root: HTMLElement, st: PluginState) => {
      root.querySelectorAll<HTMLElement>(".sg-widget").forEach((w) => {
        w.classList.toggle("sg-active", w.dataset.sid === st.activeId);
        w.classList.toggle("sg-leaving", st.leaving.includes(w.dataset.sid ?? ""));
      });
    };

    return [
      new Plugin<PluginState>({
        key: suggestionsKey,
        state: {
          init: () => ({ items: [], activeId: null, leaving: [], flashes: [], deco: DecorationSet.empty }),
          apply(tr, prev) {
            const meta = tr.getMeta(suggestionsKey) as SuggestionMeta | undefined;
            let flashes = tr.docChanged
              ? prev.flashes.map((f) => ({ ...f, from: tr.mapping.map(f.from, 1), to: tr.mapping.map(f.to, -1) }))
              : prev.flashes;
            if (!meta && !tr.docChanged) return prev;
            if (meta?.flash) flashes = [...flashes, meta.flash];
            if (meta?.unflash) flashes = flashes.filter((f) => f.id !== meta.unflash);
            const items = meta?.items ?? prev.items;
            const activeId = meta && meta.activeId !== undefined ? meta.activeId : prev.activeId;
            const leaving = meta?.leaving ?? prev.leaving;
            return { items, activeId, leaving, flashes, deco: build(tr.doc, items, activeId, leaving, flashes) };
          },
        },
        view: (view) => ({
          update(v) {
            const st = suggestionsKey.getState(v.state);
            if (st) syncWidgets(v.dom, st);
          },
          destroy() {
            void view;
          },
        }),
        props: {
          decorations(state) {
            return suggestionsKey.getState(state)?.deco;
          },
          handleDOMEvents: {
            mouseover(_view, event) {
              const el = (event.target as HTMLElement).closest?.("[data-sid]") as HTMLElement | null;
              if (el?.dataset.sid) handlers()?.focus(el.dataset.sid);
              return false;
            },
          },
        },
      }),
    ];
  },
});
