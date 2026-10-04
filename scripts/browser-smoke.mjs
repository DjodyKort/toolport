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
  await expect(page.getByText("Not built yet")).toBeVisible();
  await expect(page.getByText(/built by MIG-GUI-3/)).toBeVisible();
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
      await guiShot(shot, "library-light");
    }
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
