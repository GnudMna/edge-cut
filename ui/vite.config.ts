// EdgeCut の UI のビルド設定
//
// 出力先は build.rs が `--outDir` で `OUT_DIR` 配下を指定する
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  build: {
    // WebView2 (Chromium) だけで動けばよい
    target: "es2022",
    sourcemap: false,
    // カスタムプロトコルから配信するため、小さな画像もファイルのまま埋め込む
    assetsInlineLimit: 0,
    reportCompressedSize: false,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
