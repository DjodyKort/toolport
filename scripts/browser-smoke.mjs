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
