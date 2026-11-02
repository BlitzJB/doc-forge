"use client";

import { useCallback, useEffect, useState } from "react";
import { desktop } from "@/lib/desktop";
import { Proofread } from "./Loaders";

type Status = Awaited<ReturnType<typeof desktop.cliStatus>>;

/** Shared state for the `df` command: status, install, remove. */
function useCli() {
  const [status, setStatus] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ tone: "ok" | "bad"; text: string } | null>(null);
  const refresh = useCallback(() => desktop.cliStatus().then(setStatus, () => setStatus(null)), []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  const run = async (fn: () => Promise<unknown>, ok: string) => {
    setBusy(true);
    setMsg(null);
    try {
      await fn();
      setMsg({ tone: "ok", text: ok });
    } catch (e) {
      setMsg({ tone: "bad", text: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setBusy(false);
      void refresh();
    }
  };
  return { status, busy, msg, run };
}

/** Settings → Editor: the full control. */
export function CliSetting() {
  const { status, busy, msg, run } = useCli();
  if (!status) return <span className="text-[12.5px] text-graphite">Checking…</span>;
  const installed = !!status.installedAt;
  return (
    <div className="text-[13px]">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        {installed ? (
          <span className="text-ink-2">
            Installed at <span className="num text-[12px]">{status.installedAt}</span>
          </span>
        ) : (
          <button
            onClick={() => run(desktop.cliInstall, "Installed. Try df notes.md in a new terminal window.")}
            disabled={busy || !!status.foreign}
            className="press h-8 rounded-[3px] bg-ink px-3 text-[12px] font-medium text-paper hover:opacity-85 disabled:opacity-30"
          >
            Install df
          </button>
        )}
        {installed && !status.current && (
          <button onClick={() => run(desktop.cliInstall, "Now points at this copy of Doc Forge.")} disabled={busy} className="tbtn text-[12.5px]">
            Point at this copy
          </button>
        )}
        {installed && (
          <button onClick={() => run(desktop.cliUninstall, "Removed. df is the system's disk-free command again.")} disabled={busy} className="tbtn text-[12.5px]">
            Remove
          </button>
        )}
        {busy && <Proofread />}
      </div>
      <p className="mt-2 text-[12px] leading-snug text-graphite">
        <span className="num">df notes.md</span> opens a file here, and offers to create it if it doesn&apos;t exist. Everything else, like{" "}
        <span className="num">df -h</span>, still runs the system&apos;s disk-free command.
      </p>
      {status.foreign && !installed && (
        <p className="mt-2 text-[12px] text-pencil-deep">{status.foreign} already exists and isn&apos;t Doc Forge&apos;s, so it&apos;s left alone.</p>
      )}
      {msg && <p className={`swap-fade mt-2 text-[12px] ${msg.tone === "ok" ? "text-ink-2" : "text-pencil-deep"}`}>{msg.text}</p>}
    </div>
  );
}

/** Start screen: a one-line offer until it's installed (or dismissed). */
export function CliOffer() {
  const { status, busy, msg, run } = useCli();
  const [dismissed, setDismissed] = useState(() => {
    try {
      return localStorage.getItem("forge:cli-offer") === "dismissed";
    } catch {
      return false;
    }
  });
  if (!status || dismissed || status.foreign) return null;
  if (status.installedAt && !msg) return null;
  return (
    <span className="swap-fade flex min-w-0 items-center gap-3">
      {msg ? (
        <span className={msg.tone === "ok" ? "text-ink-2" : "text-pencil-deep"}>{msg.text}</span>
      ) : (
        <>
          <span className="truncate">
            Open files from Terminal with <span className="num text-ink-2">df notes.md</span>
          </span>
          <button onClick={() => run(desktop.cliInstall, "Installed. Try df notes.md in a new terminal window.")} disabled={busy} className="tbtn shrink-0 font-medium text-ink">
            Install
          </button>
          <button
            onClick={() => {
              setDismissed(true);
              try {
                localStorage.setItem("forge:cli-offer", "dismissed");
              } catch {}
            }}
            className="tbtn shrink-0"
          >
            Not now
          </button>
        </>
      )}
    </span>
  );
}
