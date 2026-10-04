#!/usr/bin/env node
// Runs the browser smoke and keeps its GUI screenshots in docs/assets/gui-<screen>.png: the
// shell (sidebar B) and the All commands page at 1280x800 in light and dark, the Servers screen, the typed
// confirmation with its plan, a not-built screen with its tabs, the Tokens screen with the Usage
// tab (the figures, an index that never ran, the OTel plan, the receiver on) and the Compression
// tab (state, provider plan, health, the ledger empty and filled, the typed disable),
// and the element shots of the smoke. Fails when a page shot is missing or not 1280x800. The PNGs come out of
// Chromium already deflated tighter than zlib level 9 can do, so they are kept as they are.
import { spawnSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dir = process.env.TOOLPORT_SCREENSHOT_DIR || path.join(root, "docs/assets");
const PAGES = [
  "shell-light",
  "shell-dark",
  "all-commands-light",
  "all-commands-dark",
  "plan-confirm-light",
  "library-light",
  "servers-light",
  "servers-dark",
  "servers-remove-light",
  "servers-plan-light",
  "servers-profiles-light",
  "servers-profile-inspect-light",
  "servers-clients-light",
  "servers-health-light",
  "logins-light",
  "logins-dark",
  "logins-signin-light",
  "secrets-light",
  "secrets-set-light",
  "secrets-reveal-light",
  "integrations-light",
  "skills-light",
  "skills-dark",
  "skills-checks-light",
  "skills-checks-dark",
  "skills-sync-plan-light",
  "skills-uninstall-light",
  "skills-taps-light",
  "skills-install-blocked-light",
  "agents-light",
  "agents-dark",
  "agents-plan-light",
  "agents-clean-light",
  "styles-empty-light",
  "styles-light",
  "styles-dark",
  "usage-light",
  "usage-dark",
  "usage-empty-light",
  "usage-otel-plan-light",
  "usage-otel-on-light",
  "tokens-light",
  "tokens-dark",
  "tokens-provider-plan-light",
  "tokens-health-light",
  "tokens-ledger-empty-light",
  "tokens-ledger-light",
  "tokens-disable-light",
  "context-light",
  "context-dark",
  "context-plan-light",
  "context-remove-light",
  "context-move-light",
  "context-loads-light",
];
const SIZE = [1280, 800];

const started = Date.now() - 1000;
const run = spawnSync(process.execPath, [path.join(root, "scripts/browser-smoke.mjs")], {
  cwd: root,
  stdio: "inherit",
  env: { ...process.env, TOOLPORT_SCREENSHOT_DIR: dir },
});
if (run.status !== 0) process.exit(run.status ?? 1);

let failed = false;
for (const name of PAGES) {
  const file = path.join(dir, `gui-${name}.png`);
  const info = statSync(file, { throwIfNoEntry: false });
  if (!info || info.mtimeMs < started) {
    console.error(`missing or stale: ${file}`);
    failed = true;
    continue;
  }
  const header = readFileSync(file).subarray(16, 24);
  const [width, height] = [header.readUInt32BE(0), header.readUInt32BE(4)];
  const ok = width === SIZE[0] && height === SIZE[1];
  failed ||= !ok;
  console.log(
    `${ok ? "ok" : "WRONG SIZE"} ${file} ${width}x${height} ${info.size} bytes`,
  );
}
process.exit(failed ? 1 : 0);
