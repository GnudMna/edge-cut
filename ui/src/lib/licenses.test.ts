/* eslint-disable @typescript-eslint/no-unsafe-call -- 型定義の無いビルドスクリプト (JavaScript) を呼ぶ */
import { describe, expect, it } from "vitest";

// @ts-expect-error ビルドスクリプト (JavaScript) を直接テストする
import { isLicenseAllowed } from "../../scripts/licenses.mjs";

const allowed = new Set(["MIT", "Apache-2.0", "Unicode-3.0"]);

describe("isLicenseAllowed", () => {
  it("単独のライセンスを判定すること", () => {
    expect(isLicenseAllowed("MIT", allowed)).toBe(true);
    expect(isLicenseAllowed("GPL-3.0", allowed)).toBe(false);
  });

  it("OR はいずれか、AND はすべてが許可されていればよいこと", () => {
    expect(isLicenseAllowed("GPL-3.0 OR MIT", allowed)).toBe(true);
    expect(isLicenseAllowed("(MIT OR Apache-2.0) AND Unicode-3.0", allowed)).toBe(true);
    expect(isLicenseAllowed("MIT AND GPL-3.0", allowed)).toBe(false);
  });
});
