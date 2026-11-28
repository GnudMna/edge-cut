// UI に同梱する npm パッケージのライセンス表記を出力する
//
// 使い方: node scripts/third-party-notices.mjs <出力ファイル>
// generate-third-party-notices.ps1 が THIRD_PARTY_NOTICES の末尾へ追記する

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { listProdPackages, readLicenseText } from "./licenses.mjs";

const out = process.argv[2];
if (!out) {
  console.error("使い方: node scripts/third-party-notices.mjs <出力ファイル>");
  process.exit(2);
}

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const rule = "=".repeat(72);
const sections = [
  rule,
  "UI (JavaScript) に含まれるソフトウェア",
  rule,
  "",
];
for (const p of listProdPackages(uiDir)) {
  const text = readLicenseText(p.path);
  if (!text) throw new Error(`ライセンス本文が見つかりません: ${p.name}@${p.version} (${p.path})`);
  sections.push(`${p.name} ${p.version} (${p.license})`, "-".repeat(72), text, "");
}
writeFileSync(out, `${sections.join("\n").trimEnd()}\n`, "utf8");
console.log(`生成しました: ${out}`);
