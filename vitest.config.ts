import path from "node:path";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    // This repository contains vendored tools, plugin worktrees, and local
    // analysis checkouts that each carry their own test suites. Restrict the
    // root runner to cc-switch's own trees so `pnpm test:unit` is deterministic
    // and does not execute nested projects with incompatible fixtures.
    //
    // src/ is listed alongside tests/ because six suites live next to the code
    // they cover; with only tests/** they were silently never executed. Every
    // vendored checkout sits at the repo root, never under src/, so this stays
    // as tight as before.
    include: [
      "src/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
      "tests/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
    ],
    environment: "jsdom",
    setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
    globals: true,
    coverage: {
      reporter: ["text", "lcov"],
    },
  },
});
