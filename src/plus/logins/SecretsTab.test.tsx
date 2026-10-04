import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { toast } from "sonner";
import { loginsServerLs, loginsStatus } from "../fixtures/logins";
import { renderTab, wire } from "./harness";
import {
  bridgeDown,
  commandsGolden,
  createBridge,
  ctlReplyFailure,
  deferred,
  type Bridge,
} from "./testkit";

const VALUE = "fixture-vaulted-value";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
  vi.mocked(toast.error).mockClear();
  vi.mocked(toast.success).mockClear();
});
afterEach(() => {
  vi.useRealTimers();
});

const notSet = (server: string, key: string) =>
  ctlReplyFailure("not_found", `${key} is not set for ${server}`);
const isSet = (server: string, key: string) => ({ server, key, set: true });

function withTier(id: string, tier: string) {
  const data = commandsGolden as { commands: Array<{ kind: string; id: string }> };
  bridge.set("commands", {
    ...data,
    commands: data.commands.map((row) =>
      row.kind === "command" && row.id === id ? { ...row, tier } : row,
    ),
  });
}

async function openSecrets(options: { fakeTimers?: boolean } = {}) {
  const view = renderTab("secrets", options);
  const list = await screen.findByRole("list", { name: "Secrets" });
  await waitFor(() =>
    expect(within(list).queryByText("Checking…")).not.toBeInTheDocument(),
  );
  return { ...view, list };
}

const keyRow = (key: string, server: string) =>
  screen.getByRole("group", { name: `${key} of ${server}`, hidden: true });

