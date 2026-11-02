import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Static export: the Tauri shell loads ./out as files, no Node server.
  output: "export",
  images: { unoptimized: true },
  devIndicators: false,
};

export default nextConfig;
