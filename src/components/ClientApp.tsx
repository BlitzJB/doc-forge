"use client";

import dynamic from "next/dynamic";

// The editor, version timestamps and mock agent are all client-only.
const DocForge = dynamic(() => import("./DocForge"), {
  ssr: false,
  loading: () => <div className="h-dvh w-full bg-paper" />,
});

export function ClientApp() {
  return <DocForge />;
}
