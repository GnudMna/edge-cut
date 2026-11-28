// UI に同梱する npm パッケージのライセンスが deny.toml の許可リストに収まるか検証する
//
// 使い方: pnpm run licenses:check (check.ps1 から呼ぶ)

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { isLicenseAllowed, listProdPackages, readAllowedLicenses } from "./licenses.mjs";

const uiDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const allowed = readAllowedLicenses(join(uiDir, "..", "deny.toml"));
const packages = listProdPackages(uiDir);

const rejected = packages.filter((p) => !isLicenseAllowed(p.license, allowed));
for (const p of packages) {
  console.log(`  ${p.name}@${p.version}: ${p.license}`);
}
if (rejected.length > 0) {
  console.error("エラー: 許可されていないライセンスの npm パッケージがあります");
  for (const p of rejected) console.error(`  ${p.name}@${p.version}: ${p.license}`);
  process.exit(1);
}
console.log(`npm パッケージ ${packages.length} 件のライセンスを確認しました`);
