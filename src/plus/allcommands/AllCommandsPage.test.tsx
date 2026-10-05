import { readFileSync } from "node:fs";
import { join } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  commandsFixture,
  commandsFixtureWithMcpCall,
} from "../fixtures/commandsRegistry";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { guiParity } from "../guiParity";
import { AllCommandsPage } from "./AllCommandsPage";

const CANARY = "CANARY-allcmds-77c1";
const goldenDir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");
const golden = (stem: string) =>
  JSON.parse(readFileSync(join(goldenDir, `${stem}.json`), "utf8")).envelope.data;

type Reply = { data?: unknown; error?: { code: string; message: string } };
type Handler = (call: Call) => Reply | Promise<Reply>;
interface Call {
  job: string;
  argv: string[];
  stdin: string | null;
}

let calls: Call[];
let cancelled: string[];
let handlers: Map<string, Handler>;
let emit: (payload: unknown) => void;
let jobCount: number;

const ran = () => calls.filter((call) => call.argv[0] !== "commands");
const key = (argv: string[]) => argv.join(" ");

function answer(argv: string, handler: Reply | Handler) {
  handlers.set(argv, typeof handler === "function" ? handler : () => handler);
}

beforeEach(() => {
  calls = [];
  cancelled = [];
  handlers = new Map();
  jobCount = 0;
  answer("commands", { data: commandsFixture });
  listen.mockReset();
  listen.mockImplementation(async (_name: string, callback: (e: unknown) => void) => {
    emit = (payload) => callback({ payload });
    return () => {};
  });
  invoke.mockReset();
  const jobs = new Map<string, Call>();
  invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
    if (command === "plus_ctl") {
      const call: Call = {
        job: `job-${++jobCount}`,
        argv: args.argv as string[],
        stdin: (args.stdinSecret as string | null) ?? null,
      };
      calls.push(call);
      jobs.set(call.job, call);
      return call.job;
    }
    if (command === "plus_ctl_result") {
      const call = jobs.get((args as { job: string }).job)!;
      const handler = handlers.get(key(call.argv));
      if (!handler) throw new Error(`no fake reply for ${key(call.argv)}`);
      const reply = await handler(call);
      const wasCancelled = cancelled.includes(call.job);
      return {
        job: call.job,
        exitCode: wasCancelled ? null : reply.error ? 1 : 0,
        signal: null,
        cancelled: wasCancelled,
        envelope: wasCancelled
          ? null
          : {
              ok: !reply.error,
              command: call.argv[0],
              schemaVersion: 1,
              data: reply.data,
              error: reply.error,
            },
        parseError: null,
        stderr: [],
        truncated: false,
      };
    }
    if (command === "plus_ctl_cancel") {
      cancelled.push((args as { job: string }).job);
      return null;
    }
    throw new Error(`unexpected invoke ${command}`);
  });
});

async function open(id: string, user = userEvent.setup()) {
  render(<AllCommandsPage />);
  await screen.findByRole("list", { name: "Commands" });
  await user.click(screen.getByRole("button", { name: new RegExp(`^${id}(?![\\w-])`) }));
  const panel = await screen.findByRole("region", { name: id });
  return {
    user,
    panel,
    button: (name: string) => within(panel).getByRole("button", { name }),
  };
}

const field = (name: RegExp | string) => screen.getByRole("textbox", { name });

