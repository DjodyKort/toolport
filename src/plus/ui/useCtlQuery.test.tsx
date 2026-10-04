import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useCtlQuery } from "./useCtlQuery";

const { ctlData } = vi.hoisted(() => ({ ctlData: vi.fn() }));
vi.mock("../bridge/ctl", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../bridge/ctl")>()),
  ctlData,
}));

beforeEach(() => ctlData.mockReset());

describe("useCtlQuery", () => {
  it("goes from loading to ready with the data of the command", async () => {
    ctlData.mockResolvedValue({ serverCount: 3 });
    const { result } = renderHook(() => useCtlQuery<{ serverCount: number }>(["status"]));
    expect(result.current.status).toBe("loading");
    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(result.current.data).toEqual({ serverCount: 3 });
    expect(ctlData).toHaveBeenCalledWith(["status"]);
  });

  it("reports a failure, then recovers on reload and keeps the old data meanwhile", async () => {
    ctlData.mockResolvedValueOnce(["a"]);
    const { result } = renderHook(() => useCtlQuery<string[]>(["server", "ls"]));
    await waitFor(() => expect(result.current.status).toBe("ready"));

    ctlData.mockRejectedValueOnce(new Error("gateway down"));
    act(() => result.current.reload());
    expect(result.current.status).toBe("loading");
    expect(result.current.data).toEqual(["a"]);
    await waitFor(() => expect(result.current.status).toBe("error"));
    expect((result.current.error as Error).message).toBe("gateway down");
    expect(result.current.data).toEqual(["a"]);

    ctlData.mockResolvedValueOnce(["a", "b"]);
    act(() => result.current.reload());
    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(result.current.data).toEqual(["a", "b"]);
    expect(result.current.error).toBeNull();
  });

  it("drops the data of another argv and ignores a late answer", async () => {
    let release!: (value: string) => void;
    ctlData
      .mockResolvedValueOnce("first")
      .mockImplementationOnce(() => new Promise<string>((resolve) => (release = resolve)))
      .mockResolvedValueOnce("third");
    const { result, rerender } = renderHook(
      ({ argv }: { argv: string[] }) => useCtlQuery<string>(argv),
      { initialProps: { argv: ["one"] } },
    );
    await waitFor(() => expect(result.current.data).toBe("first"));
    rerender({ argv: ["two"] });
    expect(result.current.status).toBe("loading");
    expect(result.current.data).toBeNull();
    rerender({ argv: ["three"] });
    await waitFor(() => expect(result.current.data).toBe("third"));
    release("second");
    await Promise.resolve();
    expect(result.current.data).toBe("third");
  });
});
