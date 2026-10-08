import { describe, expect, it } from "vitest";
import { cleanFolder } from "./folder";

describe("cleanFolder", () => {
  it.each([
    ["'/Users/me/Zuyd Hogeschool/Stage/'", "/Users/me/Zuyd Hogeschool/Stage"],
    ['"/Users/me/a b"', "/Users/me/a b"],
    ["/Users/me/Zuyd\\ Hogeschool/Stage", "/Users/me/Zuyd Hogeschool/Stage"],
    ["  /Users/me/plain \n", "/Users/me/plain"],
    ["'/Users/me/half", "/Users/me/half"],
    ["--cwd '/Users/me/Zuyd Hogeschool/Stage/", "/Users/me/Zuyd Hogeschool/Stage"],
    ["--cwd '/Users/me/Zuyd Hogeschool/Stage/'", "/Users/me/Zuyd Hogeschool/Stage"],
    ["--cwd=/Users/me/plain", "/Users/me/plain"],
    ["--cwd /Users/me/Zuyd\\ Hogeschool", "/Users/me/Zuyd Hogeschool"],
    ["/", "/"],
  ])("makes %j plain", (raw, plain) => {
    expect(cleanFolder(raw)).toBe(plain);
  });
});
