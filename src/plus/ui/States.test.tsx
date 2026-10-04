import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CtlError, type CtlResult } from "../bridge/ctl";
import { AsyncView, ErrorState, ScreenSkeleton } from "./States";
import type { CtlQuery } from "./useCtlQuery";

const { success, toastError } = vi.hoisted(() => ({
  success: vi.fn(),
  toastError: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: { success } }));
vi.mock("@/lib/toast", () => ({ toastError }));

const writeText = vi.fn();
const failed = (code: string, message: string) =>
  new CtlError(message, code, {} as CtlResult);
const query = (over: Partial<CtlQuery<string[]>>): CtlQuery<string[]> => ({
  status: "ready",
  data: null,
  error: null,
  reload: vi.fn(),
  ...over,
});

beforeEach(() => {
  success.mockReset();
  toastError.mockReset();
  writeText.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
});

describe("ScreenSkeleton", () => {
  it("is a busy status region with the asked number of rows", () => {
    const { container } = render(<ScreenSkeleton rows={3} label="Loading commands" />);
    const region = screen.getByRole("status", { name: "Loading commands" });
    expect(region).toHaveAttribute("aria-busy", "true");
    expect(container.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(3);
  });
});

describe("ErrorState", () => {
  it("shows the code and message of a failed command, with Retry", async () => {
    const onRetry = vi.fn();
    render(
      <ErrorState
        error={failed("not_found", "no such server: acme-erp")}
        title="Couldn't load servers"
        onRetry={onRetry}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Couldn't load servers");
    expect(alert).toHaveTextContent("not_found");
    expect(alert).toHaveTextContent("no such server: acme-erp");
    await userEvent.click(screen.getByRole("button", { name: /retry/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("copies the title, context, code and message and nothing else", async () => {
    render(
      <ErrorState
        error={failed("bridge", "toolportctl exited with 3")}
        title="Couldn't load servers"
        context="server ls"
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /copy diagnostics/i }));
    expect(writeText).toHaveBeenCalledWith(
      "Couldn't load servers\nserver ls\ncode: bridge\ntoolportctl exited with 3",
    );
    await waitFor(() => expect(success).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
  });

  it("says so when the clipboard refuses", async () => {
    writeText.mockRejectedValue(new Error("denied"));
    render(<ErrorState error="plain text failure" />);
    expect(screen.getByRole("alert")).toHaveTextContent("plain text failure");
    await userEvent.click(screen.getByRole("button", { name: /copy diagnostics/i }));
    await waitFor(() => expect(toastError).toHaveBeenCalled());
  });
});

describe("AsyncView", () => {
  const list = (data: string[]) => <p>{data.join(", ")}</p>;

  it("shows a skeleton while the first answer is on its way", () => {
    render(<AsyncView query={query({ status: "loading" })}>{list}</AsyncView>);
    expect(screen.getByRole("status")).toHaveAttribute("aria-busy", "true");
  });

  it("shows the error with Retry instead of a blank page", async () => {
    const reload = vi.fn();
    render(
      <AsyncView
        query={query({ status: "error", error: failed("io", "disk gone"), reload })}
        errorTitle="Couldn't load"
      >
        {list}
      </AsyncView>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("disk gone");
    await userEvent.click(screen.getByRole("button", { name: /retry/i }));
    expect(reload).toHaveBeenCalled();
  });

  it("shows the empty state, and the content when there is data", () => {
    const { rerender } = render(
      <AsyncView
        query={query({ data: [] })}
        isEmpty={(data) => data.length === 0}
        empty={<p>Nothing here yet</p>}
      >
        {list}
      </AsyncView>,
    );
    expect(screen.getByText("Nothing here yet")).toBeInTheDocument();
    rerender(
      <AsyncView
        query={query({ data: ["a", "b"] })}
        isEmpty={(data) => data.length === 0}
        empty={<p>Nothing here yet</p>}
      >
        {list}
      </AsyncView>,
    );
    expect(screen.getByText("a, b")).toBeInTheDocument();
    expect(screen.queryByText("Nothing here yet")).not.toBeInTheDocument();
  });

  it("keeps showing the last data above the error of a failed reload", () => {
    render(
      <AsyncView
        query={query({ status: "error", data: ["kept"], error: failed("io", "offline") })}
      >
        {list}
      </AsyncView>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("offline");
    expect(screen.getByText("kept")).toBeInTheDocument();
  });
});
