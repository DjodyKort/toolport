#!/usr/bin/env node
/* global window, document, getComputedStyle, Image */
import { chromium, expect } from "@playwright/test";
import { createServer } from "vite";
import { existsSync, realpathSync } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(root, ".verify", `browser-${Date.now()}-${process.pid}`);
await mkdir(output, { recursive: true });
const server = await createServer({
  root,
  server: {
    host: "127.0.0.1",
    port: 0,
    strictPort: false,
    open: false,
    hmr: false,
    fs: { allow: [root, realpathSync(path.join(root, "node_modules"))] },
  },
  logLevel: "error",
});
// `npm run screenshots:gui` sets TOOLPORT_SCREENSHOT_DIR so the screens of the app that
// docs and reviews cite (docs/assets/gui-<screen>.png) come from this run, not from a hand copy.
// The shell and the All commands page are taken at 1280x800 in both themes.
const screenshotDir = process.env.TOOLPORT_SCREENSHOT_DIR;
async function guiShot(target, screen, options = {}) {
  const file = `gui-${screen}.png`;
  await target.screenshot({ path: path.join(output, file), ...options });
  if (!screenshotDir) return;
  await mkdir(screenshotDir, { recursive: true });
  await target.screenshot({ path: path.join(screenshotDir, file), ...options });
}
// The Servers screen on the fixture registry: a server that needs a login, one that fails to
// start, profiles with a server that answers 401, clients, health, and the two dialogs that
// guard a write (the plan, and the typed confirmation of a removal).
async function serversScreen(shot, theme) {
  const snap = (target, name) => guiShot(target, name, { animations: "disabled" });
  const nav = shot.getByRole("navigation", { name: "Views" });
  await nav.getByRole("button", { name: "Servers", exact: true }).click();
  await expect(shot.getByRole("tablist", { name: "Servers sections" })).toBeVisible();
  const list = shot.getByRole("region", { name: "Needs attention" });
  await list.getByRole("button", { name: /issue-tracker/ }).click();
  const detail = shot.getByRole("region", { name: "issue-tracker details" });
  await expect(detail.getByText("Sign-in needed.")).toBeVisible();
  await detail.getByRole("button", { name: "Inspect live" }).click();
  await expect(detail.getByText(/asked for a sign-in/)).toBeVisible();
  await shot.evaluate(() => document.fonts.ready);
  await snap(shot, `servers-${theme}`);
  if (theme !== "light") return;
  await snap(shot.getByRole("region", { name: "Gateway" }), "servers-gateway-light");

  await list.getByRole("button", { name: /acme-erp/ }).click();
  const erp = shot.getByRole("region", { name: "acme-erp details" });
  await expect(erp.getByText("ERP_API_KEY")).toBeVisible();
  await expect(erp.getByText("secret, stored in the vault")).toBeVisible();
  await erp.getByRole("button", { name: "Remove…" }).click();
  await shot.getByRole("button", { name: "Preview removal" }).click();
  const remove = shot.getByRole("dialog", { name: "Remove acme-erp?" });
  await expect(remove.getByRole("region", { name: "Preview" })).toBeVisible();
  await expect(remove.getByRole("button", { name: "Remove server" })).toBeDisabled();
  await remove.getByRole("textbox", { name: /type acme-erp to confirm/i }).fill("acme");
  await snap(shot, "servers-remove-light");
  await remove.getByRole("button", { name: "Cancel" }).click();

  await shot
    .getByRole("region", { name: "Connected" })
    .getByRole("button", { name: /docs-search/ })
    .click();
  await shot.getByRole("switch", { name: "docs-search in profile Work" }).click();
  const plan = shot.getByRole("dialog", { name: "Add docs-search to Work?" });
  await expect(plan.getByRole("region", { name: "Preview" })).toBeVisible();
  await snap(shot, "servers-plan-light");
  await plan.getByRole("button", { name: "Cancel" }).click();

  const tabs = shot.getByRole("tablist", { name: "Servers sections" });
  await tabs.getByRole("tab", { name: /^Profiles/ }).click();
  await expect(shot.getByRole("list", { name: "Profiles" })).toBeVisible();
  await snap(shot, "servers-profiles-light");
  await shot.getByRole("button", { name: "Inspect Work" }).click();
  const inspect = shot.getByRole("dialog", { name: "Inspect Work" });
  await expect(inspect.getByText(/2 of 3 servers answered/)).toBeVisible();
  await expect(inspect.getByText("Needs a login")).toBeVisible();
  await snap(shot, "servers-profile-inspect-light");
  await inspect.getByRole("button", { name: "Close" }).first().click();

  await tabs.getByRole("tab", { name: /^Clients/ }).click();
  await expect(shot.getByRole("table")).toBeVisible();
  await snap(shot, "servers-clients-light");

  await tabs.getByRole("tab", { name: "Health" }).click();
  await expect(shot.getByRole("list", { name: "Doctor checks" })).toBeVisible();
  await expect(shot.getByRole("button", { name: "Run skills sync" })).toBeVisible();
  await snap(shot, "servers-health-light");
  await tabs.getByRole("tab", { name: "Servers" }).click();
  await shot.getByRole("button", { name: "Classic view" }).click();
  await expect(shot.getByText("GitHub", { exact: true })).toBeVisible();
  await expect(nav.getByRole("button", { name: "Servers", exact: true })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await nav.getByRole("button", { name: "Servers", exact: true }).click();
  await expect(shot.getByRole("tablist", { name: "Servers sections" })).toBeVisible();
}

// The Tokens screen, tab Usage, on the synthetic transcript index: the strip and the chart, the
// tables, an index that never ran, and the OTel card, whose Enable is previewed with the keys it
// writes, applied (the receiver then listens) and turned off again.
async function usageTab(shot, theme) {
  const snap = (target, name) => guiShot(target, name, { animations: "disabled" });
  const nav = shot.getByRole("navigation", { name: "Views" });
  await nav.getByRole("button", { name: "Tokens", exact: true }).click();
  await shot
    .getByRole("tablist", { name: "Tokens sections" })
    .getByRole("tab", { name: "Usage" })
    .click();
  await expect(shot.getByRole("group", { name: "Usage summary" })).toContainText(
    "Tokens, last 14 days",
  );
  await expect(shot.getByRole("img", { name: /Tokens per day/ })).toBeVisible();
  const projects = shot.getByRole("region", { name: "By project" });
  await expect(projects.getByText("acme-erp", { exact: true })).toBeVisible();
  const servers = shot.getByRole("region", { name: "By MCP server" });
  await expect(servers.getByText("github", { exact: true })).toBeVisible();
  const card = shot.getByRole("region", { name: "OpenTelemetry receiver" });
  await expect(card.getByText("Off", { exact: true })).toBeVisible();
  await shot.evaluate(() => document.fonts.ready);
  await snap(shot, `usage-${theme}`);
  if (theme === "light") {
    const dialog = shot.getByRole("dialog");
    const closeResult = async () => {
      await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
      await expect(dialog).toHaveCount(0);
    };
    const root = shot.getByLabel(/Transcript folder/);
    await root.fill("/fixture/empty");
    await shot.getByRole("button", { name: "Use this folder" }).click();
    await expect(shot.getByText("Nothing indexed yet")).toBeVisible();
    await snap(shot, "usage-empty-light");
    await root.fill("");
    await shot.getByRole("button", { name: "Use this folder" }).click();
    await expect(projects.getByText("acme-erp", { exact: true })).toBeVisible();

    await card.scrollIntoViewIfNeeded();
    await shot.getByRole("button", { name: "Enable…" }).click();
    const plan = shot.getByRole("dialog", {
      name: "Enable the OTel receiver on port 4318?",
    });
    await expect(plan.getByRole("region", { name: "Preview" })).toBeVisible();
    await expect(plan.getByText("env.CLAUDE_CODE_ENABLE_TELEMETRY: added")).toBeVisible();
    await expect(plan.getByText(/Restart running Claude Code sessions/)).toBeVisible();
    await snap(shot, "usage-otel-plan-light");
    await plan.getByRole("button", { name: "Enable receiver" }).click();
    await closeResult();
    await expect(card.getByText("Listening")).toBeVisible();
    await expect(shot.getByRole("group", { name: "Usage summary" })).toContainText(
      "Listening",
    );
    await card.scrollIntoViewIfNeeded();
    await snap(shot, "usage-otel-on-light");

    await shot.getByRole("button", { name: "Disable…" }).click();
    await shot
      .getByRole("dialog", { name: "Disable the OTel receiver?" })
      .getByRole("button", { name: "Disable receiver" })
      .click();
    await closeResult();
    await expect(card.getByText("Off", { exact: true })).toBeVisible();
    await shot.evaluate(() => window.scrollTo(0, 0));
  }
  expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
}

// The Tokens screen, tab Compression, on the stateful compression world: today's policy and
// its presets, a provider switch (the plan, then the apply that changes the strip and the
// health checks), a ledger that starts empty and grows from two recorded entries, and the
// typed confirmation of a disable.
async function tokensScreen(shot, theme) {
  const snap = (target, name) => guiShot(target, name, { animations: "disabled" });
  const nav = shot.getByRole("navigation", { name: "Views" });
  await nav.getByRole("button", { name: "Tokens", exact: true }).click();
  await shot
    .getByRole("tablist", { name: "Tokens sections" })
    .getByRole("tab", { name: "Compression" })
    .click();
  const strip = shot.getByLabel("Compression status");
  await expect(strip).toContainText("rtk-only");
  const presets = shot.getByRole("region", { name: "Presets" });
  await expect(presets.getByText("agent", { exact: true })).toBeVisible();
  const ledger = shot.getByRole("region", { name: "Savings ledger" });
  await expect(ledger.getByText("No launches recorded yet")).toBeVisible();
  const health = shot.getByRole("region", { name: "Health checks" });
  await expect(health.getByText(/rtk binary found/)).toBeVisible();
  await shot.evaluate(() => document.fonts.ready);
  await snap(shot, `tokens-${theme}`);
  if (theme === "light") {
    const dialog = shot.getByRole("dialog");
    const closeResult = async () => {
      await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
      await expect(dialog).toHaveCount(0);
    };
    await ledger.scrollIntoViewIfNeeded();
    await snap(shot, "tokens-ledger-empty-light");
    await shot.evaluate(() => window.scrollTo(0, 0));

    await shot.getByRole("button", { name: "Switch to headroom" }).click();
    const plan = shot.getByRole("dialog", { name: "Switch the provider to headroom?" });
    await expect(plan.getByRole("region", { name: "Preview" })).toBeVisible();
    await expect(plan.getByText(/would save config \(provider=headroom\)/)).toBeVisible();
    await snap(shot, "tokens-provider-plan-light");
    await plan.getByRole("button", { name: "Switch provider" }).click();
    await expect(dialog.getByText(/^Provider headroom \(proxy\)/)).toBeVisible();
    await closeResult();
    await expect(strip).toContainText("headroom");
    await expect(strip).toContainText("Not installed");
    await expect(health.getByText("no ready proxy on :8787")).toBeVisible();
    await health.scrollIntoViewIfNeeded();
    await snap(shot, "tokens-health-light");

    const record = async (provider, before, after) => {
      await shot.getByRole("button", { name: "Record savings…" }).click();
      await dialog.getByLabel("Provider").selectOption(provider);
      await dialog.getByLabel("Tokens before").fill(before);
      await dialog.getByLabel("Tokens after").fill(after);
      await dialog.getByRole("button", { name: "Review" }).click();
      await dialog.getByRole("button", { name: "Record", exact: true }).click();
      await closeResult();
    };
    await record("rtk-only", "1000", "400");
    await record("headroom", "20000", "8000");
    await expect(
      ledger.getByRole("table", { name: "Savings by provider" }),
    ).toBeVisible();
    await expect(ledger.getByText("12,600")).toBeVisible();
    await ledger.scrollIntoViewIfNeeded();
    await snap(shot, "tokens-ledger-light");
    await shot.evaluate(() => window.scrollTo(0, 0));

    await shot.getByRole("button", { name: "Disable…" }).click();
    await dialog.getByRole("button", { name: "Preview" }).click();
    const off = shot.getByRole("dialog", { name: "Disable compression?" });
    await expect(off.getByRole("region", { name: "Preview" })).toBeVisible();
    await expect(
      off.getByRole("button", { name: "Disable", exact: true }),
    ).toBeDisabled();
    await off.getByRole("textbox").fill("disa");
    await snap(shot, "tokens-disable-light");
    await off.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
  }
  expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
}

// The Context screen on the fixture home: the Launch & shell tab with its deploy plan, launch
// profiles, shell shims, layers, what loads, folder routing and the checkpoint gauge, and the
// dialogs that guard a write (the plan, the typed confirmation, the shell move).
async function contextScreen(shot, theme) {
  const snap = (name) => guiShot(shot, name, { animations: "disabled" });
  const nav = shot.getByRole("navigation", { name: "Views" });
  await nav.getByRole("button", { name: "Context", exact: true }).click();
  const tabs = shot.getByRole("tablist", { name: "Context sections" });
  await expect(tabs.getByRole("tab", { name: "This folder" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await tabs.getByRole("tab", { name: "Launch & shell" }).click();
  await expect(tabs.getByRole("tab", { name: "Launch & shell" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  const section = (name) => shot.getByRole("region", { name });
  const profiles = shot.getByRole("list", { name: "Launch profiles" });
  await expect(profiles.getByText("claude-bare")).toBeVisible();
  await expect(section("Shell shims").getByText("Written")).toBeVisible();
  await expect(
    section("Shell shims").getByText(/still reads 1 line from the old folder/),
  ).toBeVisible();
  await expect(shot.getByRole("list", { name: "Tokens per layer" })).toBeVisible();
  await expect(shot.getByRole("list", { name: "Profile per folder" })).toBeVisible();
  await tabs.scrollIntoViewIfNeeded();
  await shot.evaluate(() => document.fonts.ready);
  await snap(`context-${theme}`);
  if (theme === "light") {
    const dialog = shot.getByRole("dialog");
    const cancel = async () => {
      await dialog.getByRole("button", { name: "Cancel" }).click();
      await expect(dialog).toHaveCount(0);
    };
    const deploy = section("Deploy");
    await deploy.getByRole("checkbox", { name: /Point the shell at Toolport/ }).check();
    await expect(deploy.getByText(/Line 3 of the shell rc file/)).toBeVisible();
    await deploy.getByRole("button", { name: "Sync…" }).click();
    const plan = shot.getByRole("dialog", { name: "Sync the context files?" });
    await expect(plan.getByRole("list", { name: "Changes" })).toBeVisible();
    await expect(plan.getByText(/sourced before shell-wrapper\.sh/)).toBeVisible();
    await shot.evaluate(() => document.fonts.ready);
    await snap("context-plan-light");
    await cancel();

    await profiles.getByRole("button", { name: "Remove bare" }).click();
    await dialog.getByRole("checkbox", { name: /Also delete its folder/ }).check();
    await dialog.getByRole("button", { name: "Preview", exact: true }).click();
    const remove = shot.getByRole("dialog", { name: "Remove launch profile bare?" });
    await expect(remove.getByText("Launch profile folder")).toBeVisible();
    await expect(remove.getByRole("button", { name: "Remove profile" })).toBeDisabled();
    await remove.getByRole("textbox").fill("bar");
    await shot.evaluate(() => document.fonts.ready);
    await snap("context-remove-light");
    await cancel();

    const shims = section("Shell shims");
    await shims.scrollIntoViewIfNeeded();
    await shims.getByRole("button", { name: "Move…" }).click();
    const move = shot.getByRole("dialog", { name: /Move the shell lines/ });
    await expect(move.getByText(/Line 3 of the shell rc file/)).toBeVisible();
    await expect(move.getByText(/sourced before shell-wrapper\.sh/)).toBeVisible();
    await move.getByText("Show the change").click();
    await expect(
      move.getByText(/source ~\/\.config\/mcpm\/context-shims\.zsh/),
    ).toBeVisible();
    await shot.evaluate(() => document.fonts.ready);
    await snap("context-move-light");
    await move.getByRole("button", { name: "Rewrite the shell file" }).click();
    await expect(shot.getByText("Deployed", { exact: true })).toBeVisible();
    await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
    await expect(dialog).toHaveCount(0);
    await expect(shims.getByText(/still reads/)).toHaveCount(0);

    const loads = section("What loads");
    await loads.scrollIntoViewIfNeeded();
    await expect(loads.getByText(/10,601/)).toBeVisible();
    await expect(loads.getByText(/5,666 more load on demand/)).toBeVisible();
    await shot.evaluate(() => document.fonts.ready);
    await snap("context-loads-light");
    await loads.getByLabel("Launch profile").selectOption("bare");
    await expect(loads.getByText(/with the profile bare/)).toBeVisible();

    const folders = section("Folder routing");
    await folders.getByRole("button", { name: "Turn on…" }).click();
    await expect(dialog.getByText(/no preview/)).toBeVisible();
    await dialog.getByRole("button", { name: "Turn on", exact: true }).click();
    await expect(shot.getByText("Folder routing is on")).toBeVisible();
    await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
    await expect(dialog).toHaveCount(0);
    await expect(folders.getByText("On", { exact: true })).toBeVisible();

    const checkpoint = section("Checkpoint");
    await checkpoint.getByLabel("Statusline JSON").fill('{"context_window":{}}');
    await checkpoint.getByLabel(/Checkpoint at/).fill("50000");
    await checkpoint.getByRole("button", { name: "Check" }).click();
    await expect(checkpoint.getByRole("meter", { name: "Context used" })).toBeVisible();
  }
}

// The Context tabs This folder, Profiles and Layers on the stateful Context world: the stack of
// a folder with its measure confirm, the profiles with the plan to apply one to a folder, and
// the layers with the read-only org file. Nothing is written: every dialog is cancelled.
async function contextTabsScreen(shot, theme) {
  const snap = (name) => guiShot(shot, name, { animations: "disabled" });
  const folder = "/fixture/work/erp/clients/acme-erp";
  const nav = shot.getByRole("navigation", { name: "Views" });
  await nav.getByRole("button", { name: "Context", exact: true }).click();
  const tabs = shot.getByRole("tablist", { name: "Context sections" });
  const tab = (name) => tabs.getByRole("tab", { name, exact: true });
  const dialog = shot.getByRole("dialog");
  await expect(tab("This folder")).toHaveAttribute("aria-selected", "true");
  await shot.getByLabel("Folder", { exact: true }).fill(folder);
  await shot.getByRole("button", { name: "Show", exact: true }).click();
  await expect(shot.getByRole("meter", { name: "Skill list budget" })).toBeVisible();
  await expect(
    shot
      .getByRole("region", { name: "Stack" })
      .getByText(/kit@market/)
      .first(),
  ).toBeVisible();
  await expect(shot.getByRole("list", { name: "Composed parts" })).toBeVisible();
  await tabs.scrollIntoViewIfNeeded();
  await shot.evaluate(() => document.fonts.ready);
  await guiShot(shot, `context-folder-${theme}`, { animations: "disabled" });
  if (theme === "light") {
    await shot.getByRole("button", { name: "Measure for real…" }).click();
    await expect(dialog.getByText(/spends model tokens/)).toBeVisible();
    await shot.evaluate(() => document.fonts.ready);
    await snap("context-measure-confirm-light");
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
  }

  await tab("Profiles").click();
  const profile = shot.getByRole("region", { name: "Profile acme-dev" });
  await expect(profile).toBeVisible();
  await expect(profile.getByRole("button", { name: "Open in Terminal" })).toBeDisabled();
  await expect(
    shot.getByRole("switch", { name: "Apply automatically" }),
  ).not.toBeChecked();
  await shot.evaluate(() => document.fonts.ready);
  await snap(`context-profiles-${theme}`);
  if (theme === "light") {
    await profile.getByRole("button", { name: "Apply to a folder…" }).click();
    const form = shot.getByRole("dialog", { name: "Apply profile acme-dev to a folder" });
    await form.getByLabel("Folder", { exact: true }).fill(folder);
    await form.getByRole("button", { name: "Review the plan" }).click();
    const plan = shot.getByRole("dialog", { name: /Apply profile acme-dev to / });
    await expect(plan.getByRole("list", { name: "Changes" })).toBeVisible();
    await shot.evaluate(() => document.fonts.ready);
    await snap("context-apply-plan-light");
    await plan.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
  }

  await tab("Layers").click();
  const layers = shot.getByRole("list", { name: "Layer list" });
  await expect(layers).toBeVisible();
  await expect(
    shot
      .getByRole("region", { name: "Org file" })
      .getByText("corp-tools", { exact: true }),
  ).toBeVisible();
  await shot.evaluate(() => document.fonts.ready);
  await snap(`context-layers-${theme}`);
}

// The System screen on the stateful System world: a machine that has not set sync up, the
// init with its passphrase on stdin, a push (the plan, the typed confirmation, the apply that
// moves the last sync), the updates with the update command of a server, the council with the
// key form, the import preview and the self-management card.
async function systemScreen(shot, theme) {
  const snap = (target, name) => guiShot(target, name, { animations: "disabled" });
  const nav = shot.getByRole("navigation", { name: "Views" });
  await nav.getByRole("button", { name: "System", exact: true }).click();
  const tabs = shot.getByRole("tablist", { name: "System sections" });
  const tab = (name) => tabs.getByRole("tab", { name, exact: true }).click();
  await expect(shot.getByText("Not set up", { exact: true })).toBeVisible();
  await shot.getByRole("button", { name: "Set up sync" }).click();
  const init = shot.getByRole("dialog");
  await init
    .getByLabel("Git repository")
    .fill("git@git.example.com:me/toolport-sync.git");
  await init.getByLabel("This machine's name").fill("work-laptop");
  await init.getByLabel("Passphrase", { exact: true }).fill("walk-passphrase");
  await init.getByLabel("Repeat passphrase").fill("walk-passphrase");
  await init.getByRole("button", { name: "Set up sync" }).click();
  await expect(
    init.getByText(/This machine is work-laptop on branch main/),
  ).toBeVisible();
  await init.getByRole("button", { name: "Done" }).click();
  await expect(shot.getByRole("dialog")).toHaveCount(0);
  await shot.getByRole("button", { name: "Push…" }).click();
  const push = shot.getByRole("dialog", { name: "Push to the sync repository?" });
  await expect(push.getByText("registry.json")).toBeVisible();
  await expect(push.getByRole("button", { name: "Push", exact: true })).toBeDisabled();
  if (theme === "light") await snap(shot, "system-sync-plan-light");
  await push.getByRole("textbox").fill("sync push");
  await push.getByRole("button", { name: "Push", exact: true }).click();
  const pushed = shot.getByRole("dialog");
  await expect(pushed.getByText(/^Pushed 3 files from work-laptop/)).toBeVisible();
  await pushed.getByRole("button", { name: "Close", exact: true }).last().click();
  await expect(shot.getByRole("dialog")).toHaveCount(0);
  await expect(shot.getByText(/This machine matches the remote bundle/)).toBeVisible();
  await shot.evaluate(() => document.fonts.ready);
  await snap(shot, `system-sync-${theme}`);
  if (theme === "light") {
    await tab("Updates");
    await expect(shot.getByText("srv-git", { exact: true })).toBeVisible();
    await expect(shot.getByText(/Update command:/)).toBeVisible();
    await snap(shot, "system-updates-light");

    await tab("Council");
    await shot.getByRole("button", { name: "Install…" }).click();
    await shot
      .getByRole("dialog", { name: "Install the council?" })
      .getByRole("button", { name: "Install", exact: true })
      .click();
    const installed = shot.getByRole("dialog");
    await expect(installed.getByText("Council installed")).toBeVisible();
    await installed.getByRole("button", { name: "Close", exact: true }).last().click();
    await shot.getByRole("button", { name: "Set key" }).click();
    const key = shot.getByRole("dialog", { name: "Set OPENROUTER_API_KEY" });
    await expect(key.getByLabel("New value")).toBeVisible();
    await snap(shot, "system-council-key-light");
    await key.getByRole("button", { name: "Cancel" }).click();
    await expect(shot.getByRole("dialog")).toHaveCount(0);

    await tab("Import");
    await shot.getByLabel("mcpm config folder").fill("/old/mcpm");
    await shot.getByRole("button", { name: "Preview import…" }).click();
    const plan = shot.getByRole("dialog", { name: "Import from mcpm?" });
    await expect(
      plan.getByText(/Import 2 servers, 1 profile and 0 secrets/),
    ).toBeVisible();
    await snap(shot, "system-import-light");
    await plan.getByRole("button", { name: "Cancel" }).click();
    await expect(shot.getByRole("dialog")).toHaveCount(0);

    await tab("Self-management");
    await expect(shot.getByRole("list", { name: "Tools" })).toBeVisible();
    await snap(shot, "system-self-light");
  }
  expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
}

// The Tasks screen on the stateful Tasks world: the list with a run that waits for you, that run
// opened and continued, a scheduled task run from its plan to its end, the history with a log,
// a draft made from a command, and the Refresh task action in Logins.
async function tasksScreen(shot, theme) {
  const snap = async (name) => {
    await shot.mouse.move(900, 20);
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, name, { animations: "disabled" });
  };
  const dialog = shot.getByRole("dialog");
  const nav = shot.getByRole("navigation", { name: "Views" });
  await nav.getByRole("button", { name: "Tasks", exact: true }).click();
  const tabs = shot.getByRole("tablist", { name: "Tasks sections" });
  await expect(tabs).toBeVisible();
  const list = shot.getByRole("list", { name: "Tasks", exact: true });
  await expect(list.getByRole("listitem")).toHaveCount(5);
  const summary = shot.getByRole("group", { name: "Summary" });
  await expect(summary.getByText("Needs you")).toBeVisible();
  await list.getByRole("button", { name: /Refresh the portal token/ }).click();
  const detail = shot.getByRole("group", { name: "Refresh the portal token" });
  await expect(detail.getByText("A run waits for you.")).toBeVisible();
  await snap(`tasks-list-${theme}`);
  if (theme === "light") {
    await detail
      .getByRole("region", { name: "What it does" })
      .evaluate((element) => element.scrollIntoView({ block: "start" }));
    await expect(detail.getByRole("list", { name: "Recent runs" })).toBeInViewport();
    await snap("tasks-detail-light");
  }
  const open = detail.getByRole("button", { name: "Open run" });
  await open.click();
  const waiting = shot.getByRole("dialog", { name: "Run Refresh the portal token" });
  await expect(waiting.getByRole("button", { name: "Continue" })).toBeVisible();
  if (theme === "light") await snap("tasks-run-waiting-light");
  await shot.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(open).toBeFocused();
  await open.click();
  await waiting.getByRole("button", { name: "Continue" }).click();
  await expect(waiting.getByRole("listitem", { name: "1. Sign in" })).toHaveAttribute(
    "data-status",
    "ok",
  );
  await expect(waiting.getByRole("button", { name: "Continue" })).toHaveCount(0);
  await waiting.getByRole("button", { name: "Close", exact: true }).last().click();
  await expect(dialog).toHaveCount(0);

  const row = list.getByRole("button", { name: /Nightly report/ });
  await row.focus();
  await shot.keyboard.press("Enter");
  await expect(row).toHaveAttribute("aria-current", "true");
  const nightly = shot.getByRole("group", { name: "Nightly report" });
  await expect(nightly.getByRole("list", { name: "Recent runs" })).toBeVisible();
  const runNow = nightly.getByRole("button", { name: "Run now…" });
  await runNow.click();
  await expect(dialog.getByText("Run task nightly-report (1 step)")).toBeVisible();
  await expect(
    dialog.getByText("Secret values are never shown or logged."),
  ).toBeVisible();
  if (theme === "light") await snap("tasks-run-plan-light");
  await shot.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(runNow).toBeFocused();
  await shot.keyboard.press("Enter");
  await expect(dialog.getByText("Run task nightly-report (1 step)")).toBeVisible();
  await dialog.getByRole("button", { name: "Run now", exact: true }).click();
  await expect(dialog.getByText("The run finished.")).toBeVisible({ timeout: 20000 });
  await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
  await expect(dialog).toHaveCount(0);
  await expect(
    nightly.getByRole("list", { name: "Recent runs" }).getByRole("listitem"),
  ).toHaveCount(2);

  await tabs.getByRole("tab", { name: "History" }).click();
  const runs = shot.getByRole("table", { name: "Runs" });
  await expect(runs.getByRole("row")).toHaveCount(5);
  await expect(runs.getByRole("button", { name: "Log of run-004" })).toBeVisible();
  if (theme === "light") await snap("tasks-history-light");
  await runs.getByRole("button", { name: "Log of run-004" }).click();
  await expect(dialog.getByRole("heading")).toBeVisible();
  await shot.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);

  await tabs.getByRole("tab", { name: "Tasks", exact: true }).click();
  await shot.getByRole("button", { name: "Create from a command…" }).click();
  await dialog.getByRole("radio", { name: /odoo-upgrade/ }).check();
  await dialog.getByRole("button", { name: "Review draft" }).click();
  await expect(dialog.getByText("write the new task odoo-upgrade")).toBeVisible();
  await dialog.getByRole("button", { name: "Create draft" }).click();
  await expect(dialog.getByText("Done")).toBeVisible();
  await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
  await expect(dialog).toHaveCount(0);
  await expect(list.getByRole("listitem")).toHaveCount(6);
  await expect(list.getByRole("button", { name: /Run odoo-upgrade/ })).toBeVisible();

  await nav.getByRole("button", { name: "Settings", exact: true }).click();
  await shot.getByRole("button", { name: "Open Logins & secrets" }).click();
  await expect(shot.getByRole("table", { name: "Logins" })).toBeVisible();
  const refresh = shot.getByRole("button", { name: "Refresh task for acme-erp" });
  await expect(refresh).toBeVisible();
  await expect(
    shot.getByRole("button", { name: "Refresh task for issue-tracker" }),
  ).toBeVisible();
  if (theme === "light") {
    await refresh.scrollIntoViewIfNeeded();
    await snap("logins-refresh-task-light");
  }
  await refresh.click();
  await expect(dialog.getByText("Run task erp-token (1 step)")).toBeVisible();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
}

let browser;
let context;
let page;
const errors = [];
try {
  await server.listen();
  const address = server.httpServer.address();
  const baseURL = `http://127.0.0.1:${address.port}`;
  browser = await chromium.launch({
    executablePath:
      process.env.TOOLPORT_BROWSER_BIN ||
      (existsSync("/usr/bin/chromium") ? "/usr/bin/chromium" : undefined),
    headless: true,
  });
  context = await browser.newContext({ viewport: { width: 1240, height: 900 } });
  await context.tracing.start({ screenshots: true, snapshots: true });
  const watch = async (target) => {
    target.on("pageerror", (error) => errors.push(error.message));
    target.on("response", (response) => {
      if (response.status() >= 400)
        errors.push(`HTTP ${response.status()}: ${response.url()}`);
    });
    // Fixtures must stay offline even if an application path starts using fetch.
    await target.route("**/*", (route) => {
      if (new URL(route.request().url()).origin === baseURL) return route.continue();
      errors.push(`Unexpected external request: ${route.request().url()}`);
      return route.abort();
    });
  };
  page = await context.newPage();
  await watch(page);
  await page.goto(`${baseURL}/fixtures/`);
  await expect(page.getByText("GitHub", { exact: true })).toBeVisible();
  await expect(page.getByText("≈41.1k tokens saved")).toBeVisible();
  await page.screenshot({ path: path.join(output, "servers.png") });
  await page.getByRole("button", { name: "Activity", exact: true }).click();
  await expect(page.getByText("Protection active.", { exact: true })).toBeVisible();
  await expect(
    page.getByText("Tool definitions kept out of your agent's context"),
  ).toBeVisible();
  await page.screenshot({ path: path.join(output, "activity.png") });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const authRows = page.getByRole("region", { name: "Sign-in health" });
  await expect(authRows.getByText("5 need attention")).toBeVisible();
  await expect(authRows.getByText("Needs sign-in")).toBeVisible();
  await expect(
    authRows.getByRole("button", { name: "Sign in to figma again" }),
  ).toBeVisible();
  await authRows.scrollIntoViewIfNeeded();
  await guiShot(authRows, "auth-rows");
  const whatLoads = page.getByRole("region", { name: "What loads" });
  await expect(whatLoads.getByText("~1738 tokens")).toBeVisible();
  await whatLoads.scrollIntoViewIfNeeded();
  await guiShot(whatLoads, "what-loads");
  const nav = page.getByRole("navigation", { name: "Views" });
  await expect(nav.getByRole("group")).toHaveCount(4);
  await expect(nav.getByRole("button", { name: /^Attention/ })).toContainText("3");
  await nav.getByRole("button", { name: "Library", exact: true }).click();
  await expect(page.getByRole("list", { name: "Skills" })).toBeVisible();
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(page.getByText("Not built yet")).toBeVisible();
  await expect(page.getByText(/built by MIG-GUI-12/)).toBeVisible();
  await nav.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "Open All commands" }).click();
  await expect(page.getByRole("heading", { name: "All commands" })).toBeVisible();
  await page.getByRole("button", { name: /^status/ }).click();
  await page
    .getByRole("region", { name: "status" })
    .getByRole("button", { name: "Run" })
    .click();
  await expect(page.getByText("encrypted-file")).toBeVisible();
  const fixture = await page.evaluate(() => window.toolportFixture);
  expect(fixture.missing).toEqual([]);
  expect(errors).toEqual([]);
  for (const theme of ["light", "dark"]) {
    const shot = await context.newPage();
    await shot.addInitScript((choice) => {
      localStorage.setItem("toolport-theme", choice);
    }, theme);
    await shot.setViewportSize({ width: 1280, height: 800 });
    await watch(shot);
    await shot.goto(`${baseURL}/fixtures/`);
    await expect(shot.getByText("GitHub", { exact: true })).toBeVisible();
    await expect(
      shot.getByRole("button", { name: /^Attention/ }).getByLabel("3 need you"),
    ).toBeVisible();
    await expect(shot.locator("html")).toHaveClass(
      theme === "dark" ? /dark/ : /^(?!.*dark)/,
    );
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `shell-${theme}`);
    await serversScreen(shot, theme);
    await usageTab(shot, theme);
    await tokensScreen(shot, theme);
    await systemScreen(shot, theme);
    await shot.getByRole("button", { name: "Settings", exact: true }).click();
    await shot.getByRole("button", { name: "Open All commands" }).click();
    await expect(shot.getByRole("list", { name: "Commands" })).toBeVisible();
    await shot.getByRole("button", { name: /^server uninstall/ }).click();
    const panel = shot.getByRole("region", { name: "server uninstall" });
    await panel.getByRole("textbox", { name: "server" }).fill("acme-erp");
    await expect(panel.getByLabel("Command line")).toContainText(
      "toolportctl server uninstall acme-erp",
    );
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `all-commands-${theme}`);
    if (theme === "light") {
      await panel.getByRole("button", { name: "Preview changes" }).click();
      const dialog = shot.getByRole("dialog", { name: "Apply server uninstall?" });
      await expect(dialog.getByRole("region", { name: "Preview" })).toBeVisible();
      await dialog
        .getByRole("textbox", { name: /type acme-erp to confirm/i })
        .fill("acme");
      await guiShot(shot, "plan-confirm-light");
      await dialog.getByRole("button", { name: "Cancel" }).click();
      await shot.getByRole("button", { name: "Settings", exact: true }).click();
      await shot.getByRole("button", { name: "Library", exact: true }).click();
      await expect(shot.getByRole("tablist", { name: "Library sections" })).toBeVisible();
      await shot.getByRole("tab", { name: "Plugins" }).click();
      await expect(shot.getByText("Not built yet")).toBeVisible();
      await guiShot(shot, "library-light");
    }
    await shot.getByRole("button", { name: "Settings", exact: true }).click();
    await shot.getByRole("button", { name: "Open Logins & secrets" }).click();
    await expect(shot.getByRole("table", { name: "Logins" })).toBeVisible();
    await expect(shot.getByText("Login needed").first()).toBeVisible();
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `logins-${theme}`);
    if (theme === "light") {
      await shot.getByRole("button", { name: "Sign in to issue-tracker" }).click();
      const signIn = shot.getByRole("dialog", { name: "Sign in to issue-tracker" });
      await expect(signIn.getByRole("button", { name: "Copy address" })).toBeVisible();
      await guiShot(shot, "logins-signin-light");
      await signIn.getByRole("button", { name: "Cancel" }).click();
      await expect(
        signIn.getByText("Cancelled. Nothing more was started."),
      ).toBeVisible();
      await signIn.getByRole("button", { name: "Close", exact: true }).last().click();
      await shot.getByRole("tab", { name: "Secrets" }).click();
      await expect(shot.getByRole("list", { name: "Secrets" })).toBeVisible();
      await guiShot(shot, "secrets-light");
      await shot.getByRole("button", { name: "Replace ERP_API_KEY of acme-erp" }).click();
      const setDialog = shot.getByRole("dialog", { name: "Replace ERP_API_KEY" });
      await expect(setDialog.getByLabel("New value")).toBeVisible();
      await guiShot(shot, "secrets-set-light");
      await setDialog.getByRole("button", { name: "Cancel" }).click();
      await shot.getByRole("button", { name: "Reveal ERP_API_KEY of acme-erp" }).click();
      const reveal = shot.getByRole("dialog", { name: "Reveal ERP_API_KEY?" });
      await expect(
        reveal.getByRole("button", { name: "Reveal for 10 seconds" }),
      ).toBeVisible();
      await expect(shot.getByText("fixture-vaulted-value")).toHaveCount(0);
      await guiShot(shot, "secrets-reveal-light");
      await reveal.getByRole("button", { name: "Cancel" }).click();
      await shot.getByRole("tab", { name: "Integrations" }).click();
      await expect(shot.getByRole("region", { name: "Statusline output" })).toBeVisible();
      await expect(shot.getByRole("region", { name: "Hook output" })).toBeVisible();
      await shot.evaluate(() => document.fonts.ready);
      await guiShot(shot, "integrations-light");
    }
    expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
    await shot.close();
  }
  expect(errors).toEqual([]);
  for (const theme of ["light", "dark"]) {
    const shot = await context.newPage();
    await shot.addInitScript((choice) => {
      localStorage.setItem("toolport-theme", choice);
    }, theme);
    await shot.setViewportSize({ width: 1280, height: 800 });
    await watch(shot);
    await shot.goto(`${baseURL}/fixtures/`);
    await shot.getByRole("button", { name: "Library", exact: true }).click();
    const rows = shot.getByRole("list", { name: "Skills" });
    await expect(rows).toBeVisible();
    await expect(
      shot.getByRole("group", { name: "Library" }).getByText("35 items"),
    ).toBeVisible();
    await rows
      .getByRole("button")
      .filter({ has: shot.locator("b", { hasText: /^deploy-helper$/ }) })
      .click();
    const detail = shot.getByRole("region", { name: "Skill deploy-helper" });
    await expect(detail.getByText(/longer than 200 characters/)).toBeVisible();
    await expect(
      detail
        .getByRole("list", { name: "Sync state of deploy-helper" })
        .getByText("Changed since sync"),
    ).toHaveCount(2);
    await shot
      .getByRole("tablist", { name: "Library sections" })
      .scrollIntoViewIfNeeded();
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `skills-${theme}`);
    const drift = shot.getByRole("group", { name: "Drift" });
    await expect(drift.getByText(/1 output missing or changed/)).toBeVisible();
    await expect(
      shot
        .getByRole("group", { name: "Changes since last sync" })
        .getByText("feature-spec"),
    ).toBeVisible();
    await drift.scrollIntoViewIfNeeded();
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `skills-checks-${theme}`);
    if (theme === "light") {
      await rows.scrollIntoViewIfNeeded();
      const dialog = shot.getByRole("dialog");
      await shot.getByRole("button", { name: "Sync…", exact: true }).click();
      await dialog.getByRole("button", { name: "Preview", exact: true }).click();
      await expect(
        dialog.getByText("Write 33 skills and 2 rules to 2 clients"),
      ).toBeVisible();
      await expect(
        dialog.getByText(/cursor: 'allowed-tools' field not supported/),
      ).toBeVisible();
      await guiShot(shot, "skills-sync-plan-light");
      await dialog.getByRole("button", { name: "Sync", exact: true }).click();
      await expect(
        dialog.getByText("Wrote 33 skills and 2 rules to 2 clients"),
      ).toBeVisible();
      await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
      await expect(dialog).toHaveCount(0);
      await expect(drift.getByText(/All 35 synced skills still in place/)).toBeVisible();
      await rows
        .getByRole("button")
        .filter({ has: shot.locator("b", { hasText: /^deploy-helper$/ }) })
        .click();
      await shot.getByRole("button", { name: "Uninstall deploy-helper" }).click();
      await dialog.getByRole("textbox").fill("deploy");
      await expect(
        dialog.getByRole("button", { name: "Uninstall", exact: true }),
      ).toBeDisabled();
      await guiShot(shot, "skills-uninstall-light");
      await dialog.getByRole("button", { name: "Cancel" }).click();
      await expect(dialog).toHaveCount(0);

      await shot.getByRole("tab", { name: "Taps" }).click();
      const taps = shot.getByRole("list", { name: "Taps" });
      await expect(taps.getByRole("listitem")).toHaveCount(2);
      await expect(taps.getByText("clone missing")).toBeVisible();
      await shot.evaluate(() => document.fonts.ready);
      await guiShot(shot, "skills-taps-light");

      await shot.getByRole("tab", { name: "Find and install" }).click();
      await shot.getByRole("textbox", { name: "Spec" }).fill("@acme/risky");
      await shot.getByRole("button", { name: "Preview install" }).click();
      await expect(dialog.getByRole("alert")).toContainText("1 high-severity finding");
      await guiShot(shot, "skills-install-blocked-light");
      await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
      await expect(dialog).toHaveCount(0);
    }
    expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
    await shot.close();
  }
  expect(errors).toEqual([]);
  for (const theme of ["light", "dark"]) {
    const shot = await context.newPage();
    await shot.addInitScript((choice) => {
      localStorage.setItem("toolport-theme", choice);
    }, theme);
    await shot.setViewportSize({ width: 1280, height: 800 });
    await watch(shot);
    await shot.goto(`${baseURL}/fixtures/`);
    await shot.getByRole("button", { name: "Library", exact: true }).click();
    const rows = shot.getByRole("list", { name: "Skills" });
    await expect(rows).toBeVisible();
    await shot
      .getByRole("group", { name: "Source", exact: true })
      .getByRole("button", { name: /^All/ })
      .click();
    await rows
      .getByRole("button")
      .filter({ has: shot.locator("b", { hasText: /^incident-notes$/ }) })
      .click();
    const skill = shot.getByRole("region", { name: "Skill incident-notes" });
    await expect(
      skill.getByRole("button", { name: "See the lint output" }),
    ).toBeVisible();
    await shot
      .getByRole("tablist", { name: "Library sections" })
      .scrollIntoViewIfNeeded();
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `skills-source-badges-${theme}`);

    await shot.getByRole("tab", { name: "Sources" }).click();
    const places = shot.getByRole("list", { name: "Sources" });
    await expect(places).toBeVisible();
    await expect(places.getByText("behind its remote (114)")).toBeVisible();
    const folders = shot.getByRole("list", { name: "Scanned folders" });
    await expect(folders.getByRole("listitem")).toHaveCount(3);
    await places
      .getByRole("button")
      .filter({ has: shot.locator("b", { hasText: /^ai-skills$/ }) })
      .click();
    const library = shot.getByRole("region", { name: "Source ai-skills" });
    await expect(library.getByText("2 commits behind, 2 commits ahead")).toBeVisible();
    await expect(library.getByRole("button", { name: "Pull" })).toBeEnabled();
    await expect(library.getByRole("button", { name: "Push…" })).toBeEnabled();
    await shot
      .getByRole("tablist", { name: "Library sections" })
      .scrollIntoViewIfNeeded();
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `skills-sources-${theme}`);
    if (theme === "light") {
      const dialog = shot.getByRole("dialog");
      await library
        .getByRole("group", { name: "Library remote" })
        .scrollIntoViewIfNeeded();
      await shot.evaluate(() => document.fonts.ready);
      await guiShot(shot, "sources-library-light");
      await library.getByRole("button", { name: "Pull" }).click();
      await expect(
        dialog.getByText("Fast-forward 2 commit(s) from origin/main"),
      ).toBeVisible();
      await guiShot(shot, "sources-pull-plan-light");
      await shot.keyboard.press("Escape");
      await expect(dialog).toHaveCount(0);
      await expect(library.getByRole("button", { name: "Pull" })).toBeFocused();
      await library.getByRole("button", { name: "Push…" }).click();
      await expect(dialog.getByRole("alert")).toContainText("looks like a secret");
      await expect(dialog.getByRole("button", { name: "Push", exact: true })).toHaveCount(
        0,
      );
      await guiShot(shot, "sources-push-blocked-light");
      await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
      await expect(dialog).toHaveCount(0);
      await shot.getByRole("button", { name: "Add folder to scan…" }).click();
      await dialog
        .getByRole("textbox", { name: "Folder" })
        .fill("/home/demo/work/client-repo");
      await dialog.getByRole("button", { name: "Preview", exact: true }).click();
      await expect(
        dialog.getByText("add source root /home/demo/work/client-repo"),
      ).toBeVisible();
      await guiShot(shot, "skills-sources-plan-light");
      await dialog.getByRole("button", { name: "Add folder", exact: true }).click();
      await expect(dialog.getByText("Done")).toBeVisible();
      await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
      await expect(dialog).toHaveCount(0);
      await expect(folders.getByRole("listitem")).toHaveCount(4);
      await shot.getByRole("button", { name: "Stop scanning dups" }).click();
      await expect(dialog.getByText("remove source root /home/demo/dups")).toBeVisible();
      await expect(
        dialog.getByRole("button", { name: "Stop scanning", exact: true }),
      ).toBeDisabled();
      await guiShot(shot, "skills-sources-remove-light");
      await dialog.getByRole("button", { name: "Cancel" }).click();
      await expect(dialog).toHaveCount(0);
    }
    expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
    await shot.close();
  }
  expect(errors).toEqual([]);
  for (const theme of ["light", "dark"]) {
    const shot = await context.newPage();
    await shot.addInitScript((choice) => {
      localStorage.setItem("toolport-theme", choice);
    }, theme);
    await shot.setViewportSize({ width: 1280, height: 800 });
    await watch(shot);
    await shot.goto(`${baseURL}/fixtures/`);
    await shot.getByRole("button", { name: "Library", exact: true }).click();
    await shot.getByRole("tab", { name: "Agents" }).click();
    const outputs = shot.getByRole("list", { name: "Output of scout per client" });
    await expect(outputs).toBeVisible();
    await expect(outputs.getByText("tools is dropped for Codex CLI")).toBeVisible();
    await expect(outputs.getByText("tools is dropped for Cursor")).toBeVisible();
    await expect(outputs.getByText("Not written yet")).toHaveCount(4);
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `agents-${theme}`);
    const dialog = shot.getByRole("dialog");
    const closeResult = async () => {
      await dialog.getByRole("button", { name: "Close", exact: true }).last().click();
      await expect(dialog).toHaveCount(0);
    };
    await shot.getByRole("button", { name: "Sync…", exact: true }).click();
    await expect(dialog.getByText("Write 1 agent to 4 clients")).toBeVisible();
    await expect(dialog.getByText(/cursor: 'tools' field not supported/)).toBeVisible();
    if (theme === "light") await guiShot(shot, "agents-plan-light");
    await dialog.getByRole("button", { name: "Sync", exact: true }).click();
    await expect(dialog.getByText("Wrote 1 agent to 4 clients")).toBeVisible();
    await closeResult();
    await expect(outputs.getByText("In sync")).toHaveCount(4);
    await shot.getByRole("button", { name: "Clean outputs…", exact: true }).click();
    await expect(dialog.getByText("Remove 4 synced agent files")).toBeVisible();
    await dialog.getByRole("textbox").fill("clean agent");
    await expect(
      dialog.getByRole("button", { name: "Remove", exact: true }),
    ).toBeDisabled();
    if (theme === "light") await guiShot(shot, "agents-clean-light");
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);

    await shot.getByRole("tab", { name: "Styles" }).click();
    await expect(shot.getByText("No output styles yet")).toBeVisible();
    await shot.evaluate(() => document.fonts.ready);
    if (theme === "light") await guiShot(shot, "styles-empty-light");
    await shot.getByRole("button", { name: "Create your first style" }).click();
    await dialog.getByRole("textbox").fill("terse");
    await dialog.getByRole("button", { name: "Preview", exact: true }).click();
    await shot
      .getByRole("dialog", { name: /Create style terse/ })
      .getByRole("button", { name: "Create", exact: true })
      .click();
    await expect(
      dialog.getByText("Created the style 'terse' from the template"),
    ).toBeVisible();
    await closeResult();
    await shot.getByRole("button", { name: "Sync…", exact: true }).click();
    await expect(dialog.getByText("Write 1 style to 2 native clients")).toBeVisible();
    await dialog.getByRole("button", { name: "Sync", exact: true }).click();
    await expect(dialog.getByText("Wrote 1 style to 2 native clients")).toBeVisible();
    await closeResult();
    await shot.getByRole("button", { name: "Apply terse to other clients" }).click();
    await expect(
      dialog.getByText("Apply 'terse' as an always-on rule in 4 clients"),
    ).toBeVisible();
    await dialog.getByRole("button", { name: "Apply", exact: true }).click();
    await expect(
      dialog.getByText("Applied 'terse' as an always-on rule in 4 clients"),
    ).toBeVisible();
    await closeResult();
    await expect(shot.getByText("Active", { exact: true })).toBeVisible();
    await expect(shot.getByRole("table")).toBeVisible();
    await shot.evaluate(() => document.fonts.ready);
    await guiShot(shot, `styles-${theme}`);
    expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
    await shot.close();
  }
  for (const theme of ["light", "dark"]) {
    const shot = await context.newPage();
    await shot.addInitScript((choice) => {
      localStorage.setItem("toolport-theme", choice);
    }, theme);
    await shot.setViewportSize({ width: 1280, height: 800 });
    await watch(shot);
    await shot.goto(`${baseURL}/fixtures/`);
    await contextScreen(shot, theme);
    expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
    await shot.close();
  }
  for (const theme of ["light", "dark"]) {
    const shot = await context.newPage();
    await shot.addInitScript((choice) => {
      localStorage.setItem("toolport-theme", choice);
    }, theme);
    await shot.setViewportSize({ width: 1280, height: 800 });
    await watch(shot);
    await shot.goto(`${baseURL}/fixtures/`);
    await contextTabsScreen(shot, theme);
    expect((await shot.evaluate(() => window.toolportFixture)).missing).toEqual([]);
    await shot.close();
  }
  for (const theme of ["light", "dark"]) {
    const shot = await context.newPage();
    await shot.addInitScript((choice) => {
      localStorage.setItem("toolport-theme", choice);
    }, theme);
    await shot.setViewportSize({ width: 1280, height: 800 });
    await watch(shot);
    await shot.goto(`${baseURL}/fixtures/`);
    await tasksScreen(shot, theme);
    await shot.close();
  }
  expect(errors).toEqual([]);
  await page.goto(`${baseURL}/fixtures/?logos`);
  await expect(page.getByText("Dark logo fixture")).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await expect
    .poll(() =>
      page
        .locator("img")
        .evaluateAll((images) =>
          images.every((img) => img.complete && img.naturalWidth > 0),
        ),
    )
    .toBe(true);
  // Wait for CSS mask assets too, so screenshots do not capture blank logos.
  await page.evaluate(async () => {
    await Promise.all(
      [...document.querySelectorAll("[style]")].map(async (element) => {
        const match = getComputedStyle(element).maskImage.match(/^url\("?(.*?)"?\)$/);
        if (!match) return;
        const image = new Image();
        image.src = match[1];
        await image.decode();
      }),
    );
  });
  await page.screenshot({ path: path.join(output, "logos.png"), fullPage: true });
  expect(errors).toEqual([]);
  console.log(`Browser smoke passed. Screenshots: ${output}`);
} catch (error) {
  if (page) {
    await page.screenshot({ path: path.join(output, "failure.png") }).catch(() => {});
    await writeFile(path.join(output, "failure.html"), await page.content()).catch(
      () => {},
    );
  }
  console.error(`Browser smoke failed. Artifacts: ${output}`);
  throw error;
} finally {
  await writeFile(path.join(output, "errors.json"), JSON.stringify(errors, null, 2));
  await context?.tracing.stop({ path: path.join(output, "trace.zip") });
  await browser?.close();
  await server.close();
}
