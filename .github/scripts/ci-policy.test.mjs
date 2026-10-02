import assert from "node:assert/strict";
import test from "node:test";
import { needsRust, requireResults } from "./ci-policy.mjs";

test("frontend-only PRs skip unchanged native code", () => {
  for (const files of [
    ["src/App.tsx"],
    ["public/logo.svg"],
    ["index.html", "vite.config.ts", "tsconfig.app.json"],
  ])
    assert.equal(needsRust("pull_request", files), false);
});

test("native, shared, unknown, empty diffs and main pushes run native checks", () => {
  for (const file of [
    "src-tauri/src/lib.rs",
    "package-lock.json",
    "package.json",
    "scripts/install.sh",
    ".github/workflows/ci.yml",
    "docs/design.md",
    "new-file",
  ])
    assert.equal(needsRust("pull_request", ["src/App.tsx", file]), true);
  assert.equal(needsRust("pull_request", []), true);
  assert.equal(needsRust("push", ["src/App.tsx"]), true);
});

function results(selected) {
  return Object.fromEntries(
    [
      "changes",
      "frontend",
      "installer-script",
      "installer-script-bash",
      "pinned-install-urls",
      "build-test",
      "cross-platform-rust",
    ].map((job) => [
      job,
      {
        result:
          selected === "false" && ["build-test", "cross-platform-rust"].includes(job)
            ? "skipped"
            : "success",
        outputs: job === "changes" ? { rust: selected } : {},
      },
    ]),
  );
}

test("gate accepts successful selected checks and only intentional native skips", () => {
  requireResults(results("true"));
  requireResults(results("false"));
});

test("any failed, canceled, missing or unexpectedly skipped required check blocks", () => {
  for (const selected of ["true", "false"]) {
    for (const job of Object.keys(results(selected))) {
      for (const result of ["failure", "cancelled", undefined, "skipped"]) {
        if (
          selected === "false" &&
          ["build-test", "cross-platform-rust"].includes(job) &&
          result === "skipped"
        )
          continue;
        const needs = results(selected);
        needs[job].result = result;
        assert.throws(() => requireResults(needs));
      }
    }
  }
  assert.throws(() => requireResults(results(undefined)));
  assert.throws(() => requireResults(results("maybe")));
  const needs = results("false");
  needs["build-test"].result = "success";
  assert.throws(() => requireResults(needs));
});
