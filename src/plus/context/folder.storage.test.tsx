import { afterEach, describe, expect, it } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useFolderChoice, useRecentFolders } from "./folder";

describe("the remembered folders", () => {
  afterEach(() => window.localStorage.clear());

  it("heal a fragment of a command that an earlier version stored as a folder", () => {
    window.localStorage.setItem(
      "toolport.context.recent-folders",
      JSON.stringify([
        "--cwd '/Users/me/Zuyd Hogeschool/Stage/",
        "/Users/me/Zuyd Hogeschool/Stage",
        "/Users/me/other",
        42,
      ]),
    );
    const { result } = renderHook(() => useRecentFolders());
    expect(result.current.recent).toEqual([
      "/Users/me/Zuyd Hogeschool/Stage",
      "/Users/me/other",
    ]);
    const choice = renderHook(() => useFolderChoice());
    expect(choice.result.current.folder).toBe("/Users/me/Zuyd Hogeschool/Stage");
  });

  it("keep a folder once, however it was pasted", () => {
    const { result } = renderHook(() => useRecentFolders());
    act(() => result.current.remember("'/Users/me/a b/'"));
    act(() => result.current.remember("/Users/me/a b"));
    expect(result.current.recent).toEqual(["/Users/me/a b"]);
  });
});
