"use client";

import { useEffect, useRef, useState } from "react";

export interface MissingFile {
  path: string;
  /** "missing": can be created. "no-folder" / "not-a-file": can't be opened at all. */
  status: "missing" | "no-folder" | "not-a-file";
}

interface Props {
  file: MissingFile | null;
  onCreate: (path: string) => Promise<void>;
  onClose: () => void;
}

const HOME = /^\/Users\/[^/]+/;

function split(path: string) {
  const i = path.lastIndexOf("/");
  return { name: path.slice(i + 1), folder: path.slice(0, i).replace(HOME, "~") || "/" };
}

/** Asked when `df` names a file that isn't there yet. */
export function CreatePrompt({ file, onCreate, onClose }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const create = useRef<HTMLButtonElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (file && !d.open) {
      d.showModal();
      create.current?.focus();
    } else if (!file && d.open) d.close();
  }, [file]);

  const shown = file ?? { path: "", status: "missing" as const };
  const { name, folder } = split(shown.path);
  const canCreate = shown.status === "missing";

  return (
    <dialog
      ref={ref}
      className="settings w-[min(420px,calc(100vw-32px))]"
      aria-labelledby="create-title"
      onClose={() => {
        setError(null);
        onClose();
      }}
    >
      <div className="px-7 pb-6 pt-7">
        <div className="label mb-3">{canCreate ? "New file" : "Can't open"}</div>
        <h2 id="create-title" className="font-[family-name:var(--doc-font)] text-[21px] leading-snug text-ink">
          <span className="break-all">{name}</span> {canCreate ? "doesn't exist yet." : shown.status === "no-folder" ? "is in a folder that doesn't exist." : "isn't a file."}
        </h2>
        <p className="num mt-2 break-all text-[11.5px] text-graphite">{folder}</p>
        <p className="mt-4 text-[13px] leading-relaxed text-graphite">
          {canCreate
            ? "Would you like to create it? Doc Forge starts an empty document there and keeps its history from the first word."
            : shown.status === "no-folder"
              ? "Create the folder first, then run df again."
              : "Doc Forge opens text files such as Markdown documents."}
        </p>
        {error && <p className="swap-fade mt-3 text-[12.5px] text-pencil-deep">{error}</p>}
        <div className="mt-6 flex items-center justify-end gap-4">
          <button onClick={() => ref.current?.close()} className="tbtn text-[12.5px]">
            {canCreate ? "Cancel" : "OK"}
          </button>
          {canCreate && (
            <button
              ref={create}
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError(null);
                try {
                  await onCreate(shown.path);
                  ref.current?.close();
                } catch (e) {
                  setError(String(e).replace(/^Error:\s*/, ""));
                } finally {
                  setBusy(false);
                }
              }}
              className="press h-8 rounded-[3px] bg-ink px-3.5 text-[12.5px] font-medium text-paper hover:opacity-85 disabled:opacity-40"
            >
              Create {name}
            </button>
          )}
        </div>
      </div>
    </dialog>
  );
}
