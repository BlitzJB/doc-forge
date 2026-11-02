/**
 * Scripted stand-in for the real agent. It emits the same kinds of "parts"
 * a real streaming agent would (reasoning, tool calls, text, proposed edits),
 * so the UI can be built against the final protocol before wiring a model.
 */
import type { Part, SearchResult } from "./types";

export interface ProposedEdit {
  find: string;
  replace: string;
  reason: string;
}

export interface AgentIO {
  setParts: (fn: (parts: Part[]) => Part[]) => void;
  /** Registers edits with the app; returns suggestion ids. */
  propose: (edits: ProposedEdit[]) => string[];
  isAborted: () => boolean;
  getDocText: () => string;
}

export interface AgentRequest {
  prompt: string;
  quote?: string | null;
  webSearch: boolean;
  io: AgentIO;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

class Stream {
  private n = 0;
  constructor(private io: AgentIO) {}

  push(part: Part) {
    const idx = this.n++;
    this.io.setParts((p) => [...p, part]);
    return idx;
  }

  patch(idx: number, patch: Partial<Part>) {
    this.io.setParts((p) => p.map((x, i) => (i === idx ? ({ ...x, ...patch } as Part) : x)));
  }

  async reason(text: string, ms = 1400) {
    const idx = this.push({ type: "reasoning", text, done: false });
    await sleep(ms);
    this.patch(idx, { done: true, seconds: Math.max(1, Math.round(ms / 1000)) } as Partial<Part>);
  }

  async text(text: string) {
    const idx = this.push({ type: "text", text: "" });
    for (let i = 0; i < text.length; ) {
      if (this.io.isAborted()) return;
      i = Math.min(text.length, i + 2 + Math.floor(Math.random() * 5));
      this.patch(idx, { text: text.slice(0, i) } as Partial<Part>);
      await sleep(16);
    }
  }

  async readDoc() {
    const idx = this.push({ type: "tool", tool: "read_document", status: "running" });
    await sleep(650);
    const words = this.io.getDocText().split(/\s+/).filter(Boolean).length;
    this.patch(idx, { status: "done", summary: `${words} words · 6 sections` } as Partial<Part>);
  }

  async search(query: string, results: SearchResult[]) {
    const idx = this.push({ type: "tool", tool: "web_search", status: "running", query });
    await sleep(1700);
    this.patch(idx, { status: "done", results } as Partial<Part>);
  }

