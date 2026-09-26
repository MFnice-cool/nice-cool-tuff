import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Two projects: node (default, pure logic) and a happy-dom project for
    // tests that touch the DOM (localStorage persistence, body-visibility
    // sync for the idle-CPU policy).
    projects: [
      {
        test: {
          name: "node",
          include: ["src/**/*.test.ts"],
          exclude: ["src/lib/libraryGroups.test.ts", "src/lib/idlePerf.test.ts"],
        },
      },
      {
        test: {
          name: "dom",
          include: ["src/lib/libraryGroups.test.ts", "src/lib/idlePerf.test.ts"],
          environment: "happy-dom",
        },
      },
    ],
  },
});
