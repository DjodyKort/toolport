// Build toolportctl and toolport-selfmcp from plus/bins and stage them where Tauri's
// `externalBin` bundler expects them, next to the gateway:
// `src-tauri/binaries/<name>-<target-triple><ext>`. Runs from the fork overlay's
// `beforeBuildCommand`, in the shape of scripts/prepare-sidecar.mjs.
//
// Builds for the host triple. Set CONDUIT_SIDECAR_TARGET to cross-build for another
// target (the same variable prepare-sidecar.mjs reads). Pass `--debug` for a debug build.
// plus/ is its own workspace. PLUS_SIDECAR_TARGET_DIR sets its target dir (default plus/target,
// which Prettier and ESLint would scan; scripts/fork/build-app.sh points it into ~/.cache).
import { execSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { join, resolve } from "node:path";

const PLUS_SIDECARS = ["toolportctl", "toolport-selfmcp"];

function hostTriple() {
  const out = execSync("rustc -vV", { encoding: "utf8" });
  const line = out.split("\n").find((l) => l.startsWith("host:"));
  if (!line) throw new Error("could not determine host target triple from `rustc -vV`");
  return line.split(":")[1].trim();
}

const windowsMsvc = (triple) => /windows-msvc$/.test(triple);

function main() {
  const debug = process.argv.includes("--debug");
  const profile = debug ? "debug" : "release";
  const ext = process.platform === "win32" ? ".exe" : "";
  const requested = process.env.CONDUIT_SIDECAR_TARGET || "";
  const triple = requested || hostTriple();
  const targetDir = resolve(
    process.env.PLUS_SIDECAR_TARGET_DIR || join("plus", "target"),
  );
  const staticCrt = windowsMsvc(triple);
  const env = staticCrt
    ? {
        ...process.env,
        RUSTFLAGS: [process.env.RUSTFLAGS, "-C target-feature=+crt-static"]
          .filter(Boolean)
          .join(" "),
      }
    : process.env;

  console.log(
    `[plus-sidecar] building ${PLUS_SIDECARS.join(", ")} (${profile}) for ${triple}`,
  );
  const bins = PLUS_SIDECARS.map((name) => `--bin ${name}`).join(" ");
  execSync(
    `cargo build --locked ${debug ? "" : "--release "}${requested ? `--target ${requested} ` : ""}--manifest-path plus/Cargo.toml ${bins}`,
    { stdio: "inherit", env: { ...env, CARGO_TARGET_DIR: targetDir } },
  );

  const destDir = join("src-tauri", "binaries");
  mkdirSync(destDir, { recursive: true });
  for (const name of PLUS_SIDECARS) {
    const src = join(
      targetDir,
      ...(requested ? [requested] : []),
      profile,
      `${name}${ext}`,
    );
    if (!existsSync(src)) throw new Error(`built ${name} not found at ${src}`);
    if (staticCrt && readFileSync(src).includes("VCRUNTIME140.dll", 0, "latin1"))
      throw new Error(`${src} still imports VCRUNTIME140.dll`);
    const dest = join(destDir, `${name}-${triple}${ext}`);
    rmSync(dest, { force: true });
    copyFileSync(src, dest);
    if (process.platform !== "win32") chmodSync(dest, 0o755);
    console.log(`[plus-sidecar] staged -> ${dest}`);
  }
}

main();