  suggest(edits: ProposedEdit[]) {
    const ids = this.io.propose(edits);
    if (ids.length) this.push({ type: "suggestions", ids });
  }
}

// ---------------------------------------------------------------------------

const RESEARCH_RESULTS: SearchResult[] = [
  {
    title: "The State of Independent Commerce 2026",
    url: "https://merchantpulse.co/reports/independent-commerce-2026",
    domain: "merchantpulse.co",
    snippet:
      "In our survey of 1,200 independent merchants, 68% still rely on spreadsheets for weekly reporting, and only 1 in 5 uses a dedicated analytics product…",
  },
  {
    title: "SaaS churn benchmarks for SMB-focused tools",
    url: "https://saasmetrics.dev/benchmarks/smb-churn",
    domain: "saasmetrics.dev",
    snippet:
      "Monthly logo churn for SMB-focused SaaS typically falls between 3% and 7%, with best-in-class products under 2.5%…",
  },
  {
    title: "Why small stores abandon analytics tools",
    url: "https://storefrontweekly.com/analytics-abandonment",
    domain: "storefrontweekly.com",
    snippet:
      "Setup complexity was the #1 reason merchants cited for abandoning analytics tools within the first 30 days…",
  },
  {
    title: "Product Hunt launch playbook (2026 edition)",
    url: "https://launchnotes.io/product-hunt-playbook",
    domain: "launchnotes.io",
    snippet:
      "B2B tools that pre-build a waitlist of 1,000+ see on average 3× more day-one signups than cold launches…",
  },
];

const TIGHTEN_EDITS: ProposedEdit[] = [
  {
    find: "This document outlines how we plan to launch the product in the first quarter, who we are targeting, and how we will measure whether the launch was a success.",
    replace: "This plan covers our Q1 launch: who we're targeting, how we'll reach them, and how we'll measure success.",
    reason: "Shorter, front-loads the three things the doc covers",
  },
  {
    find: "Small ecommerce teams have a lot of data but they don't really have the time or the expertise to make sense of it.",
    replace: "Small ecommerce teams are drowning in data but lack the time and expertise to act on it.",
    reason: "Removes filler (“a lot of”, “really”) and sharpens the verb",
  },
  {
    find: "Most existing analytics tools are very complicated and expensive, and they are built for large companies with dedicated data teams.",
    replace: "Existing analytics tools are complex, expensive, and built for enterprises with dedicated data teams.",
    reason: "Parallel list reads faster",
  },
  {
    find: "We think that word of mouth will be our biggest channel.",
    replace: "We expect word of mouth to be our strongest acquisition channel.",
    reason: "More confident and more specific",
  },
];

function simplify(text: string) {
  let out = text
    .replace(/\b(really|very|just|quite|basically|actually) /gi, "")
    .replace(/\ba lot of\b/gi, "plenty of")
    .replace(/\bin order to\b/gi, "to")
    .replace(/\bWe think that\b/g, "We expect")
    .replace(/\bthat (?=word)/g, "")
    .replace(/\bwho currently track metrics in spreadsheets\b/g, "still running their business out of spreadsheets")
    .replace(/\bmanaging several stores at once\b/g, "juggling multiple storefronts")
    .replace(/\s{2,}/g, " ");
  if (out === text) out = text.replace(/, and /, " and ");
  return out.charAt(0).toUpperCase() + out.slice(1);
}

type Scenario = "summarize" | "research" | "decide" | "tighten" | "selection" | "help";

function pick(prompt: string, quote?: string | null): Scenario {
  const p = prompt.toLowerCase();
  if (/summar|tl;?dr|overview|recap/.test(p)) return "summarize";
  if (/research|data|stat|source|back.*up|evidence|market|search|cite|web/.test(p)) return "research";
  if (/free tier|trial|decid|open question|pricing/.test(p)) return "decide";
  if (quote) return "selection";
  if (/tighten|concise|clean|improve|polish|edit|rewrite|shorter|clarity|flow|better/.test(p)) return "tighten";
  return "help";
}

export async function runAgent({ prompt, quote, webSearch, io }: AgentRequest) {
  const s = new Stream(io);
  const scenario = pick(prompt, quote);
  const stopped = () => io.isAborted();

  switch (scenario) {
    case "summarize": {
      await s.reason("The user wants a quick summary. Read the doc, pull out the plan, the numbers, and what's unresolved.", 1100);
      if (stopped()) return;
      await s.readDoc();
      if (stopped()) return;
      await s.text(
        "Here's the short version:\n\n" +
          "- **What:** Northwind, lightweight analytics for small ecommerce teams\n" +
          "- **Who:** Shopify merchants ($50k–$2M), spreadsheet-driven founders, small agencies\n" +
          "- **How:** 50-merchant private beta in January → public waitlist → Product Hunt in late February\n" +
          "- **Goal:** 500 paying customers by end of March, churn under 5%\n\n" +
          "One thing is still open: **free tier vs. 14-day trial**. Want me to help you decide that?",
      );
      return;
    }

    case "research": {
      await s.reason(
        "The Problem and Success metrics sections make claims with no evidence. Look for data on merchant spreadsheet usage and SMB SaaS churn benchmarks, then cite them inline.",
        1500,
      );
      if (stopped()) return;
      await s.readDoc();
      if (stopped()) return;
      if (!webSearch) {
        await s.text(
          "I'd like to back the **Problem** and **Success metrics** sections with real numbers, but web search is turned off. Turn on **Web** in the composer and ask again.",
        );
        return;
      }
      await s.search("independent ecommerce merchants analytics spreadsheet usage 2026", RESEARCH_RESULTS.slice(0, 3));
      if (stopped()) return;
      await s.search("SMB SaaS monthly churn benchmark", [RESEARCH_RESULTS[1], RESEARCH_RESULTS[3]]);
      if (stopped()) return;
      await s.text(
        "Found two solid data points:\n\n" +
          "- **68%** of independent merchants still report out of spreadsheets [1]\n" +
          "- SMB SaaS monthly churn usually lands at **3–7%** [2], so your 5% target is reasonable but not ambitious\n\n" +
          "I've added both as cited sentences:",
      );
      if (stopped()) return;
      s.suggest([
        {
          find: "with dedicated data teams.",
          replace:
            "with dedicated data teams. In a 2026 survey of 1,200 independent merchants, 68% said they still rely on spreadsheets for weekly reporting [1].",
          reason: "Adds evidence for the problem statement",
        },
        {
          find: "keep monthly churn below 5%.",
          replace: "keep monthly churn below 5%, in line with the 3–7% benchmark for SMB-focused SaaS [2].",
          reason: "Puts the churn target in context",
        },
      ]);
      return;
    }

    case "decide": {
      await s.reason(
        "Weigh free tier vs. trial against the goal: 500 *paying* customers in ~6 weeks after launch. A free tier grows top-of-funnel but delays conversion.",
        1800,
      );
      if (stopped()) return;
      await s.readDoc();
      if (stopped()) return;
      await s.text(
        "Given your goal is **500 paying customers by end of March**, I'd go with the **14-day trial only**:\n\n" +
          "- A free tier mostly delays conversion, and you only have ~5 weeks after the Product Hunt launch\n" +
          "- Trials give you a clean conversion-rate signal from the beta cohort\n" +
          "- You can always add a free tier later. Taking one away hurts.\n\n" +
          "I turned the open question into a decision:",
      );
      if (stopped()) return;
      s.suggest([
        {
          find: "Open question: should we offer a free tier at launch, or only a 14-day trial?",
          replace:
            "Decision: launch with a 14-day trial only, and revisit a free tier once we have three months of conversion data.",
          reason: "Resolves the open question in line with the paid-customer goal",
        },
      ]);
      return;
    }

    case "selection": {
      const q = quote!;
      await s.reason("Rewrite only the selected passage, keeping its meaning and the surrounding tone.", 900);
      if (stopped()) return;
      const next = simplify(q);
      if (next === q) {
        await s.text("That passage already reads tight. I wouldn't change it. Want me to look at the surrounding section instead?");
        return;
      }
      await s.text("Here's a tighter version of your selection:");
      if (stopped()) return;
      s.suggest([{ find: q, replace: next, reason: "Cut filler words, kept the meaning" }]);
      return;
    }

    case "tighten": {
      await s.reason(
        "Go section by section looking for filler, hedging and long sentences. Keep the structure, change only sentence-level wording.",
        1600,
      );
      if (stopped()) return;
      await s.readDoc();
      if (stopped()) return;
      await s.text(
        "The structure is solid. Most of the slack is in **hedging** (“we think that…”) and **filler** (“a lot of”, “really”, “very”). I've marked four passages:",
      );
      if (stopped()) return;
      s.suggest(TIGHTEN_EDITS);
      if (stopped()) return;
      await s.text("Hover a mark to find it in the manuscript, or accept them together from the header.");
      return;
    }

    default: {
      await s.reason("No specific instruction. Offer what I can do with this document.", 700);
      if (stopped()) return;
      await s.text(
        "I can work directly on this document. For example:\n\n" +
          "- **Tighten the writing** by cutting filler and sharpening sentences\n" +
          "- **Back it up with data** using web search, with citations\n" +
          "- **Resolve open questions**, like the free tier vs. trial decision\n" +
          "- **Rewrite a passage**: select text in the manuscript and choose *Ask the editor*\n\n" +
          "With **Suggest** on, every change waits for your review. With **Apply directly**, I write into the manuscript and each change is kept in history.",
      );
    }
  }
}
