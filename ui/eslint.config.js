// UI の静的解析の設定 (型情報付き typescript-eslint + React Hooks)
import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/", "node_modules/"] },
  js.configs.recommended,
  tseslint.configs.recommendedTypeChecked,
  reactHooks.configs.flat["recommended-latest"],
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      globals: globals.browser,
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
  },
  {
    files: ["vite.config.ts", "scripts/**/*.mjs", "eslint.config.js"],
    extends: [tseslint.configs.disableTypeChecked],
    languageOptions: { globals: globals.node },
  },
);
