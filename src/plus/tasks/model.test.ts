import { describe, expect, it } from "vitest";
import {
  blankForm,
  blankStep,
  cronText,
  formFromTask,
  formatDuration,
  looksLikeTask,
  refreshTaskFor,
  slugFromPath,
  taskFromForm,
  taskState,
  triggerChips,
} from "./model";
import { draftCleanup, loginTasks, nightlyReport, portalToken } from "./fixtures";
import { createTasksWorld } from "./world";

describe("task model", () => {
  it("words the simple cron shapes and leaves the others alone", () => {
    expect(cronText("0 8 * * *")).toBe("Every day at 08:00");
    expect(cronText("30 16 * * 5")).toBe("Fridays at 16:30");
    expect(cronText("*/5 * * * *")).toBe("*/5 * * * *");
    expect(cronText("not a cron")).toBe("not a cron");
  });

  it("formats durations", () => {
    expect(formatDuration(null)).toBe("—");
    expect(formatDuration(0)).toBe("0 ms");
    expect(formatDuration(2100)).toBe("2.1 s");
    expect(formatDuration(41000)).toBe("41 s");
    expect(formatDuration(185000)).toBe("3 min 5 s");
  });

  it("round-trips a real definition through the form without losing a field", () => {
    for (const task of [portalToken, nightlyReport, draftCleanup]) {
      const built = taskFromForm(formFromTask(task));
      expect(built.errors).toEqual([]);
      expect(built.task).toEqual(task);
    }
  });

  it("says what is wrong with a form before anything is sent", () => {
    const empty = taskFromForm(blankForm());
    expect(empty.task).toBeNull();
    expect(empty.errors.join("|")).toMatch(/id uses 1 to 64/);
    const form = blankForm();
    form.id = "ok-id";
    form.title = "T";
    form.steps = [
      {
        ...blankStep("secret-set"),
        id: "a",
        title: "A",
        server: "s",
        key: "K",
        from: "v",
      },
      { ...blankStep("mcp"), id: "a", title: "B", server: "s", tool: "t", args: "{oops" },
    ];
    form.secrets = "not-a-secret";
    form.schedule = true;
    form.cron = "0 8";
    const errors = taskFromForm(form).errors.join("|");
    expect(errors).toMatch(/not a secret as server\/KEY/);
    expect(errors).toMatch(/Step ids must be different/);
    expect(errors).toMatch(/arguments are not valid JSON/);
    expect(errors).toMatch(/five fields/);
  });

  it("keeps a secret step to names: the form has no value and the file carries none", () => {
    const built = taskFromForm(formFromTask(portalToken));
    const text = JSON.stringify(built.task);
    expect(text).toContain("API_KEY");
    expect(Object.keys(blankStep("secret-set"))).not.toContain("value");
  });

  it("derives the state and the trigger chips of a listed task", () => {
    const { reply } = createTasksWorld();
    const rows = (
      reply(["task", "ls", "--all"]) as { tasks: Parameters<typeof taskState>[0][] }
    ).tasks;
    const by = (id: string) => rows.find((row) => row.id === id)!;
    expect(taskState(by("portal-token")).label).toBe("Needs you");
    expect(taskState(by("draft-cleanup")).label).toBe("Off");
    expect(taskState(by("nightly-report")).label).toBe("Scheduled");
    expect(triggerChips(by("portal-token").triggers).map((c) => c.label)).toEqual([
      "Button",
      "CLI",
      "Claude may ask",
      "Login fails: srv-alpha",
    ]);
  });

  it("finds the task that renews the login of a server", () => {
    const tasks = loginTasks.tasks;
    expect(refreshTaskFor(tasks, "srv-erp")?.id).toBe("erp-token");
    expect(refreshTaskFor(tasks, "srv-corp")).toBeNull();
    expect(refreshTaskFor(null, "srv-erp")).toBeNull();
    expect(refreshTaskFor([{ ...tasks[0], enabled: false }], "srv-erp")).toBeNull();
  });

  it("makes an id from a command path and spots login-like names", () => {
    expect(slugFromPath("/home/me/.claude/commands/Renew Login.md")).toBe("renew-login");
    expect(slugFromPath("C:\\cmd\\a_b.md")).toBe("a-b");
    expect(looksLikeTask("refresh-token")).toBe(true);
    expect(looksLikeTask("fix-pr")).toBe(false);
  });
});

describe("task world", () => {
  it("advances a run beat by beat, waits at a needs-you step and ends on resume", () => {
    const world = createTasksWorld({ runs: [] });
    const started = world.reply(["task", "run", "portal-token"]) as {
      run: { id: string };
    };
    const read = () =>
      (
        world.reply(["task", "history", "--run", started.run.id]) as {
          run: { status: string; steps: Array<{ status: string }> };
        }
      ).run;
    expect(read().status).toBe("waiting");
    expect(read().steps[0].status).toBe("waiting");
    world.reply(["task", "resume", started.run.id]);
    for (let i = 0; i < 12 && read().status !== "ok"; i += 1);
    expect(read().status).toBe("ok");
    expect(world.state.written).toEqual(["srv-alpha/API_KEY"]);
  });

  it("changes the next read after an applied write and not after a preview", () => {
    const world = createTasksWorld();
    const file = JSON.stringify({ ...draftCleanup, id: "extra" });
    world.reply(["task", "add", "extra", "--file", "/dev/stdin", "--dry-run"], file);
    const count = () =>
      (world.reply(["task", "ls", "--all"]) as { tasks: unknown[] }).tasks.length;
    const before = count();
    world.reply(["task", "add", "extra", "--file", "/dev/stdin"], file);
    expect(count()).toBe(before + 1);
  });
});
