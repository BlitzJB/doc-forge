import type { Metadata } from "next";
import { EB_Garamond, IBM_Plex_Mono, IBM_Plex_Sans, IBM_Plex_Serif, Literata, Source_Serif_4 } from "next/font/google";
import { BOOT_SCRIPT } from "@/lib/settings";
import "./globals.css";

const sans = IBM_Plex_Sans({ variable: "--font-ui", subsets: ["latin"], weight: ["400", "500", "600"] });
const mono = IBM_Plex_Mono({ variable: "--font-mono-ui", subsets: ["latin"], weight: ["400", "500"] });
const serif = Literata({ variable: "--font-text", subsets: ["latin"], style: ["normal", "italic"] });
// Alternate manuscript faces (Settings → Appearance); not preloaded.
const source = Source_Serif_4({ variable: "--font-source", subsets: ["latin"], style: ["normal", "italic"], preload: false });
const garamond = EB_Garamond({ variable: "--font-garamond", subsets: ["latin"], style: ["normal", "italic"], preload: false });
const plexSerif = IBM_Plex_Serif({ variable: "--font-plex-serif", subsets: ["latin"], weight: ["400", "500", "600"], style: ["normal", "italic"], preload: false });

export const metadata: Metadata = {
  title: "Doc Forge",
  description: "A manuscript editor with an editor in the margin",
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={`${sans.variable} ${mono.variable} ${serif.variable} ${source.variable} ${garamond.variable} ${plexSerif.variable} h-full antialiased`}
    >
      <head>
        {/* Applies saved theme, ink and type before first paint. */}
        <script dangerouslySetInnerHTML={{ __html: BOOT_SCRIPT }} />
      </head>
      <body className="h-full">{children}</body>
    </html>
  );
}
