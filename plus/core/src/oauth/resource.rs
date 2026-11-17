/// A resource identifier reduced to the parts that decide whether two spellings name one resource.
#[derive(Debug, PartialEq, Eq)]
struct Identity<'a> {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "https" => Some(443),
        "http" => Some(80),
        _ => None,
    }
}

fn split_host_port(authority: &str) -> Option<(&str, Option<&str>)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        return match after {
            "" => Some((host, None)),
            _ => after.strip_prefix(':').map(|port| (host, Some(port))),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Some((host, Some(port))),
        Some(_) => None,
        None => Some((authority, None)),
    }
}

fn identity(url: &str) -> Option<Identity<'_>> {
    let (scheme, rest) = url.split_once("://")?;
    let (rest, fragment) = match rest.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (rest, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (rest, None),
    };
    let (authority, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let (host, port) = split_host_port(authority)?;
    if host.is_empty() {
        return None;
    }
    let port = match port {
        None | Some("") => default_port(&scheme),
        Some(digits) => Some(digits.parse::<u16>().ok()?),
    };
    let port = port.filter(|port| Some(*port) != default_port(&scheme));
    let path = path.strip_suffix('/').unwrap_or(path);
    Some(Identity {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
        path,
        query,
        fragment,
    })
}

/// Whether the resource a protected-resource document names is the configured server URL.
///
/// Scheme and host case, a default port and one trailing slash on the path do not tell two
/// resources apart; a different path, query, fragment or non-default port does. A URL carrying
/// credentials is never normalised: it matches only a byte-identical one.
pub fn same_resource(expected: &str, advertised: &str) -> bool {
    if expected == advertised {
        return true;
    }
    match (identity(expected), identity(advertised)) {
        (Some(expected), Some(advertised)) => expected == advertised,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::same_resource;

    const SAME: &[(&str, &str)] = &[
        ("https://host/mcp", "https://host/mcp"),
        ("https://host/mcp", "https://host/mcp/"),
        ("https://host/mcp/", "https://host/mcp"),
        ("https://host", "https://host/"),
        ("https://host/", "https://host"),
        ("HTTPS://Host.Example/mcp", "https://host.example/mcp"),
        ("https://host:443/mcp", "https://host/mcp"),
        ("https://host/mcp", "https://host:443/mcp/"),
        ("http://host:80/mcp", "http://host/mcp"),
        ("https://host:/mcp", "https://host/mcp"),
        ("http://127.0.0.1:8080/mcp", "http://127.0.0.1:8080/mcp/"),
        ("https://[::1]/mcp", "https://[::1]:443/mcp"),
        ("https://host/mcp?tenant=a", "https://host/mcp/?tenant=a"),
    ];

    const DIFFERENT: &[(&str, &str)] = &[
        ("https://host/mcp", "https://host/other"),
        ("https://host/mcp", "https://host/mcp//"),
        ("https://host/MCP", "https://host/mcp"),
        ("https://host/mcp", "https://host/mcp/sub"),
        ("https://host/mcp", "http://host/mcp"),
        ("https://host/mcp", "https://other/mcp"),
        ("https://host/mcp", "https://sub.host/mcp"),
        ("https://host/mcp", "https://host:8443/mcp"),
        ("http://host:80/mcp", "https://host:80/mcp"),
        ("https://host/mcp", "https://host/mcp?tenant=a"),
        ("https://host/mcp?tenant=a", "https://host/mcp?tenant=b"),
        ("https://host/mcp", "https://host/mcp#frag"),
        ("https://host/mcp", "https://user@host/mcp"),
        ("https://user@host/mcp", "https://user@host/mcp/"),
        ("https://host/mcp", "https://host:abc/mcp"),
        ("https://host/mcp", "https://host:99999/mcp"),
        ("https://host/mcp", "host/mcp"),
        ("https://host/mcp", ""),
        ("https:///mcp", "https:///mcp/"),
    ];

    #[test]
    fn spellings_of_one_resource_match() {
        for (expected, advertised) in SAME {
            assert!(
                same_resource(expected, advertised),
                "{expected} should match {advertised}"
            );
        }
    }

    #[test]
    fn different_resources_do_not_match() {
        for (expected, advertised) in DIFFERENT {
            assert!(
                !same_resource(expected, advertised),
                "{expected} should not match {advertised}"
            );
        }
    }

    #[test]
    fn a_match_does_not_depend_on_the_order() {
        for (expected, advertised) in SAME {
            assert!(same_resource(advertised, expected));
        }
        for (expected, advertised) in DIFFERENT {
            assert!(!same_resource(advertised, expected));
        }
    }

    #[test]
    fn identical_text_always_matches() {
        assert!(same_resource("not a url", "not a url"));
        assert!(same_resource(
            "https://user@host/mcp",
            "https://user@host/mcp"
        ));
    }
}