describe("SecretsTab: the list", () => {
  it("shows a loading skeleton until the registry has answered", async () => {
    const status = deferred<unknown>();
    bridge.set("status", () => status.promise);
    renderTab("secrets");
    expect(
      await screen.findByRole("status", { name: "Loading logins" }),
    ).toBeInTheDocument();
    status.resolve(loginsStatus);
    expect(await screen.findByRole("list", { name: "Secrets" })).toBeInTheDocument();
  });

  it("lists the secret keys of every server, set or unset, and never a value", async () => {
    const { list } = await openSecrets();
    const servers = within(list).getAllByRole("listitem");
    expect(servers).toHaveLength(2);
    expect(servers[0]).toHaveTextContent("acme-erp");
    expect(servers[1]).toHaveTextContent("mail-bridge");
    expect(
      within(keyRow("ERP_API_KEY", "acme-erp")).getByText("set"),
    ).toBeInTheDocument();
    expect(
      within(keyRow("ERP_WEBHOOK_SECRET", "acme-erp")).getByText("unset"),
    ).toBeInTheDocument();
    expect(
      within(keyRow("MAIL_TOKEN", "mail-bridge")).getByText("set"),
    ).toBeInTheDocument();
    expect(list).not.toHaveTextContent("ERP_BASE_URL");
    expect(document.body.textContent).not.toContain(VALUE);
  });

  it("reads presence with one `secret get` per key and never with --reveal", async () => {
    await openSecrets();
    const reads = bridge.ran().filter((line) => line.startsWith("secret"));
    expect(reads.sort()).toEqual([
      "secret get srv-erp ERP_API_KEY",
      "secret get srv-erp ERP_WEBHOOK_SECRET",
      "secret get srv-mail MAIL_TOKEN",
    ]);
  });

  it("offers Replace for a key that is set, Set for one that is not, and Reveal and Remove only when set", async () => {
    await openSecrets();
    const stored = keyRow("ERP_API_KEY", "acme-erp");
    expect(
      within(stored).getByRole("button", { name: "Replace ERP_API_KEY of acme-erp" }),
    ).toBeEnabled();
    expect(
      within(stored).getByRole("button", { name: "Reveal ERP_API_KEY of acme-erp" }),
    ).toBeEnabled();
    expect(
      within(stored).getByRole("button", { name: "Remove ERP_API_KEY of acme-erp" }),
    ).toBeEnabled();
    const empty = keyRow("ERP_WEBHOOK_SECRET", "acme-erp");
    expect(
      within(empty).getByRole("button", { name: "Set ERP_WEBHOOK_SECRET of acme-erp" }),
    ).toBeEnabled();
    expect(
      within(empty).getByRole("button", {
        name: "Reveal ERP_WEBHOOK_SECRET of acme-erp",
      }),
    ).toBeDisabled();
    expect(
      within(empty).getByRole("button", {
        name: "Remove ERP_WEBHOOK_SECRET of acme-erp",
      }),
    ).toBeDisabled();
  });

  it("says Unknown for a key whose presence could not be read", async () => {
    bridge.set(
      "secret get srv-mail MAIL_TOKEN",
      ctlReplyFailure("internal", "vault locked"),
    );
    await openSecrets();
    expect(
      within(keyRow("MAIL_TOKEN", "mail-bridge")).getByText("Unknown"),
    ).toBeInTheDocument();
  });

  it("explains that no server declares a secret and links to the secret commands", async () => {
    bridge.set("server ls", {
      activeProfile: "default",
      servers: loginsServerLs.servers.filter((s) => s.id === "srv-docs"),
    });
    const { user, onOpenCommands } = renderTab("secrets");
    expect(await screen.findByText("No secrets declared")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Open All commands" }));
    expect(onOpenCommands).toHaveBeenCalledWith("secret");
  });

  it("shows the CLI's error with Retry when the registry cannot be read", async () => {
    bridge.set("server ls", ctlReplyFailure("internal", "registry is locked"));
    const { user } = renderTab("secrets");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't read the secrets");
    expect(alert).toHaveTextContent("registry is locked");
    bridge.set("server ls", loginsServerLs);
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(await screen.findByRole("list", { name: "Secrets" })).toBeInTheDocument();
  });

  it("tells a missing toolportctl apart from a failed command", async () => {
    bridge.set("status", bridgeDown("toolportctl was not found next to the app"));
    const { user, onOpenCommands } = renderTab("secrets");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Toolport can't run toolportctl");
    await user.click(within(alert).getByRole("button", { name: "Open doctor" }));
    expect(onOpenCommands).toHaveBeenCalledWith("doctor");
  });

  it("keeps the list and says so when a refresh fails", async () => {
    const { list, user } = await openSecrets();
    bridge.set("status", ctlReplyFailure("internal", "registry is locked"));
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      await screen.findByText(/Could not refresh, showing the last answer/),
    ).toBeInTheDocument();
    expect(list).toBeInTheDocument();
  });
});

