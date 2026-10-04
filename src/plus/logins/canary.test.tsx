import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { toast } from "sonner";
import { ctlReplyFailure } from "../fixtures/ctlReply";
import { renderTab, wire } from "./harness";
import { createBridge, type Bridge } from "./testkit";

const CANARY = "canary-3f9c1a7e-do-not-leak";
const VAULTED = "fixture-vaulted-value";

let bridge: Bridge;
let watcher: MutationObserver | null = null;
const seen: string[] = [];

beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
  vi.mocked(toast.error).mockClear();
  vi.mocked(toast.success).mockClear();
  seen.length = 0;
  watcher = new MutationObserver(() => {
    seen.push(document.body.innerHTML);
  });
  watcher.observe(document.body, {
    subtree: true,
    childList: true,
    characterData: true,
    attributes: true,
  });
});
afterEach(() => {
  watcher?.disconnect();
  vi.useRealTimers();
});

const toastText = () =>
  JSON.stringify([
    ...vi.mocked(toast).mock.calls,
    ...vi.mocked(toast.error).mock.calls,
    ...vi.mocked(toast.success).mock.calls,
  ]);

const payloadsWithoutStdin = () =>
  mocks.invoke.mock.calls.map(([command, args]) => {
    const rest = { ...(args as Record<string, unknown> | undefined) };
    delete rest.stdinSecret;
    return JSON.stringify([command, rest]);
  });

const stdinOf = () =>
  mocks.invoke.mock.calls.flatMap(([, args]) => {
    const value = (args as { stdinSecret?: unknown } | undefined)?.stdinSecret;
    return typeof value === "string" ? [value] : [];
  });

const nowhere = (needle: string) => {
  expect(document.body.innerHTML).not.toContain(needle);
  expect(seen.filter((html) => html.includes(needle))).toEqual([]);
  expect(toastText()).not.toContain(needle);
  expect(JSON.stringify({ ...localStorage })).not.toContain(needle);
  expect(JSON.stringify({ ...sessionStorage })).not.toContain(needle);
};

async function typeSecret(
  tab: "secrets" | "logins",
  open: (user: ReturnType<typeof renderTab>["user"]) => Promise<void>,
  save = true,
) {
  const view = renderTab(tab);
  await screen.findByRole(tab === "secrets" ? "list" : "table", {
    name: tab === "secrets" ? "Secrets" : "Logins",
  });
  await waitFor(() => expect(screen.queryByText("Checking…")).not.toBeInTheDocument());
  await open(view.user);
  const dialog = await screen.findByRole("dialog", { name: /^(Set|Replace) / });
  const field = within(dialog).getByLabelText("New value") as HTMLInputElement;
  await view.user.type(field, CANARY);
  expect(field.value).toBe(CANARY);
  nowhere(CANARY);
  if (save) {
    await view.user.click(within(dialog).getByRole("button", { name: "Save to vault" }));
    expect(field.value).toBe("");
  }
  return { ...view, dialog, field };
}

