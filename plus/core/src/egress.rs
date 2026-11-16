//! Hosts the fork never contacts.

/// Upstream's own service domain. A fork build must not phone it.
const REFUSED_DOMAINS: &[&str] = &["toolport.app"];

const REFUSAL_SUFFIX: &str = " is disabled in this build";

/// True for a message `check` produced; transports that only see an error
/// string use it to tell a refusal from a network failure.
pub fn is_refusal(message: &str) -> bool {
    message.starts_with("egress to ") && message.ends_with(REFUSAL_SUFFIX)
}

/// Err names the refused domain when `host` is it or one of its subdomains.
pub fn check(host: &str) -> Result<(), String> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    match REFUSED_DOMAINS.iter().find(|domain| {
        host == **domain
            || host
                .strip_suffix(**domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    }) {
        Some(domain) => Err(format!("egress to {domain}{REFUSAL_SUFFIX}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{check, is_refusal};

    #[test]
    fn refuses_the_domain_and_its_subdomains() {
        assert!(check("toolport.app").is_err());
        assert!(check("Updates.Toolport.App").is_err());
        assert!(check("a.b.toolport.app.").is_err());
    }

    #[test]
    fn recognises_its_own_refusal_message_only() {
        let message = check("toolport.app").unwrap_err();
        assert!(is_refusal(&message));
        assert!(!is_refusal("connection refused"));
        assert!(!is_refusal("egress to nowhere"));
    }

    #[test]
    fn allows_lookalikes_and_other_hosts() {
        assert!(check("github.com").is_ok());
        assert!(check("nottoolport.app").is_ok());
        assert!(check("toolport.app.example.com").is_ok());
    }
}
