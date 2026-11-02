/*
 * Doc Forge landing page. No dependencies.
 * Every scripted scene runs only while it's on screen, and settles to its final
 * state when reduced motion is requested.
 */
(() => {
  "use strict";

  const $ = (s, el = document) => el.querySelector(s);
  const $$ = (s, el = document) => [...el.querySelectorAll(s)];
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const sleep = (ms) => new Promise((r) => setTimeout(r, reduced ? 0 : ms));
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

  /** Resolves while `el` is on screen; waits (without busy-looping) while it isn't. */
  function visibility(el, threshold = 0.25) {
    let visible = false;
    let waiters = [];
    new IntersectionObserver(
      ([e]) => {
        visible = e.isIntersecting;
        if (visible) {
          waiters.forEach((w) => w());
          waiters = [];
        }
      },
      { threshold },
    ).observe(el);
    return () => (visible ? Promise.resolve() : new Promise((r) => waiters.push(r)));
  }

  // The app's activity marks.
  const STROKE = "M2 8 C4 3, 6 3, 7 7 S10 11, 11.5 6 S14.5 2, 16 6.5 S19.5 10, 21 5.5 S23.5 3, 25 5";
  const scribe = () =>
    `<span class="scribe" aria-hidden="true"><svg width="27" height="12" viewBox="0 0 27 12"><path d="${STROKE}" pathLength="1"/></svg><span class="scribe-nib" style="offset-path: path('${STROKE}')"></span></span>`;
  const proofread = () => `<span class="proofread" aria-hidden="true"><i></i><i></i><i></i></span>`;

  /* ------------------------------------------------------------ Nav */
  const nav = $("[data-nav]");
  const onScroll = () => nav.toggleAttribute("data-scrolled", scrollY > 12);
  addEventListener("scroll", onScroll, { passive: true });
  onScroll();
  /* ------------------------------------------------------------ Reveals */
  const reveal = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) {
          e.target.setAttribute("data-in", "");
          reveal.unobserve(e.target);
        }
      }
    },
    { threshold: 0.35 },
  );
  $$("[data-reveal], [data-begin]").forEach((el) => reveal.observe(el));

  /* ------------------------------------------------------------ Hero: the headline proofreads itself */
  const proof = $("[data-proof]");
  (async () => {
    await sleep(reduced ? 0 : 750);
    proof.dataset.step = "1"; // strike
    await sleep(620);
    proof.dataset.step = "2"; // ink in
    await sleep(820);
    proof.dataset.step = "3"; // circle and number
  })();

  /* ------------------------------------------------------------ Hero: the app, replaying a real exchange */
  const stage = $("[data-stage]");
  if (stage) {
    const ui = {
      title: $("[data-title]", stage),
      state: $("[data-state]", stage),
      feed: $("[data-feed]", stage),
      input: $("[data-input]", stage),
      send: $("[data-send]", stage),
      ver: $("[data-ver]", stage),
      railNew: $("[data-rail-new]", stage),
      cuts: $$("[data-cut]", stage),
    };
    // Each loop starts from the manuscript exactly as written.
    const ms = $(".ms", stage);
    const pristine = ms.innerHTML;
    const ins = { 3: "slowly" };
    const prompt = "Tighten the opening, but keep my voice.";
    const reply = "Three hedges were softening a plain story. I cut them and left the rest as you wrote it.";
    const waitVisible = visibility(stage, 0.3);

    const setState = (label, mark) => {
      ui.state.innerHTML = `${mark ? mark() : ""}<span>${label}</span>`;
    };
    const add = (html) => {
      const d = document.createElement("div");
      d.className = "f-item";
      d.innerHTML = html;
      ui.feed.append(d);
      return d;
    };

    const reset = () => {
      ui.title.textContent = "Editor";
      setState("idle");
      ui.feed.innerHTML = "";
      ui.input.innerHTML = `<span class="ph">Write a note to the editor</span>`;
      ui.send.removeAttribute("data-ready");
      ui.ver.textContent = "v6";
      ui.ver.removeAttribute("data-fresh");
      ui.railNew.removeAttribute("data-in");
      ms.innerHTML = pristine;
      ui.cuts = $$("[data-cut]", stage);
    };

    const finalState = () => {
      reset();
      ui.title.textContent = "Tightening the opening";
      add(`<div class="f-you">${esc(prompt)}</div>`);
      add(`<div class="f-tool"><span class="lab">Read</span>The manuscript <span class="sum">· 74 words</span></div>`);
      add(`<div class="f-text">${esc(reply)}</div>`);
      add(`<div class="f-accepted">Accepted 3 · saved as v7</div>`);
      ui.cuts.forEach((c) => {
        const n = c.dataset.cut;
        if (ins[n]) c.insertAdjacentText("afterend", ins[n]);
        c.setAttribute("data-gone", "");
      });
      ui.railNew.setAttribute("data-in", "");
      ui.ver.textContent = "v7";
    };

    async function play() {
      reset();
      await sleep(900);
      await waitVisible();
      // The author types a note.
      ui.input.innerHTML = `<span data-t></span><span class="caret-i"></span>`;
      const t = $("[data-t]", ui.input);
      for (const ch of prompt) {
        t.textContent += ch;
        await sleep(28 + Math.random() * 46);
      }
      ui.send.setAttribute("data-ready", "");
      await sleep(380);
      ui.send.setAttribute("data-press", "");
      await sleep(140);
      ui.send.removeAttribute("data-press");
      ui.send.removeAttribute("data-ready");
      ui.input.innerHTML = `<span class="ph">Write a note to the editor</span>`;
      add(`<div class="f-you">${esc(prompt)}</div>`);

      // Forge considers, then reads.
      setState("considering", scribe);
      await sleep(1300);
      await waitVisible();
      setState("reading", proofread);
      const read = add(`<div class="f-tool"><span class="lab">Read</span>The manuscript ${proofread()}</div>`);
      await sleep(1100);
      read.innerHTML = `<div class="f-tool"><span class="lab">Read</span>The manuscript <span class="sum">· 74 words</span></div>`;

      // Marks land in the manuscript, staggered.
      setState("writing", scribe);
      ui.cuts.forEach((c, i) => {
        const n = c.dataset.cut;
        c.style.setProperty("--i", i);
        c.setAttribute("data-marked", "");
        let html = "";
        if (ins[n]) html += `<span class="ins-w" style="--i:${i}">${ins[n]}</span>`;
        html += `<sup class="n-w" style="--i:${i}">${n}</sup>`;
        c.insertAdjacentHTML("afterend", html);
      });
      await sleep(500);

      // The reply streams in.
      const text = add(`<div class="f-text"></div>`).firstChild;
      for (const w of reply.split(" ")) {
        text.textContent += (text.textContent ? " " : "") + w;
        await sleep(55 + Math.random() * 50);
      }
      const marks = add(`<div class="f-actions"><span class="f-accept">Accept all 3</span><span>Decline</span></div>`);
      setState("idle");
      await sleep(500);
      ui.title.innerHTML = `<span class="f-item" style="display:inline-block">Tightening the opening</span>`;

      // The author accepts.
      await sleep(1700);
      await waitVisible();
      const btn = $(".f-accept", marks);
      btn.setAttribute("data-press", "");
      await sleep(160);
      btn.removeAttribute("data-press");
      ui.cuts.forEach((c) => {
        c.setAttribute("data-gone", "");
        let next = c.nextSibling;
        while (next && next.nodeType === 1 && next.matches(".ins-w, .n-w")) {
          const after = next.nextSibling;
          if (next.matches(".ins-w")) {
            const s = document.createElement("span");
            s.className = "settle";
            s.textContent = next.textContent;
            next.replaceWith(s);
          } else next.remove();
          next = after;
        }
      });
      marks.innerHTML = `<div class="f-actions f-item"><span class="f-accepted">Accepted 3 · saved as v7</span></div>`;
      await sleep(350);
      ui.railNew.setAttribute("data-in", "");
      ui.ver.textContent = "v7";
      ui.ver.setAttribute("data-fresh", "");
      await sleep(5200);
    }

    (async () => {
      if (reduced) return finalState();
      for (;;) {
        await play();
        stage.querySelector(".win").animate([{ opacity: 1 }, { opacity: 0.0 }], { duration: 380, easing: "ease-in", fill: "forwards" });
        await sleep(400);
        reset();
        stage.querySelector(".win").animate([{ opacity: 0 }, { opacity: 1 }], { duration: 520, easing: "cubic-bezier(.23,1,.32,1)", fill: "forwards" });
      }
    })();
  }

  /* ------------------------------------------------------------ Try it: real marks to accept or decline */
  const tryEl = $("[data-try]");
  if (tryEl) {
    const marks = $$(".mk", tryEl);
    const count = $("[data-try-count]", tryEl);
    const acceptAll = $("[data-accept-all]", tryEl);
    const again = $("[data-try-reset]", tryEl);
    const notes = new Map();

    const update = () => {
      const left = marks.filter((m) => !m.dataset.state).length;
      count.textContent = left ? `${left} mark${left === 1 ? "" : "s"}` : "Done";
      acceptAll.hidden = left === 0;
      again.hidden = left !== 0;
    };
    const decide = (m, state) => {
      if (m.dataset.state) return;
      m.dataset.state = state;
      m.removeAttribute("tabindex");
      m.removeAttribute("data-active");
      notes.get(m.dataset.mk)?.setAttribute("data-done", "");
      notes.get(m.dataset.mk)?.removeAttribute("data-active");
      update();
    };
    marks.forEach((m) => {
      m.tabIndex = 0;
      m.setAttribute("role", "group");
      m.setAttribute("aria-label", `Mark ${m.dataset.mk}: replace "${$("del", m).textContent}" with "${$("ins", m).textContent}"`);
      const bar = document.createElement("span");
      bar.className = "mk-bar";
      bar.innerHTML = `<button type="button" data-a="accepted">Accept</button><button type="button" data-a="declined">Decline</button>`;
      m.append(bar);
      bar.addEventListener("click", (e) => {
        const b = e.target.closest("button");
        if (b) decide(m, b.dataset.a);
      });
      // Keep the toolbar inside the sheet when a mark sits near its right edge.
      const fit = () => {
        bar.style.setProperty("--shift", "0px");
        const over = bar.getBoundingClientRect().right - (tryEl.getBoundingClientRect().right - 12);
        if (over > 0) bar.style.setProperty("--shift", `${-Math.ceil(over)}px`);
      };
      const on = () => !m.dataset.state && (fit(), m.setAttribute("data-active", ""), notes.get(m.dataset.mk)?.setAttribute("data-active", ""));
      const off = () => (m.removeAttribute("data-active"), notes.get(m.dataset.mk)?.removeAttribute("data-active"));
      m.addEventListener("mouseenter", on);
      m.addEventListener("mouseleave", off);
      m.addEventListener("focusin", on);
      m.addEventListener("focusout", (e) => !m.contains(e.relatedTarget) && off());
      m.addEventListener("keydown", (e) => {
        if (e.target !== m) return;
        if (e.key === "Enter") decide(m, "accepted");
        if (e.key === "Backspace" || e.key === "Delete") decide(m, "declined");
      });
      // Hovering a note lights its mark.
      const note = notes.get(m.dataset.mk);
      note?.addEventListener("mouseenter", () => !m.dataset.state && m.setAttribute("data-active", ""));
      note?.addEventListener("mouseleave", () => m.removeAttribute("data-active"));
    });
    acceptAll.addEventListener("click", async () => {
      for (const m of marks) {
        decide(m, "accepted");
        await sleep(110);
      }
    });
    again.addEventListener("click", () => {
      marks.forEach((m) => {
        delete m.dataset.state;
        m.tabIndex = 0;
      });
      notes.forEach((n) => n.removeAttribute("data-done"));
      update();
      marks[0].focus({ preventScroll: true });
    });
    update();
  }

  /* ------------------------------------------------------------ History: a timeline you can scrub */
  const hist = $("[data-hist]");
  if (hist) {
    const DAY = 86_400_000;
    const now = Date.now();
    const at = (daysAgo, h, m) => {
      const d = new Date(now - daysAgo * DAY);
      d.setHours(h, m, 0, 0);
      return d.getTime();
    };
    const B = {
      title: { id: "t", k: "h4", x: "Northwind: launch plan" },
      intro1: { id: "i", k: "p", x: "Northwind is a lightweight analytics tool for small shops. This plan covers who we launch to, how, and how we will know it worked." },
      intro2: { id: "i", k: "p", x: "Northwind is analytics for small shops. This plan covers who we launch to, how, and how we will know it worked." },
      priceH: { id: "ph", k: "h5", x: "Pricing" },
      price: { id: "pp", k: "p", x: "Pricing starts at $29 a month for three seats, with a 14-day trial and no card required." },
      launchH: { id: "lh", k: "h5", x: "Launch" },
      launch1: { id: "l", k: "p", x: "We will basically start with a private beta for around 50 merchants in January, and then we think we will open a public waitlist." },
      launch2: { id: "l", k: "p", x: "We start with a private beta for 50 merchants in January, then open a public waitlist in February." },
      metricsH: { id: "mh", k: "h5", x: "Success" },
      metrics: { id: "m", k: "p", x: "500 paying shops by the end of March, with monthly churn under 4.5%." },
    };
    const versions = [
      { at: at(6, 10, 2), who: "you", label: "First draft", blocks: [B.title, B.intro1, B.priceH, B.price, B.launchH, B.launch1] },
      { at: at(6, 10, 21), who: "you", label: "Tightened the intro", blocks: [B.title, B.intro2, B.priceH, B.price, B.launchH, B.launch1] },
      { at: at(4, 16, 40), who: "you", label: "Cut the pricing section", blocks: [B.title, B.intro2, B.launchH, B.launch1] },
      { at: at(4, 16, 52), who: "agent", label: "Forge: 3 edits accepted", blocks: [B.title, B.intro2, B.launchH, B.launch2] },
      { at: at(2, 9, 15), who: "you", label: "Added success metrics", blocks: [B.title, B.intro2, B.launchH, B.launch2, B.metricsH, B.metrics] },
      { at: at(0, 11, 53), who: "agent", label: "Forge: restored Pricing", blocks: [B.title, B.intro2, B.priceH, B.price, B.launchH, B.launch2, B.metricsH, B.metrics] },
    ];
    const rail = $("[data-hist-rail]", hist);
    const meta = $("[data-hist-meta]", hist);
    const textEl = $("[data-hist-text]", hist);

    const dayName = (ts) => {
      const days = Math.round((new Date(now).setHours(0, 0, 0, 0) - new Date(ts).setHours(0, 0, 0, 0)) / DAY);
      if (days === 0) return "Today";
      if (days === 1) return "Yesterday";
      return new Date(ts).toLocaleDateString([], { weekday: "long" });
    };
    const time = (ts) => new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

    // Spacing by time, as in the app: close for minutes apart, far across days.
    const gaps = versions.map((v, i) => {
      if (i === 0) return 0;
      const mins = Math.max(1, (v.at - versions[i - 1].at) / 60000);
      const sameDay = new Date(v.at).toDateString() === new Date(versions[i - 1].at).toDateString();
      return Math.min(30, 6 + Math.log10(mins) * 7) + (sameDay ? 0 : 30);
    });
    const total = gaps.reduce((a, b) => a + b, 0);
    let y = 0;
    const tops = gaps.map((g) => (y += g) / total);

    // Word-level diff for a changed paragraph (LCS; paragraphs are short).
    const wordDiff = (a, b) => {
      const x = a.split(/(\s+)/), z = b.split(/(\s+)/);
      const n = x.length, m = z.length;
      const L = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
      for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) L[i][j] = x[i] === z[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
      let i = 0, j = 0, out = "";
      while (i < n && j < m) {
        if (x[i] === z[j]) { out += esc(x[i]); i++; j++; }
        else if (L[i + 1][j] >= L[i][j + 1]) { out += /\S/.test(x[i]) ? `<span class="gone">${esc(x[i])}</span>` : esc(x[i]); i++; }
        else { out += /\S/.test(z[j]) ? `<span class="new">${esc(z[j])}</span>` : esc(z[j]); j++; }
      }
      while (i < n) { out += /\S/.test(x[i]) ? `<span class="gone">${esc(x[i])}</span>` : ""; i++; }
      while (j < m) { out += /\S/.test(z[j]) ? `<span class="new">${esc(z[j])}</span>` : esc(z[j]); j++; }
      return out.replace(/<\/span>(\s+)<span class="(gone|new)">/g, "$1").replace(/(<span class="(gone|new)">[^<]*)<\/span>(\s+)<span class="\2">/g, "$1$3");
    };

    const render = (idx) => {
      const v = versions[idx];
      const prev = versions[idx - 1];
      const before = prev ? new Map(prev.blocks.map((b) => [b.id, b])) : null;
      const after = new Set(v.blocks.map((b) => b.id));
      let html = "";
      // Blocks removed since the previous version show where they were, struck.
      const rows = [];
      if (prev) {
        let j = 0;
        for (const b of v.blocks) {
          while (j < prev.blocks.length && !after.has(prev.blocks[j].id) && prev.blocks[j].id !== b.id) rows.push({ b: prev.blocks[j++], kind: "gone" });
          if (j < prev.blocks.length && prev.blocks[j].id === b.id) j++;
          rows.push({ b, kind: !before.has(b.id) ? "new" : before.get(b.id).x === b.x ? "same" : "changed" });
        }
        while (j < prev.blocks.length) {
          if (!after.has(prev.blocks[j].id)) rows.push({ b: prev.blocks[j], kind: "gone" });
          j++;
        }
      } else v.blocks.forEach((b) => rows.push({ b, kind: "first" }));
      for (const { b, kind } of rows) {
        const inner =
          kind === "changed" ? wordDiff(before.get(b.id).x, b.x)
          : kind === "new" ? `<span class="new">${esc(b.x)}</span>`
          : kind === "gone" ? `<span class="gone">${esc(b.x)}</span>`
          : esc(b.x);
        html += `<${b.k} class="${kind === "same" ? "same" : ""}">${inner}</${b.k}>`;
      }
      textEl.innerHTML = html;
      meta.innerHTML = `<b>v${idx + 1}</b><span>${dayName(v.at)} ${time(v.at)}</span><span class="${v.who === "agent" ? "agent" : ""}">${esc(v.label)}</span>`;
      dots.forEach((d, i) => d.setAttribute("aria-selected", String(i === idx)));
      current = idx;
    };

    let current = versions.length - 1;
    let touched = false;
    const dots = versions.map((v, i) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "hist-dot";
      b.setAttribute("role", "option");
      b.setAttribute("aria-label", `v${i + 1}, ${dayName(v.at)} ${time(v.at)}: ${v.label}`);
      if (v.who === "agent") b.setAttribute("data-agent", "");
      // Newest at the top, as in the app.
      b.style.top = `calc(30px + (100% - 60px) * ${1 - tops[i]})`;
      b.addEventListener("mouseenter", () => ((touched = true), render(i)));
      b.addEventListener("focus", () => ((touched = true), render(i)));
      b.addEventListener("click", () => ((touched = true), render(i)));
      rail.append(b);
      return b;
    });
    rail.addEventListener("keydown", (e) => {
      const d = e.key === "ArrowUp" ? 1 : e.key === "ArrowDown" ? -1 : 0;
      if (!d) return;
      e.preventDefault();
      const n = Math.max(0, Math.min(versions.length - 1, current + d));
      dots[n].focus();
    });
    render(current);

    // Once, when it comes into view: rewind to v1 and play forward.
    const seen = new IntersectionObserver(async ([e]) => {
      if (!e.isIntersecting) return;
      seen.disconnect();
      if (reduced) return;
      await sleep(300);
      for (let i = 0; i < versions.length; i++) {
        if (touched) return;
        render(i);
        await sleep(i === 0 ? 1400 : 1700);
      }
    }, { threshold: 0.45 });
    seen.observe(hist);
  }

  /* ------------------------------------------------------------ df: the terminal scene */
  const term = $("[data-term]");
  if (term) {
    const body = $("[data-term-body]", term);
    const pop = $("[data-pop]", term);
    const mini = $("[data-mini]", term);
    const waitVisible = visibility(term, 0.35);
    const PS = `<span class="pr">~/novel</span> <span class="dim">%</span> `;
    let lines = [];
    const paint = (live = "") => {
      body.innerHTML = [...lines.slice(-12), live].join("\n");
    };
    const typeCmd = async (cmd) => {
      let typed = "";
      paint(`${PS}<span class="cur"></span>`);
      await sleep(700);
      await waitVisible();
      for (const ch of cmd) {
        typed += ch;
        paint(`${PS}${esc(typed)}<span class="cur"></span>`);
        await sleep(45 + Math.random() * 70);
      }
      await sleep(280);
      lines.push(`${PS}${esc(cmd)}`);
      paint(`${PS}<span class="cur"></span>`);
    };
    const DISK = [
      "Filesystem       Size   Used  Avail Capacity  Mounted on",
      "/dev/disk3s1s1  460Gi   12Gi  238Gi     5%    /",
      "/dev/disk3s5    460Gi  197Gi  238Gi    46%    /System/Volumes/Data",
    ];

    const settled = () => {
      lines = [`${PS}df chapter-7.md`, `${PS}df ghost-chapter.md`, `${PS}df -h`, ...DISK];
      paint(`${PS}<span class="cur"></span>`);
    };

    (async () => {
      if (reduced) return settled();
      for (;;) {
        lines = [];
        await typeCmd("df chapter-7.md");
        mini.setAttribute("data-in", "");
        await sleep(2300);
        mini.removeAttribute("data-in");
        await typeCmd("df ghost-chapter.md");
        pop.setAttribute("data-in", "");
        await sleep(2600);
        pop.removeAttribute("data-in");
        await typeCmd("df -h");
        for (const l of DISK) {
          lines.push(esc(l));
          paint(`${PS}<span class="cur"></span>`);
          await sleep(60);
        }
        await sleep(4200);
      }
    })();
  }

  /* ------------------------------------------------------------ Copy */
  $$("[data-copy]").forEach((b) => {
    const state = $("[data-copy-state]", b);
    b.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(b.dataset.copy);
        state.textContent = "Copied";
      } catch {
        state.textContent = "Select to copy";
      }
      b.setAttribute("data-done", "");
      setTimeout(() => {
        state.textContent = "Copy";
        b.removeAttribute("data-done");
      }, 1600);
    });
  });

  /* ------------------------------------------------------------ Closing: the nib writes the underline */
  const begin = $("[data-begin]");
  if (begin) {
    const nib = $(".begin-nib", begin);
    const svg = $(".begin-line", begin);
    const D = $("path", svg).getAttribute("d");
    const place = () => {
      // The line is drawn in a 600x30 box stretched to the heading; give the nib the same path in pixels.
      const r = svg.getBoundingClientRect();
      const o = begin.getBoundingClientRect();
      const sx = r.width / 600, sy = r.height / 30, dx = r.left - o.left, dy = r.top - o.top;
      let k = 0;
      const scaled = D.replace(/-?\d+(\.\d+)?/g, (n) => {
        const v = parseFloat(n);
        return ((k++ % 2 === 0 ? v * sx + dx : v * sy + dy)).toFixed(1);
      });
      nib.style.offsetPath = `path("${scaled}")`;
      nib.style.left = "0";
      nib.style.top = "0";
      nib.style.bottom = "auto";
    };
    place();
    addEventListener("resize", place, { passive: true });
    document.fonts?.ready.then(place);
  }
})();