describe("the command list", () => {
  it("is generated from the registry, with a tier badge on every row", async () => {
    render(<AllCommandsPage />);
    const list = await screen.findByRole("list", { name: "Commands" });
    const items = within(list).getAllByRole("listitem");
    const runnable = commandsFixture.commands.filter((row) => row.kind === "command");
    expect(items).toHaveLength(runnable.length);
    expect(ran()).toEqual([]);
    expect(calls[0].argv).toEqual(["commands"]);
    const row = within(list).getByRole("button", { name: /^server uninstall/ });
    expect(row).toHaveTextContent("Destructive");
    expect(within(list).getByRole("button", { name: /^status/ })).toHaveTextContent(
      "Read",
    );
    expect(within(list).getByRole("button", { name: /^profile edit/ })).toHaveTextContent(
      "Write",
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      `${runnable.length} of ${runnable.length} commands`,
    );
  });

  it("filters by search words and by group, and says when nothing matches", async () => {
    const user = userEvent.setup();
    render(<AllCommandsPage />);
    const list = await screen.findByRole("list", { name: "Commands" });
    await user.type(
      screen.getByRole("searchbox", { name: "Search commands" }),
      "uninstall",
    );
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByRole("status")).toHaveTextContent(/^1 of /);
    await user.clear(screen.getByRole("searchbox", { name: "Search commands" }));
    await user.click(screen.getByRole("combobox", { name: "Group" }));
    await user.click(screen.getByRole("option", { name: /^server \(3\)/ }));
    expect(
      within(screen.getByRole("list", { name: "Commands" }))
        .getAllByRole("button")
        .map((button) => button.textContent?.match(/^[\w -]+?(?=[A-Z])/)?.[0]?.trim()),
    ).toHaveLength(3);
    await user.type(
      screen.getByRole("searchbox", { name: "Search commands" }),
      "zzz-nothing",
    );
    expect(await screen.findByText("No command matches")).toBeInTheDocument();
  });

  it("starts on the group it is opened for", async () => {
    render(<AllCommandsPage initialGroup="sync" />);
    const list = await screen.findByRole("list", { name: "Commands" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByRole("combobox", { name: "Group" })).toHaveTextContent("sync (2)");
  });

  it("shows an error with Retry when the registry cannot be read, never a blank page", async () => {
    answer("commands", { error: { code: "bridge", message: "toolportctl is missing" } });
    const user = userEvent.setup();
    render(<AllCommandsPage />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not load the command list");
    expect(alert).toHaveTextContent("toolportctl is missing");
    answer("commands", { data: commandsFixture });
    await user.click(screen.getByRole("button", { name: /retry/i }));
    expect(await screen.findByRole("list", { name: "Commands" })).toBeInTheDocument();
  });
});

describe("all-commands.run", () => {
  it("all-commands.run: a read runs at once and shows what it returned", async () => {
    answer("status", { data: { serverCount: 3, activeProfile: "work" } });
    const { user, button } = await open("status");
    await user.click(button("Run"));
    expect(await screen.findByText("Done")).toBeInTheDocument();
    expect(ran().map((call) => call.argv)).toEqual([["status"]]);
    expect(screen.getByText("work")).toBeInTheDocument();
  });

  it("all-commands.run: a write previews with --dry-run, shows the plan, then applies without it", async () => {
    answer("profile edit default --add-server=beta --dry-run", {
      data: golden("profile-edit.preview"),
    });
    answer("profile edit default --add-server=beta", {
      data: { changed: true, backups: [], undo: null },
    });
    const { user, button } = await open("profile edit");
    const run = button("Preview changes");
    expect(run).toBeDisabled();
    expect(screen.getByText(/Still needed: profile is required/)).toBeInTheDocument();
    await user.type(field("profile"), "default");
    await user.type(field("--add-server"), "beta");
    expect(screen.getByLabelText("Command line")).toHaveTextContent(
      "toolportctl profile edit default --add-server=beta",
    );
    await user.click(run);

    const dialog = await screen.findByRole("dialog", { name: "Apply profile edit?" });
    expect(ran().map((call) => call.argv)).toEqual([
      ["profile", "edit", "default", "--add-server=beta", "--dry-run"],
    ]);
    expect(within(dialog).getByRole("region", { name: "Preview" })).toHaveTextContent(
      "beta",
    );
    expect(within(dialog).getByLabelText("Command line")).not.toHaveTextContent(
      "dry-run",
    );
    expect(within(dialog).queryByRole("textbox")).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(ran()).toHaveLength(2));
    expect(ran()[1].argv).toEqual(["profile", "edit", "default", "--add-server=beta"]);
    expect(await screen.findByText("Done")).toBeInTheDocument();
  });

  it("all-commands.run: cancelling the confirmation applies nothing and keeps the plan to review", async () => {
    answer("profile edit default --name=Team --dry-run", {
      data: golden("profile-edit.preview"),
    });
    const { user, button } = await open("profile edit");
    await user.type(field("profile"), "default");
    await user.type(field("--name"), "Team");
    await user.click(button("Preview changes"));
    const dialog = await screen.findByRole("dialog", { name: "Apply profile edit?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(ran()).toHaveLength(1);
    expect(screen.getByRole("region", { name: "Preview result" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Review and apply" }));
    expect(
      await screen.findByRole("dialog", { name: "Apply profile edit?" }),
    ).toBeVisible();
  });

  it("all-commands.run: editing the form after a preview discards the preview", async () => {
    answer("profile edit default --name=A --dry-run", {
      data: golden("profile-edit.preview"),
    });
    const { user, button } = await open("profile edit");
    await user.type(field("profile"), "default");
    await user.type(field("--name"), "A");
    await user.click(button("Preview changes"));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.type(field("--name"), "B");
    expect(
      screen.queryByRole("region", { name: "Preview result" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Review and apply" }),
    ).not.toBeInTheDocument();
    expect(ran()).toHaveLength(1);
  });

  it("all-commands.run: a destructive command needs its phrase typed before it applies", async () => {
    answer("server uninstall acme-erp --dry-run", {
      data: golden("server-uninstall.preview"),
    });
    answer("server uninstall acme-erp", {
      data: { changed: true, backups: [], undo: null },
    });
    const { user, button } = await open("server uninstall");
    await user.type(field("server"), "acme-erp");
    await user.click(button("Preview changes"));

    const dialog = await screen.findByRole("dialog", { name: "Apply server uninstall?" });
    const apply = within(dialog).getByRole("button", { name: "Apply" });
    expect(apply).toBeDisabled();
    expect(apply).toHaveAttribute("data-variant", "destructive");
    const typed = within(dialog).getByRole("textbox", {
      name: /type acme-erp to confirm/i,
    });
    await user.type(typed, "acme-er{Enter}");
    expect(apply).toBeDisabled();
    await user.type(typed, "p{Enter}");
    expect(ran()).toHaveLength(1);
    await user.click(apply);
    await waitFor(() => expect(ran()).toHaveLength(2));
    expect(ran()[1].argv).toEqual(["server", "uninstall", "acme-erp"]);
  });

  it("all-commands.run: a command that previews unless applied is applied with its flag", async () => {
    answer("compression update", { data: { current: "1.0", available: "1.1" } });
    answer("compression update --accept", { data: { installed: "1.1" } });
    const { user, button } = await open("compression update");
    expect(screen.queryByText("--accept")).not.toBeInTheDocument();
    await user.click(button("Preview changes"));
    await user.click(
      within(
        await screen.findByRole("dialog", { name: "Apply compression update?" }),
      ).getByRole("button", { name: "Apply" }),
    );
    await waitFor(() => expect(ran()).toHaveLength(2));
    expect(ran().map((call) => call.argv)).toEqual([
      ["compression", "update"],
      ["compression", "update", "--accept"],
    ]);
  });

  it("all-commands.run: a flag that makes a read change things turns the run into a preview", async () => {
    answer("client import acme", { data: { clients: [] } });
    answer("client import acme --select=a --dry-run", { data: { dryRun: true } });
    const { user, button } = await open("client import");
    await user.type(field("client"), "acme");
    expect(button("Run")).toBeEnabled();
    await user.type(field("--select"), "a");
    expect(button("Preview changes")).toBeEnabled();
    expect(screen.getAllByText("changes things").length).toBeGreaterThan(0);
  });

  it("all-commands.run: a read that costs asks first and shows the exact command line", async () => {
    answer("council ask why", { data: { answers: [] } });
    const { user, button } = await open("council ask");
    await user.type(field("question"), "why");
    await user.click(button("Run…"));
    const dialog = await screen.findByRole("dialog", { name: "Run council ask?" });
    expect(within(dialog).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl council ask why",
    );
    expect(dialog).toHaveTextContent(/paid tokens/);
    expect(ran()).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Run" }));
    await waitFor(() => expect(ran()).toHaveLength(1));
  });

  it("all-commands.run: a secret goes over stdin only, never into argv, the page or a log", async () => {
    answer("secret set acme-erp API_KEY", { data: { changed: true } });
    const { user, button } = await open("secret set");
    await user.type(field("server"), "acme-erp");
    await user.type(field("key"), "API_KEY");
    expect(button("Run…")).toBeDisabled();
    const secret = screen.getByLabelText("Secret value");
    expect(secret).toHaveAttribute("type", "password");
    await user.type(secret, CANARY);
    expect(screen.getByLabelText("Command line")).not.toHaveTextContent(CANARY);
    await user.click(button("Run…"));

    const dialog = await screen.findByRole("dialog", { name: "Run secret set?" });
    expect(dialog).toHaveTextContent(/no preview/i);
    expect(within(dialog).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl secret set acme-erp API_KEY",
    );
    await user.click(within(dialog).getByRole("button", { name: "Run" }));
    await waitFor(() => expect(ran()).toHaveLength(1));

    expect(ran()[0].stdin).toBe(CANARY);
    expect(JSON.stringify(ran()[0].argv)).not.toContain(CANARY);
    await screen.findByText("Done");
    expect(document.body.innerHTML).not.toContain(CANARY);
    expect((screen.getByLabelText("Secret value") as HTMLInputElement).value).toBe("");
    expect(
      JSON.stringify(invoke.mock.calls.filter(([name]) => name !== "plus_ctl")),
    ).not.toContain(CANARY);
  });

  it("all-commands.run: a destructive command with no preview types its phrase, then runs", async () => {
    answer("sync rotate-passphrase --passphrase-stdin", { data: { rotated: true } });
    const { user, button } = await open("sync rotate-passphrase");
    await user.type(screen.getByLabelText("Passphrase"), CANARY);
    await user.click(button("Run…"));
    const dialog = await screen.findByRole("dialog", {
      name: "Run sync rotate-passphrase?",
    });
    const run = within(dialog).getByRole("button", { name: "Run" });
    expect(run).toBeDisabled();
    await user.type(
      within(dialog).getByRole("textbox", {
        name: /type sync rotate-passphrase to confirm/i,
      }),
      "sync rotate-passphrase",
    );
    await user.click(run);
    await waitFor(() => expect(ran()).toHaveLength(1));
    expect(ran()[0]).toMatchObject({
      argv: ["sync", "rotate-passphrase", "--passphrase-stdin"],
      stdin: CANARY,
    });
  });

  it("all-commands.run: shows a failure as such and keeps the form for another try", async () => {
    answer("server ls", {
      error: { code: "registry_unreadable", message: "registry.json is broken" },
    });
    const { user, button } = await open("server ls");
    await user.click(button("Run"));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("registry_unreadable");
    expect(alert).toHaveTextContent("registry.json is broken");
    expect(button("Run")).toBeEnabled();
  });

  it("all-commands.run: shows live stderr and Cancel stops the job", async () => {
    let release: (reply: Reply) => void = () => {};
    answer("status", () => new Promise<Reply>((resolve) => (release = resolve)));
    const { user, button } = await open("status");
    await user.click(button("Run"));
    await waitFor(() => expect(ran()).toHaveLength(1));
    act(() =>
      emit({ job: ran()[0].job, seq: 1, kind: "stderr", line: "checking gateway" }),
    );
    expect(await screen.findByRole("log", { name: "Output" })).toHaveTextContent(
      "checking gateway",
    );
    expect(button("Run")).toBeDisabled();
    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(cancelled).toEqual([ran()[0].job]);
    await act(async () => release({ data: {} }));
    expect(await screen.findByText(/Cancelled/)).toBeInTheDocument();
  });
});

describe("the generated form", () => {
  it("names every field, links its help text and follows the flag's value type", async () => {
    const { user, panel } = await open("server new");
    expect(within(panel).getByRole("textbox", { name: "name" })).toHaveAttribute(
      "aria-required",
      "true",
    );
    const url = within(panel).getByRole("textbox", { name: "--url" });
    expect(url).toHaveAccessibleDescription("Address of a remote server");
    expect(within(panel).getByRole("textbox", { name: "--arg" })).toHaveAttribute(
      "placeholder",
      "Separate with commas",
    );
    const env = within(panel).getByRole("textbox", { name: "--env" });
    expect(env.tagName).toBe("TEXTAREA");
    const transport = within(panel).getByRole("combobox", { name: "--transport" });
    expect(transport).toHaveTextContent("Default");
    await user.click(transport);
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "Default",
      "http",
      "sse",
    ]);
    await user.click(screen.getByRole("option", { name: "sse" }));
    await user.type(within(panel).getByRole("textbox", { name: "name" }), "acme-erp");
    await user.type(url, "https://example.test/mcp");
    await user.type(env, "A=1{Enter}B=2");
    expect(within(panel).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl server new acme-erp --url=https://example.test/mcp --env=A=1 --env=B=2 --transport=sse",
    );
  });

  it("offers a bool flag as a switch and leaves out hidden, sensitive and preview flags", async () => {
    const { user, panel } = await open("skills sync");
    const project = within(panel).getByRole("switch", { name: "--project" });
    expect(project).toHaveAttribute("aria-checked", "false");
    await user.click(project);
    expect(within(panel).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl skills sync --project",
    );
    expect(
      within(panel).queryByRole("switch", { name: "--dry-run" }),
    ).not.toBeInTheDocument();
  });

  it("says when a command takes no input, and why a secret flag is not offered", async () => {
    const { panel } = await open("status");
    expect(within(panel).getByText("This command takes no input.")).toBeInTheDocument();
  });

  it("explains the flags it keeps off the form for a command that reads a secret", async () => {
    const { panel } = await open("sync init");
    const note = within(panel).getByText(/put a secret on a command line/);
    expect(note.textContent).toMatch(/: --passphrase-env$/);
    expect(within(panel).getByLabelText("Passphrase")).toHaveAttribute(
      "type",
      "password",
    );
  });
});

describe("all-commands.terminal", () => {
  it("all-commands.terminal: shows the exact command line to copy and offers no Run", async () => {
    const { user, panel } = await open("compression run");
    expect(
      within(panel).queryByRole("button", { name: /run|preview/i }),
    ).not.toBeInTheDocument();
    expect(within(panel).getByText(/needs a terminal/i)).toBeInTheDocument();
    await user.type(within(panel).getByLabelText("assistant-args"), "--model fast 'a b'");
    const line = within(panel).getByLabelText("Command line");
    expect(line).toHaveTextContent("toolportctl compression run --model fast 'a b'");
    await user.click(within(panel).getByRole("button", { name: "Copy command" }));
    expect(await navigator.clipboard.readText()).toBe(line.textContent);
    expect(ran()).toEqual([]);
  });
});

describe("all-commands.tool", () => {
  beforeEach(() => answer("commands", { data: commandsFixtureWithMcpCall }));

  async function openTool(name: string) {
    const user = userEvent.setup();
    render(<AllCommandsPage />);
    const box = await screen.findByRole("region", { name: "Run a tool" });
    await user.click(within(box).getByRole("combobox", { name: "Tool" }));
    await user.click(screen.getByRole("option", { name }));
    return { user, box };
  }

  it("all-commands.tool: is disabled with a tooltip while the CLI has no mcp call", async () => {
    answer("commands", { data: commandsFixture });
    render(<AllCommandsPage />);
    const box = await screen.findByRole("region", { name: "Run a tool" });
    expect(box).toHaveAttribute("title", expect.stringMatching(/no `mcp call` command/));
    expect(within(box).getByText(/no `mcp call` command/)).toBeInTheDocument();
    expect(within(box).getByRole("combobox", { name: "Tool" })).toBeDisabled();
    expect(within(box).getByRole("textbox")).toBeDisabled();
    expect(within(box).getByRole("button", { name: /run|preview/i })).toBeDisabled();
  });

  it("all-commands.tool: a read tool runs through mcp call with its arguments on stdin", async () => {
    answer("mcp call skills_get --args-stdin", { data: { name: "demo" } });
    const { user, box } = await openTool("skills_get");
    expect(within(box).getByRole("textbox")).toBeEnabled();
    await user.click(within(box).getByRole("textbox"));
    await user.paste('{"name":"demo"}');
    await user.click(within(box).getByRole("button", { name: "Run" }));
    await waitFor(() => expect(ran()).toHaveLength(1));
    expect(ran()[0]).toMatchObject({
      argv: ["mcp", "call", "skills_get", "--args-stdin"],
      stdin: '{"name":"demo"}',
    });
    expect(JSON.stringify(ran()[0].argv)).not.toContain('demo"');
  });

  it("all-commands.tool: a tool with a dry run previews first, then applies with dry_run off", async () => {
    answer("mcp call styles_apply_note --args-stdin", ({ stdin }) => ({
      data: JSON.parse(stdin ?? "{}").dry_run
        ? { dryRun: true, steps: [] }
        : { changed: true, backups: [], undo: null },
    }));
    const { user, box } = await openTool("styles_apply_note");
    await user.click(within(box).getByRole("textbox"));
    await user.paste('{"id":"x"}');
    await user.click(within(box).getByRole("button", { name: "Preview changes" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Apply styles_apply_note?",
    });
    expect(dialog).toHaveTextContent('"id": "x"');
    await user.click(within(dialog).getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(ran()).toHaveLength(2));
    expect(ran().map((call) => JSON.parse(call.stdin ?? "{}"))).toEqual([
      { id: "x", dry_run: true },
      { id: "x", dry_run: false },
    ]);
  });

  it("all-commands.tool: a tier 3 writer without a dry run is confirmed and sends confirm", async () => {
    answer("mcp call skills_edit_body --args-stdin", { data: { changed: true } });
    const { user, box } = await openTool("skills_edit_body");
    await user.click(within(box).getByRole("button", { name: "Run…" }));
    const dialog = await screen.findByRole("dialog", { name: "Run skills_edit_body?" });
    expect(dialog).toHaveTextContent(
      "toolportctl mcp call skills_edit_body --args-stdin",
    );
    await user.click(within(dialog).getByRole("button", { name: "Run" }));
    await waitFor(() => expect(ran()).toHaveLength(1));
    expect(JSON.parse(ran()[0].stdin ?? "{}")).toEqual({ confirm: true });
  });

  it("all-commands.tool: a destructive tool needs its name typed", async () => {
    answer("mcp call skills_git_push --args-stdin", { data: { pushed: true } });
    const { user, box } = await openTool("skills_git_push");
    await user.click(within(box).getByRole("button", { name: "Run…" }));
    const dialog = await screen.findByRole("dialog", { name: "Run skills_git_push?" });
    const run = within(dialog).getByRole("button", { name: "Run" });
    expect(run).toBeDisabled();
    await user.type(within(dialog).getByRole("textbox"), "skills_git_push");
    await user.click(run);
    await waitFor(() => expect(ran()).toHaveLength(1));
  });

  it("all-commands.tool: the real registry offers the 23 tools that have no command and runs one through mcp call", async () => {
    const registry = golden("commands") as typeof commandsFixture;
    const toolOnly = registry.tools.filter((tool) => tool.command === null);
    expect(toolOnly).toHaveLength(23);
    answer("commands", { data: registry });
    answer("mcp call servers_set_mode --args-stdin", { data: { changed: true } });
    const user = userEvent.setup();
    render(<AllCommandsPage />);
    const box = await screen.findByRole("region", { name: "Run a tool" });
    expect(box).not.toHaveAttribute("title");
    expect(within(box).queryByText(/no `mcp call` command/)).not.toBeInTheDocument();
    await user.click(within(box).getByRole("combobox", { name: "Tool" }));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(
      toolOnly.map((tool) => tool.name),
    );
    await user.click(screen.getByRole("option", { name: "servers_set_mode" }));
    await user.click(within(box).getByRole("textbox"));
    await user.paste('{"name":"alpha","mode":"direct"}');
    await user.click(within(box).getByRole("button", { name: "Run…" }));
    const dialog = await screen.findByRole("dialog", { name: "Run servers_set_mode?" });
    await user.click(within(dialog).getByRole("button", { name: "Run" }));
    await waitFor(() => expect(ran()).toHaveLength(1));
    expect(ran()[0].argv).toEqual(["mcp", "call", "servers_set_mode", "--args-stdin"]);
    expect(JSON.parse(ran()[0].stdin ?? "{}")).toEqual({
      name: "alpha",
      mode: "direct",
      confirm: true,
    });
  });

  it("all-commands.tool: refuses arguments that are not one JSON object", async () => {
    const { user, box } = await openTool("skills_get");
    await user.click(within(box).getByRole("textbox"));
    await user.paste("[1, 2]");
    await user.click(within(box).getByRole("button", { name: "Run" }));
    expect(await within(box).findByRole("alert")).toHaveTextContent(/one JSON object/);
    expect(ran()).toEqual([]);
  });
});

describe("catalog.commands", () => {
  const realRegistry = () => golden("commands") as typeof commandsFixture;

  it("catalog.commands: lists the whole registry, the commands command and mcp call included, with a tier badge each", async () => {
    const registry = realRegistry();
    answer("commands", { data: registry });
    render(<AllCommandsPage />);
    const list = await screen.findByRole("list", { name: "Commands" });
    const runnable = registry.commands.filter((row) => row.kind === "command");
    expect(within(list).getAllByRole("listitem")).toHaveLength(runnable.length);
    expect(
      within(list).getByRole("button", { name: /^commands(?![\w-])/ }),
    ).toHaveTextContent("Read");
    expect(within(list).getByRole("button", { name: /^mcp call/ })).toHaveTextContent(
      "Write",
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      `${runnable.length} of ${runnable.length} commands`,
    );
    expect(calls.map((call) => call.argv)).toEqual([["commands"]]);
  });

  it("catalog.commands: picking the commands command offers its form and runs nothing until Run", async () => {
    answer("commands", { data: realRegistry() });
    const { panel, button } = await open("commands");
    expect(within(panel).getByRole("button", { name: "Run" })).toBeEnabled();
    expect(button("Run")).toBeInTheDocument();
    expect(calls).toHaveLength(1);
  });

  it("catalog.commands: the parity manifest points the commands and mcp call rows at this page as a built route of its own", () => {
    const route = guiParity.routes.catalog;
    expect(route).toMatchObject({
      status: "built",
      view: "commands",
      component: "src/plus/allcommands/AllCommandsPage.tsx",
    });
    expect(guiParity.commands.commands).toEqual({
      route: "catalog",
      action: "catalog.commands",
      surface: "screen",
    });
    expect(guiParity.commands["mcp call"]).toEqual({
      route: "catalog",
      action: "catalog.mcp-call",
      surface: "screen",
    });
  });
});

describe("catalog.mcp-call", () => {
  const withRealRegistry = () => answer("commands", { data: golden("commands") });

  async function openRealTool(name: string) {
    withRealRegistry();
    const user = userEvent.setup();
    render(<AllCommandsPage />);
    const box = await screen.findByRole("region", { name: "Run a tool" });
    await user.click(within(box).getByRole("combobox", { name: "Tool" }));
    await user.click(screen.getByRole("option", { name }));
    return { user, box };
  }

  it("catalog.mcp-call: a read tool runs through mcp call with {} on stdin and its answer is shown", async () => {
    answer("mcp call where_am_i --args-stdin", {
      data: golden("mcp-call.where_am_i"),
    });
    const { user, box } = await openRealTool("where_am_i");
    await user.click(within(box).getByRole("button", { name: "Run" }));
    expect(await screen.findByText("Done")).toBeInTheDocument();
    expect(ran()).toHaveLength(1);
    expect(ran()[0]).toMatchObject({
      argv: ["mcp", "call", "where_am_i", "--args-stdin"],
      stdin: "{}",
    });
    expect(screen.getByRole("region", { name: "Run result" })).toHaveTextContent(
      '"dataDir": "<WORLD>/data"',
    );
  });

  it("catalog.mcp-call: a tool that fails is shown with its error code and message, and the box stays usable", async () => {
    answer("mcp call skills_get --args-stdin", {
      error: { code: "invalid_arguments", message: "missing required argument: name" },
    });
    const { user, box } = await openRealTool("skills_get");
    await user.click(within(box).getByRole("button", { name: "Run" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("invalid_arguments");
    expect(alert).toHaveTextContent("missing required argument: name");
    expect(within(box).getByRole("button", { name: "Run" })).toBeEnabled();
    expect(ran()).toHaveLength(1);
  });

  it("catalog.mcp-call: the arguments never reach argv, only stdin", async () => {
    answer("mcp call skills_get --args-stdin", { data: { name: "demo" } });
    const { user, box } = await openRealTool("skills_get");
    await user.click(within(box).getByRole("textbox"));
    await user.paste(`{"name":"${CANARY}"}`);
    await user.click(within(box).getByRole("button", { name: "Run" }));
    await waitFor(() => expect(ran()).toHaveLength(1));
    expect(JSON.stringify(ran()[0].argv)).not.toContain(CANARY);
    expect(ran()[0].stdin).toContain(CANARY);
  });
});
