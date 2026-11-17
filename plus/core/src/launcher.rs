//! Which stdio launch lines resolve their package or environment before the server can answer.

/// `uv run` syncs the project environment, and may fetch an interpreter and dependencies, before
/// it execs the server; `uv tool run` is `uvx`. Other `uv` subcommands (`sync`, `pip`,
/// `tool install`) are not launches of a server.
pub fn is_uv_download_launcher(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some("run") => true,
        Some("tool") => args.get(1).map(String::as_str) == Some("run"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn test01_uv_run_and_uv_tool_run_are_download_launchers() {
        assert!(is_uv_download_launcher(&args(&[
            "run",
            "--directory",
            "/srv/server",
            "tool"
        ])));
        assert!(is_uv_download_launcher(&args(&["tool", "run", "some-mcp"])));
    }

    #[test]
    fn test02_other_uv_subcommands_keep_the_tight_budget() {
        for line in [
            &["sync"][..],
            &["pip", "install", "x"],
            &["tool", "install", "x"],
            &["tool"],
            &[],
        ] {
            assert!(!is_uv_download_launcher(&args(line)), "{line:?}");
        }
    }
}
