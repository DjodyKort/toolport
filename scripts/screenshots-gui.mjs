#!/usr/bin/env node
// Runs the browser smoke and keeps its GUI screenshots in docs/assets/gui-<screen>.png.
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const result = spawnSync(
  process.execPath,
  [path.join(root, "scripts/browser-smoke.mjs")],
  {
    cwd: root,
    stdio: "inherit",
    env: {
      ...process.env,
      TOOLPORT_SCREENSHOT_DIR:
        process.env.TOOLPORT_SCREENSHOT_DIR || path.join(root, "docs/assets"),
    },
  },
);
process.exit(result.status ?? 1);
