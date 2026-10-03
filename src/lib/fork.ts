/** Fork switch: Toolport+ never reaches the upstream product's hosted services.
 * Mirrors `brand::FORK_EGRESS_DISABLED` in Rust. Setting
 * `VITE_TOOLPORT_UPSTREAM_EGRESS=1` at build time restores upstream behaviour
 * (the vitest config does, so upstream tests keep covering that code). */
export function forkEgressDisabled(): boolean {
  return import.meta.env.VITE_TOOLPORT_UPSTREAM_EGRESS !== "1";
}

export function isUpstreamEgressUrl(url: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(url.trim());
  } catch {
    return false;
  }
  const host = parsed.hostname.toLowerCase();
  return (
    host === "toolport.app" ||
    host.endsWith(".toolport.app") ||
    (host === "github.com" && parsed.pathname.toLowerCase().startsWith("/btsouth/"))
  );
}
