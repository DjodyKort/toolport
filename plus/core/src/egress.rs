//! Hosts the fork never contacts.

/// Upstream's own service domain. A fork build must not phone it.
const REFUSED_DOMAINS: &[&str] = &["toolport.app"];

/// Err names the refused domain when `host` is it or one of its subdomains.
pub fn check(host: &str) -> Result<(), String> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    match REFUSED_DOMAINS.iter().find(|domain| {
        host == **domain
            || host
                .strip_suffix(**domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    }) {
        Some(domain) => Err(format!("egress to {domain} is disabled in this build")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn refuses_the_domain_and_its_subdomains() {
        assert!(check("toolport.app").is_err());
        assert!(check("Updates.Toolport.App").is_err());
        assert!(check("a.b.toolport.app.").is_err());
    }

    #[test]
    fn allows_lookalikes_and_other_hosts() {
        assert!(check("github.com").is_ok());
        assert!(check("nottoolport.app").is_ok());
        assert!(check("toolport.app.example.com").is_ok());
    }
}