describe("leak canary: a typed secret value", () => {
  it("is only in the field until Save, then only in the child's stdin", async () => {
    const { dialog } = await typeSecret("secrets", (user) =>
      user.click(
        screen.getByRole("button", { name: "Set ERP_WEBHOOK_SECRET of acme-erp" }),
      ),
    );
    await within(dialog).findByText("Saved to the vault");
    expect(stdinOf()).toEqual([CANARY]);
    expect(seen.length).toBeGreaterThan(0);
    const call = bridge.calls.find((c) => c.stdin === CANARY)!;
    expect(call.argv).toEqual(["secret", "set", "srv-erp", "ERP_WEBHOOK_SECRET"]);
    for (const payload of payloadsWithoutStdin()) expect(payload).not.toContain(CANARY);
    expect(JSON.stringify(bridge.calls.map((c) => c.argv))).not.toContain(CANARY);
    nowhere(CANARY);
  });

  it("stays out of the DOM and the payloads when the save fails", async () => {
    bridge.set(
      "secret set srv-erp ERP_API_KEY",
      ctlReplyFailure("vault_locked", "the vault is locked"),
    );
    const { dialog } = await typeSecret("secrets", (user) =>
      user.click(screen.getByRole("button", { name: "Replace ERP_API_KEY of acme-erp" })),
    );
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "the vault is locked",
    );
    for (const payload of payloadsWithoutStdin()) expect(payload).not.toContain(CANARY);
    nowhere(CANARY);
  });

  it("stays out of the DOM when the dialog is cancelled with the value still in the field", async () => {
    const { dialog, user } = await typeSecret(
      "secrets",
      (u) =>
        u.click(screen.getByRole("button", { name: "Replace ERP_API_KEY of acme-erp" })),
      false,
    );
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(stdinOf()).toEqual([]);
    for (const payload of payloadsWithoutStdin()) expect(payload).not.toContain(CANARY);
    nowhere(CANARY);
  });

  it("takes the same path from the Logins tab", async () => {
    const { dialog } = await typeSecret("logins", (user) =>
      user.click(screen.getByRole("button", { name: "Set secret for mail-bridge" })),
    );
    await within(dialog).findByText("Saved to the vault");
    expect(stdinOf()).toEqual([CANARY]);
    for (const payload of payloadsWithoutStdin()) expect(payload).not.toContain(CANARY);
    nowhere(CANARY);
  });
});

describe("leak canary: a stored secret value", () => {
  it("is never read while the lists are shown, and only shown inside the reveal window", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { user } = renderTab("secrets", { fakeTimers: true });
    await screen.findByRole("list", { name: "Secrets" });
    await waitFor(() => expect(screen.queryByText("Checking…")).not.toBeInTheDocument());
    expect(bridge.ran().some((line) => line.includes("--reveal"))).toBe(false);
    nowhere(VAULTED);

    await user.click(
      screen.getByRole("button", { name: "Reveal ERP_API_KEY of acme-erp" }),
    );
    const confirm = await screen.findByRole("dialog", { name: "Reveal ERP_API_KEY?" });
    expect(bridge.ran().some((line) => line.includes("--reveal"))).toBe(false);
    nowhere(VAULTED);

    await user.click(
      within(confirm).getByRole("button", { name: "Reveal for 10 seconds" }),
    );
    const shown = await screen.findByLabelText("Secret value");
    expect(shown).toHaveTextContent(VAULTED);
    expect(document.body.innerHTML.split(VAULTED)).toHaveLength(2);
    expect(shown.innerHTML).toContain(VAULTED);
    expect(toastText()).not.toContain(VAULTED);
    for (const payload of payloadsWithoutStdin()) expect(payload).not.toContain(VAULTED);
    expect(JSON.stringify({ ...localStorage })).not.toContain(VAULTED);

    await vi.advanceTimersByTimeAsync(11000);
    await waitFor(() =>
      expect(screen.queryByLabelText("Secret value")).not.toBeInTheDocument(),
    );
    expect(document.body.innerHTML).not.toContain(VAULTED);
    const lastWithValue = seen.map((html) => html.includes(VAULTED)).lastIndexOf(true);
    expect(lastWithValue).toBeGreaterThanOrEqual(0);
    expect(seen.slice(lastWithValue + 1).some((html) => html.includes(VAULTED))).toBe(
      false,
    );
    expect(seen.at(-1) ?? "").not.toContain(VAULTED);
  });

  it("is dropped when the value dialog is closed early", async () => {
    const { user } = renderTab("secrets");
    await screen.findByRole("list", { name: "Secrets" });
    await waitFor(() => expect(screen.queryByText("Checking…")).not.toBeInTheDocument());
    await user.click(
      screen.getByRole("button", { name: "Reveal MAIL_TOKEN of mail-bridge" }),
    );
    await user.click(
      within(await screen.findByRole("dialog", { name: "Reveal MAIL_TOKEN?" })).getByRole(
        "button",
        { name: "Reveal for 10 seconds" },
      ),
    );
    expect(await screen.findByLabelText("Secret value")).toHaveTextContent(VAULTED);
    await user.click(screen.getByRole("button", { name: "Hide now" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(document.body.innerHTML).not.toContain(VAULTED);
    expect(document.body.textContent).not.toContain(VAULTED);
  });
});