describe("logins.secret-set: secret set, as Set and as Replace", () => {
  async function openSet(key: string, server: string, verb: string) {
    const view = await openSecrets();
    await view.user.click(
      within(keyRow(key, server)).getByRole("button", {
        name: `${verb} ${key} of ${server}`,
      }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: `${verb === "Replace" ? "Replace" : "Set"} ${key}`,
    });
    return { ...view, dialog };
  }

  it("Replace shows the plan and the command, and sends the value on stdin only", async () => {
    const { dialog, user } = await openSet("ERP_API_KEY", "acme-erp", "Replace");
    expect(dialog).toHaveTextContent("Replace ERP_API_KEY for acme-erp in the vault");
    expect(dialog).toHaveTextContent("toolportctl secret set srv-erp ERP_API_KEY");
    const field = within(dialog).getByLabelText("New value") as HTMLInputElement;
    expect(field).toHaveAttribute("type", "password");
    expect(within(dialog).getByRole("button", { name: "Save to vault" })).toBeDisabled();
    await user.type(field, "stdin-only-value");
    await user.click(within(dialog).getByRole("button", { name: "Save to vault" }));
    expect(await within(dialog).findByText("Saved to the vault")).toBeInTheDocument();
    const call = bridge.calls.find((c) => c.argv[0] === "secret" && c.argv[1] === "set")!;
    expect(call.argv).toEqual(["secret", "set", "srv-erp", "ERP_API_KEY"]);
    expect(call.stdin).toBe("stdin-only-value");
    expect(JSON.stringify(call.argv)).not.toContain("stdin-only-value");
    expect(field.value).toBe("");
  });

  it("Set on an unset key stores it and the badge flips to set", async () => {
    bridge.after("secret set srv-erp ERP_WEBHOOK_SECRET", {
      "secret get srv-erp ERP_WEBHOOK_SECRET": isSet("srv-erp", "ERP_WEBHOOK_SECRET"),
    });
    const { dialog, user } = await openSet("ERP_WEBHOOK_SECRET", "acme-erp", "Set");
    expect(dialog).toHaveTextContent(
      "Store ERP_WEBHOOK_SECRET for acme-erp in the vault",
    );
    await user.type(within(dialog).getByLabelText("New value"), "hook-value");
    await user.click(within(dialog).getByRole("button", { name: "Save to vault" }));
    await within(dialog).findByText("Saved to the vault");
    await user.click(within(dialog).getAllByRole("button", { name: "Done" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() =>
      expect(
        within(keyRow("ERP_WEBHOOK_SECRET", "acme-erp")).getByText("set"),
      ).toBeInTheDocument(),
    );
    expect(
      within(keyRow("ERP_WEBHOOK_SECRET", "acme-erp")).getByRole("button", {
        name: "Replace ERP_WEBHOOK_SECRET of acme-erp",
      }),
    ).toBeEnabled();
  });

  it("lets the key be changed in the dialog when the server has more than one", async () => {
    const { dialog, user } = await openSet("ERP_API_KEY", "acme-erp", "Replace");
    await user.selectOptions(within(dialog).getByLabelText("Key"), "ERP_WEBHOOK_SECRET");
    expect(
      await screen.findByRole("dialog", { name: "Set ERP_WEBHOOK_SECRET" }),
    ).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("New value"), "v");
    await user.click(within(dialog).getByRole("button", { name: "Save to vault" }));
    await within(dialog).findByText("Saved to the vault");
    expect(bridge.ran()).toContain("secret set srv-erp ERP_WEBHOOK_SECRET");
  });

  it("shows a failed save in the dialog, keeps it open and leaves the badge alone", async () => {
    bridge.set(
      "secret set srv-erp ERP_API_KEY",
      ctlReplyFailure("vault_locked", "the vault is locked"),
    );
    const { dialog, user } = await openSet("ERP_API_KEY", "acme-erp", "Replace");
    await user.type(within(dialog).getByLabelText("New value"), "kept-out-of-dom");
    await user.click(within(dialog).getByRole("button", { name: "Save to vault" }));
    const failed = await within(dialog).findByRole("alert");
    expect(failed).toHaveTextContent("vault_locked");
    expect(failed).toHaveTextContent("the vault is locked");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain("kept-out-of-dom");
    expect(
      within(keyRow("ERP_API_KEY", "acme-erp")).getByText("set"),
    ).toBeInTheDocument();
  });

  it("Cancel closes the dialog without running anything", async () => {
    const { dialog, user } = await openSet("ERP_API_KEY", "acme-erp", "Replace");
    await user.type(within(dialog).getByLabelText("New value"), "never-sent");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(bridge.ran().some((line) => line.startsWith("secret set"))).toBe(false);
    expect(document.body.innerHTML).not.toContain("never-sent");
  });

  it("offers to probe the server once the value is stored", async () => {
    const { dialog, user } = await openSet("ERP_WEBHOOK_SECRET", "acme-erp", "Set");
    await user.type(within(dialog).getByLabelText("New value"), "v");
    await user.click(within(dialog).getByRole("button", { name: "Save to vault" }));
    await within(dialog).findByText("Saved to the vault");
    await user.click(within(dialog).getByRole("button", { name: "Probe acme-erp now" }));
    await waitFor(() =>
      expect(bridge.ran()).toContain("auth probe --server srv-erp --force"),
    );
    expect(
      await screen.findByRole("region", { name: "Probe result" }),
    ).toBeInTheDocument();
  });
});

describe("logins.secret-reveal: secret get --reveal", () => {
  async function openReveal(key = "ERP_API_KEY", server = "acme-erp") {
    const view = await openSecrets({ fakeTimers: true });
    await view.user.click(
      within(keyRow(key, server)).getByRole("button", {
        name: `Reveal ${key} of ${server}`,
      }),
    );
    const dialog = await screen.findByRole("dialog", { name: `Reveal ${key}?` });
    return { ...view, dialog };
  }

  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  it("asks first and shows no value, no input and no --reveal call before the confirmation", async () => {
    const { dialog } = await openReveal();
    expect(within(dialog).queryByRole("textbox")).not.toBeInTheDocument();
    expect(dialog).toHaveTextContent(
      "toolportctl secret get srv-erp ERP_API_KEY --reveal",
    );
    expect(
      within(dialog).getByRole("button", { name: "Reveal for 10 seconds" }),
    ).toBeEnabled();
    expect(bridge.ran().some((line) => line.includes("--reveal"))).toBe(false);
    expect(document.body.textContent).not.toContain(VALUE);
  });

  it("shows the value for ten seconds, counts down and then hides it", async () => {
    const { dialog, user } = await openReveal();
    await user.click(
      within(dialog).getByRole("button", { name: "Reveal for 10 seconds" }),
    );
    const shown = await screen.findByLabelText("Secret value");
    expect(shown).toHaveTextContent(VALUE);
    expect(bridge.ran()).toContain("secret get srv-erp ERP_API_KEY --reveal");
    expect(screen.getByText(/^Hides in \d+ s$/)).toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(5000);
    expect(screen.getByLabelText("Secret value")).toHaveTextContent(VALUE);
    expect(screen.getByText(/^Hides in [1-9] s$/)).toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(5600);
    await waitFor(() =>
      expect(screen.queryByLabelText("Secret value")).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain(VALUE);
  });

  it("Hide now hides it at once", async () => {
    const { dialog, user } = await openReveal();
    await user.click(
      within(dialog).getByRole("button", { name: "Reveal for 10 seconds" }),
    );
    await screen.findByLabelText("Secret value");
    await user.click(screen.getByRole("button", { name: "Hide now" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(document.body.textContent).not.toContain(VALUE);
  });

  it("Escape closes the value dialog and drops the value", async () => {
    const { dialog, user } = await openReveal();
    await user.click(
      within(dialog).getByRole("button", { name: "Reveal for 10 seconds" }),
    );
    await screen.findByLabelText("Secret value");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(document.body.textContent).not.toContain(VALUE);
  });

  it("hides the value when the window is hidden", async () => {
    const { dialog, user } = await openReveal();
    await user.click(
      within(dialog).getByRole("button", { name: "Reveal for 10 seconds" }),
    );
    await screen.findByLabelText("Secret value");
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    try {
      document.dispatchEvent(new Event("visibilitychange"));
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    } finally {
      Reflect.deleteProperty(document, "hidden");
    }
    expect(document.body.textContent).not.toContain(VALUE);
  });

  it("Cancel at the confirmation never asks for the value", async () => {
    const { dialog, user } = await openReveal();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(bridge.ran().some((line) => line.includes("--reveal"))).toBe(false);
  });

  it("follows the policy tier: a destructive tier needs the key typed first", async () => {
    withTier("secret get", "destructive");
    const { dialog, user } = await openReveal();
    const confirm = within(dialog).getByRole("button", { name: "Reveal for 10 seconds" });
    expect(confirm).toBeDisabled();
    await user.type(within(dialog).getByRole("textbox"), "ERP_API");
    expect(confirm).toBeDisabled();
    await user.type(within(dialog).getByRole("textbox"), "_KEY");
    expect(confirm).toBeEnabled();
    expect(bridge.ran().some((line) => line.includes("--reveal"))).toBe(false);
    await user.click(confirm);
    expect(await screen.findByLabelText("Secret value")).toHaveTextContent(VALUE);
  });

  it("reports a reveal the CLI refused and shows no value", async () => {
    bridge.set(
      "secret get srv-erp ERP_API_KEY --reveal",
      ctlReplyFailure("vault_locked", "the vault is locked"),
    );
    const { dialog, user } = await openReveal();
    await user.click(
      within(dialog).getByRole("button", { name: "Reveal for 10 seconds" }),
    );
    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(vi.mocked(toast.error).mock.calls[0][0]).toContain("the vault is locked");
    expect(screen.queryByLabelText("Secret value")).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain(VALUE);
  });

  it("cannot be started for a key that is not set", async () => {
    await openSecrets({ fakeTimers: true });
    expect(
      within(keyRow("ERP_WEBHOOK_SECRET", "acme-erp")).getByRole("button", {
        name: "Reveal ERP_WEBHOOK_SECRET of acme-erp",
      }),
    ).toBeDisabled();
  });
});

describe("logins.secret-remove: secret rm", () => {
  async function openRemove(key = "ERP_API_KEY", server = "acme-erp") {
    const view = await openSecrets();
    await view.user.click(
      within(keyRow(key, server)).getByRole("button", {
        name: `Remove ${key} of ${server}`,
      }),
    );
    const dialog = await screen.findByRole("dialog", { name: `Remove ${key}?` });
    return { ...view, dialog };
  }

  it("needs the key typed, because the policy tier of secret rm is destructive", async () => {
    const { dialog, user } = await openRemove();
    expect(dialog).toHaveTextContent("Remove ERP_API_KEY for acme-erp from the vault");
    expect(dialog).toHaveTextContent("This command has no preview of its own");
    const confirm = within(dialog).getByRole("button", { name: "Remove secret" });
    expect(confirm).toBeDisabled();
    const field = within(dialog).getByRole("textbox");
    await user.type(field, "erp_api_key");
    expect(confirm).toBeDisabled();
    await user.clear(field);
    await user.type(field, "ERP_API_KEY");
    expect(confirm).toBeEnabled();
  });

  it("Enter in the typed field never confirms", async () => {
    const { dialog, user } = await openRemove();
    await user.type(within(dialog).getByRole("textbox"), "ERP_API_KEY{Enter}");
    expect(bridge.ran().some((line) => line.startsWith("secret rm"))).toBe(false);
  });

  it("removes the secret, says so and the badge flips to unset", async () => {
    bridge.after("secret rm srv-erp ERP_API_KEY", {
      "secret get srv-erp ERP_API_KEY": notSet("srv-erp", "ERP_API_KEY"),
    });
    const { dialog, user } = await openRemove();
    await user.type(within(dialog).getByRole("textbox"), "ERP_API_KEY");
    await user.click(within(dialog).getByRole("button", { name: "Remove secret" }));
    await waitFor(() => expect(bridge.ran()).toContain("secret rm srv-erp ERP_API_KEY"));
    expect(toast.success).toHaveBeenCalledWith("Removed ERP_API_KEY for acme-erp");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() =>
      expect(
        within(keyRow("ERP_API_KEY", "acme-erp")).getByText("unset"),
      ).toBeInTheDocument(),
    );
    expect(
      within(keyRow("ERP_API_KEY", "acme-erp")).getByRole("button", {
        name: "Remove ERP_API_KEY of acme-erp",
      }),
    ).toBeDisabled();
  });

  it("is a plain destructive confirmation when the policy says write", async () => {
    withTier("secret rm", "write");
    const { dialog, user } = await openRemove();
    expect(within(dialog).queryByRole("textbox")).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Remove secret" }));
    await waitFor(() => expect(bridge.ran()).toContain("secret rm srv-erp ERP_API_KEY"));
  });

  it("keeps the dialog open and reports a removal the CLI refused", async () => {
    bridge.set(
      "secret rm srv-erp ERP_API_KEY",
      ctlReplyFailure("vault_locked", "the vault is locked"),
    );
    const { dialog, user } = await openRemove();
    await user.type(within(dialog).getByRole("textbox"), "ERP_API_KEY");
    await user.click(within(dialog).getByRole("button", { name: "Remove secret" }));
    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(vi.mocked(toast.error).mock.calls[0][0]).toContain("the vault is locked");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("Cancel removes nothing", async () => {
    const { dialog, user } = await openRemove();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(bridge.ran().some((line) => line.startsWith("secret rm"))).toBe(false);
  });
});

describe("SecretsTab: end to end against the mocked bridge", () => {
  it("sets an unset key, reveals a stored one, then removes it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    bridge.after("secret set srv-erp ERP_WEBHOOK_SECRET", {
      "secret get srv-erp ERP_WEBHOOK_SECRET": isSet("srv-erp", "ERP_WEBHOOK_SECRET"),
    });
    bridge.after("secret rm srv-erp ERP_API_KEY", {
      "secret get srv-erp ERP_API_KEY": notSet("srv-erp", "ERP_API_KEY"),
    });
    const { user } = await openSecrets({ fakeTimers: true });

    await user.click(
      within(keyRow("ERP_WEBHOOK_SECRET", "acme-erp")).getByRole("button", {
        name: "Set ERP_WEBHOOK_SECRET of acme-erp",
      }),
    );
    const setDialog = await screen.findByRole("dialog", {
      name: "Set ERP_WEBHOOK_SECRET",
    });
    await user.type(within(setDialog).getByLabelText("New value"), "e2e-value");
    await user.click(within(setDialog).getByRole("button", { name: "Save to vault" }));
    await within(setDialog).findByText("Saved to the vault");
    await user.click(within(setDialog).getAllByRole("button", { name: "Done" }).at(-1)!);
    await waitFor(() =>
      expect(
        within(keyRow("ERP_WEBHOOK_SECRET", "acme-erp")).getByText("set"),
      ).toBeInTheDocument(),
    );

    await user.click(
      within(keyRow("ERP_API_KEY", "acme-erp")).getByRole("button", {
        name: "Reveal ERP_API_KEY of acme-erp",
      }),
    );
    const reveal = await screen.findByRole("dialog", { name: "Reveal ERP_API_KEY?" });
    await user.click(
      within(reveal).getByRole("button", { name: "Reveal for 10 seconds" }),
    );
    expect(await screen.findByLabelText("Secret value")).toHaveTextContent(VALUE);
    await user.click(screen.getByRole("button", { name: "Hide now" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    await user.click(
      within(keyRow("ERP_API_KEY", "acme-erp")).getByRole("button", {
        name: "Remove ERP_API_KEY of acme-erp",
      }),
    );
    const remove = await screen.findByRole("dialog", { name: "Remove ERP_API_KEY?" });
    await user.type(within(remove).getByRole("textbox"), "ERP_API_KEY");
    await user.click(within(remove).getByRole("button", { name: "Remove secret" }));
    await waitFor(() =>
      expect(
        within(keyRow("ERP_API_KEY", "acme-erp")).getByText("unset"),
      ).toBeInTheDocument(),
    );
    expect(bridge.ran().filter((line) => /^secret (set|rm)|--reveal/.test(line))).toEqual(
      [
        "secret set srv-erp ERP_WEBHOOK_SECRET",
        "secret get srv-erp ERP_API_KEY --reveal",
        "secret rm srv-erp ERP_API_KEY",
      ],
    );
    expect(document.body.textContent).not.toContain(VALUE);
    expect(document.body.textContent).not.toContain("e2e-value");
  });
});
