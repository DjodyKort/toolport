import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { closeResult, confirmResult, mountContext, review } from "./e2e";
import { FOLDER } from "./tabsKit";
import { createBridge, wire, type Bridge } from "./testkit";

/** The tabs This folder and Profiles walked the way a person uses them, against the world that
 * changes: a preview never changes it, an apply does, and the next read shows it. Each test is
 * named by the parity actions it proves (`src/plus/gui-parity.json`). */
let bridge: Bridge;
beforeEach(() => {
  window.localStorage.clear();
  bridge = createBridge({ world: true });
  wire({ invoke, listen }, bridge);
});
afterEach(() => {
  expect(bridge.missing).toEqual([]);
  expect(bridge.ran().some((line) => /--home|--reveal|secret/.test(line))).toBe(false);
  expect(bridge.stdins()).toEqual([]);
});

const status = () =>
  (
    bridge.world!.run(["context", "bundle", "status", "--cwd", FOLDER]) as {
      applied: { bundle: string } | null;
    }
  ).applied;

async function showFolder(user: UserEvent) {
  await screen.findByRole("group", { name: "Skill list budget" });
  await user.type(screen.getByLabelText("Folder"), FOLDER);
  await user.click(screen.getByRole("button", { name: "Show" }));
  await waitFor(() =>
    expect(screen.getByText(FOLDER, { selector: "code" })).toBeVisible(),
  );
  await screen.findByRole("group", { name: "Skill list budget" });
}

const numbers = () =>
  screen.getByRole("group", { name: "What loads, in numbers" }).textContent;
const summary = () =>
  within(screen.getByRole("group", { name: "What loads, in numbers" }));

describe("Context tabs against the changing world", () => {
  it("context.measure, context.loads: measure: confirm, progress, result", async () => {
    const user = mountContext("here");
    await showFolder(user);
    const loads = `context loads --cwd ${FOLDER} --measured`;
    const argv = `context measure --cwd ${FOLDER} --yes`;
    await user.click(screen.getByRole("button", { name: "Measure for real…" }));
    const box = await review(/Measure what Claude really loads here\?/);
    expect(box.getByText(/spends model tokens/)).toBeVisible();
    expect(bridge.ran()).not.toContain(argv);
    await user.click(box.getByRole("button", { name: "Measure" }));
    const result = within(await screen.findByRole("region", { name: "Measured" }));
    expect(result.getByText(/Claude Code 2\.1\.289/)).toBeVisible();
    expect(bridge.ran()).toContain(argv);
    await closeResult(user);
    await waitFor(() => expect(bridge.count(loads)).toBe(2));
    expect(
      await summary().findByText(/first request · Claude Code 2\.1\.289/),
    ).toBeVisible();
  });

  it("context.measure-without: measures one plugin out, and shows the saving only from the run", async () => {
    const user = mountContext("here");
    await showFolder(user);
    const plugins = within(screen.getByRole("region", { name: "Plugins" }));
    await user.click(plugins.getByRole("button", { name: "Measure without it…" }));
    const box = await review(/Measure without kit@market\?/);
    expect(bridge.ran().some((line) => line.startsWith("context measure"))).toBe(false);
    await user.click(box.getByRole("button", { name: "Measure" }));
    expect(bridge.ran()).toContain(
      `context measure --cwd ${FOLDER} --without plugin:kit@market --yes`,
    );
    const savings = within(await screen.findByRole("list", { name: "Measured savings" }));
    expect(savings.getAllByRole("listitem").length).toBeGreaterThan(0);
    await closeResult(user);
  });

  it("context.use, context.bundle.apply, context.bundle.undo, context.bundle.status, context.bundle.ls: apply a profile: plan, confirm, result, undo", async () => {
    const user = mountContext("here");
    await showFolder(user);
    const before = numbers();
    expect(status()).toBeNull();

    await user.click(screen.getByRole("tab", { name: "Profiles" }));
    await screen.findByRole("region", { name: "Profile acme-dev" });
    await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Apply profile acme-dev to a folder" }),
    );
    const field = form.getByLabelText("Folder") as HTMLInputElement;
    if (!field.value) await user.type(field, FOLDER);
    await user.click(form.getByRole("button", { name: "Review the plan" }));
    const box = await review(/Apply profile acme-dev to clients\/acme-erp\?/);
    expect(box.getByText(/Apply bundle acme-dev in/)).toBeVisible();
    expect(box.getByRole("list", { name: "Changes" })).toBeVisible();
    expect(status()).toBeNull();
    const result = await confirmResult(
      user,
      /Apply profile acme-dev to clients\/acme-erp\?/,
      "Apply profile",
    );
    expect(result.getByText(/toolportctl context use --none --cwd/)).toBeVisible();
    await closeResult(user);
    expect(status()).toMatchObject({ bundle: "acme-dev" });
    const ran = bridge.ran();
    expect(ran.indexOf(`context use acme-dev --cwd ${FOLDER} --dry-run`)).toBeLessThan(
      ran.indexOf(`context use acme-dev --cwd ${FOLDER}`),
    );

    await user.click(screen.getByRole("tab", { name: "This folder" }));
    await showFolder(user);
    await waitFor(() => expect(numbers()).not.toBe(before));
    await user.click(screen.getByRole("tab", { name: "Profiles" }));
    await waitFor(() =>
      expect(
        within(screen.getByRole("region", { name: "Profile acme-dev" })).getAllByText(
          /clients\/acme-erp/,
        ).length,
      ).toBeGreaterThan(0),
    );
    await user.click(
      within(screen.getByRole("region", { name: "Profile acme-dev" })).getByRole(
        "button",
        {
          name: "Undo…",
        },
      ),
    );
    await confirmResult(user, /Undo profile acme-dev in clients\/acme-erp\?/, "Undo");
    await closeResult(user);
    expect(status()).toBeNull();
    expect(bridge.ran()).toContain(`context bundle undo --cwd ${FOLDER}`);
    await user.click(screen.getByRole("tab", { name: "This folder" }));
    await showFolder(user);
    await waitFor(() => expect(numbers()).toBe(before));
  });

  it("context.bundle.apply: a profile without a server set goes through context bundle apply", async () => {
    const user = mountContext("profiles");
    await screen.findByRole("region", { name: "Profile acme-dev" });
    await user.click(screen.getByRole("button", { name: /^default/ }));
    await screen.findByRole("region", { name: "Profile default" });
    await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Apply profile default to a folder" }),
    );
    await user.type(form.getByLabelText("Folder"), `${FOLDER}{Enter}`);
    await confirmResult(
      user,
      /Apply profile default to clients\/acme-erp\?/,
      "Apply profile",
    );
    await closeResult(user);
    expect(status()).toMatchObject({ bundle: "default" });
    expect(bridge.ran().some((line) => line.startsWith("context use default"))).toBe(
      false,
    );
  });

  it("context.compose: shows the composed text of the folder from the layers that match", async () => {
    const user = mountContext("here");
    await showFolder(user);
    const parts = await screen.findByRole("list", { name: "Composed parts" });
    expect(within(parts).getByText("CLAUDE.local.md")).toBeVisible();
    expect(bridge.ran()).toContain(`context compose --cwd ${FOLDER}`);
  });
});
