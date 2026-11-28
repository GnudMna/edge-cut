// UI に同梱する npm パッケージ (dependencies) のライセンス情報の取得
//
// check-licenses.mjs と third-party-notices.mjs が共有する

import { execSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

/** `pnpm licenses list --prod` の結果をパッケージの一覧にする */
export function listProdPackages(cwd) {
  // Windows の pnpm はバッチファイルのため、シェル経由で起動する (引数は固定値のみ)
  const json = execSync("pnpm licenses list --prod --json", { cwd, encoding: "utf8" });
  const byLicense = JSON.parse(json);
  const packages = [];
  for (const [license, entries] of Object.entries(byLicense)) {
    for (const entry of entries) {
      entry.versions.forEach((version, i) => {
        packages.push({ name: entry.name, version, license, path: entry.paths[i] ?? entry.paths[0] });
      });
    }
  }
  return packages.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}

/** deny.toml の `[licenses] allow` の一覧を読む (コメント行は除く) */
export function readAllowedLicenses(denyTomlPath) {
  const text = readFileSync(denyTomlPath, "utf8");
  const block = /\[licenses\][\s\S]*?^allow\s*=\s*\[([\s\S]*?)^\]/m.exec(text);
  if (!block) throw new Error(`deny.toml に [licenses] allow が見つかりません: ${denyTomlPath}`);
  const allowed = new Set();
  for (const line of block[1].split(/\r?\n/)) {
    if (line.trim().startsWith("#")) continue;
    for (const m of line.matchAll(/"([^"]+)"/g)) allowed.add(m[1]);
  }
  return allowed;
}

/**
 * SPDX 式が許可リストで満たされるか
 *
 * `OR` はいずれか、`AND` はすべてが許可されていればよい (括弧は 1 段のみ対応)
 */
export function isLicenseAllowed(expression, allowed) {
  const expr = expression.trim().replace(/^\((.*)\)$/, "$1");
  const alternatives = splitTop(expr, "OR");
  return alternatives.some((alt) =>
    splitTop(alt.trim().replace(/^\((.*)\)$/, "$1"), "AND").every((part) => {
      const id = part.trim().replace(/^\((.*)\)$/, "$1");
      return id.includes(" OR ") || id.includes(" AND ") ? isLicenseAllowed(id, allowed) : allowed.has(id);
    }),
  );
}

/** 括弧の外にある演算子で式を分ける */
function splitTop(expr, op) {
  const parts = [];
  let depth = 0;
  let start = 0;
  const token = ` ${op} `;
  for (let i = 0; i < expr.length; i++) {
    if (expr[i] === "(") depth++;
    else if (expr[i] === ")") depth--;
    else if (depth === 0 && expr.startsWith(token, i)) {
      parts.push(expr.slice(start, i));
      start = i + token.length;
      i += token.length - 1;
    }
  }
  parts.push(expr.slice(start));
  return parts;
}

/** パッケージのライセンス本文を読む (LICENSE* / LICENCE* / COPYING*) */
export function readLicenseText(packagePath) {
  const file = readdirSync(packagePath).find((f) => /^(licen[cs]e|copying)(\.|$)/i.test(f));
  return file ? readFileSync(join(packagePath, file), "utf8").trim() : null;
}
