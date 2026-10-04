import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CtlResult, CtlRunOptions } from "../bridge/ctl";
import { JobProgress } from "./JobProgress";
import { useCtlJob } from "./useCtlJob";

const { runCtl } = vi.hoisted(() => ({ runCtl: vi.fn() }));
vi.mock("../bridge/ctl", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../bridge/ctl")>()),
  runCtl,
}));
vi.mock("@/lib/toast", () => ({ toastError: vi.fn() }));

const CANARY = "CANARY-job-progress-91c4";

function result(over: Partial<CtlResult> = {}): CtlResult {
  return {
    job: "job-1",
    exitCode: 0,
    signal: null,
    cancelled: false,
    envelope: {
      ok: true,
      command: "auth login",
      schemaVersion: 1,
      data: { signedIn: true },
    },
    parseError: null,
    stderr: [],
    truncated: false,
    ...over,
  };
}

interface Fake {
  argv: string[];
  options: CtlRunOptions;
  finish: (value: CtlResult) => void;
  fail: (error: Error) => void;
  cancel: ReturnType<typeof vi.fn>;
}
let fake: Fake;

beforeEach(() => {
  runCtl.mockReset();
  runCtl.mockImplementation((argv: string[], options: CtlRunOptions) => {
    let finish!: (value: CtlResult) => void;
    let fail!: (error: Error) => void;
    const done = new Promise<CtlResult>((resolve, reject) => {
      finish = resolve;
      fail = reject;
    });
    const cancel = vi.fn(async () => finish(result({ cancelled: true, envelope: null })));
    fake = { argv, options, finish, fail, cancel };
    return { id: Promise.resolve("job-1"), result: done, cancel };
  });
});

function Harness({ secret, doneLabel }: { secret?: string; doneLabel?: string }) {
  const job = useCtlJob();
  return (
    <>
      <button
        onClick={() =>
          void job.start(["auth", "login", "alpha"], { stdinSecret: secret })
        }
      >
        Start
      </button>
      <button onClick={job.reset}>Reset</button>
      <JobProgress
        state={job.state}
        onCancel={() => void job.cancel()}
        title="Signing in"
        doneLabel={doneLabel}
      />
    </>
  );
}

describe("JobProgress", () => {
  it("streams stderr lines while the job runs and offers Cancel", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    expect(screen.getByRole("status")).toHaveTextContent("Signing in…");
    expect(fake.argv).toEqual(["auth", "login", "alpha"]);
    act(() => fake.options.onStderr?.("waiting for the browser"));
    act(() => fake.options.onStderr?.("still waiting"));
    const log = screen.getByRole("log", { name: "Output" });
    expect(log).toHaveTextContent("waiting for the browser");
    expect(log.textContent).toBe("waiting for the browser\nstill waiting");
    expect(screen.getByRole("button", { name: /cancel/i })).toBeEnabled();
  });

  it("cancels the job and reports the cancel instead of an error", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(fake.cancel).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(/cancelled/i),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /cancel/i })).not.toBeInTheDocument();
  });

  it("disables Cancel while the cancel is on its way", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    fake.cancel.mockImplementation(() => new Promise(() => {}));
    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(screen.getByRole("button", { name: /cancel/i })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Cancelling…");
  });

  it("shows the data of a successful envelope as done", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    act(() => fake.finish(result()));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Done"));
    expect(screen.getByText("Signed in")).toBeInTheDocument();
    expect(screen.getByText("yes")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /cancel/i })).not.toBeInTheDocument();
  });

  it("shows the code and message of a failed envelope", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    act(() =>
      fake.finish(
        result({
          exitCode: 1,
          envelope: {
            ok: false,
            command: "auth login",
            schemaVersion: 1,
            error: { code: "login_failed", message: "the provider refused the sign-in" },
          },
        }),
      ),
    );
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Failed");
    expect(alert).toHaveTextContent("login_failed");
    expect(alert).toHaveTextContent("the provider refused the sign-in");
  });

  it("explains a run without an envelope and a bridge that threw", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    act(() => fake.finish(result({ exitCode: 3, envelope: null })));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "toolportctl exited with 3",
    );

    await user.click(screen.getByRole("button", { name: "Start" }));
    act(() => fake.fail(new Error("--home is not allowed through the bridge")));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("--home is not allowed"),
    );
  });

  it("lifts the address a sign-in prints and lets the user copy it", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    act(() =>
      fake.options.onStderr?.(
        "open https://mcp.example.invalid/authorize?x=1 to approve",
      ),
    );
    expect(
      screen.getByText("https://mcp.example.invalid/authorize?x=1"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Copy address" }));
    expect(await navigator.clipboard.readText()).toBe(
      "https://mcp.example.invalid/authorize?x=1",
    );
  });

  it("hands a secret to the bridge's stdin and keeps it out of everything the screen shows", async () => {
    const user = userEvent.setup();
    const { container } = render(<Harness secret={CANARY} />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    expect(fake.options.stdinSecret).toBe(CANARY);
    expect(fake.argv.join(" ")).not.toContain(CANARY);
    act(() => fake.finish(result()));
    await screen.findByText("Done");
    expect(container.innerHTML).not.toContain(CANARY);
  });

  it("names a successful result with the label it is given", async () => {
    const user = userEvent.setup();
    render(<Harness doneLabel="Preview ready" />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    act(() => fake.finish(result()));
    expect(await screen.findByText("Preview ready")).toBeInTheDocument();
    expect(screen.queryByText("Done")).not.toBeInTheDocument();
  });

  it("forgets a run on reset and ignores a result that arrives late", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    await user.click(screen.getByRole("button", { name: "Reset" }));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(fake.cancel).toHaveBeenCalledTimes(1);
    act(() => fake.finish(result()));
    await Promise.resolve();
    expect(screen.queryByText("Done")).not.toBeInTheDocument();
  });

  it("cancels a job still running when the screen goes away", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Start" }));
    const { cancel } = fake;
    unmount();
    expect(cancel).toHaveBeenCalledTimes(1);
  });
});
