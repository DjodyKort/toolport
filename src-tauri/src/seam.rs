//! Fork hook points. Each function forwards to `toolport-plus` and does nothing
//! without the `plus` feature.

/// Refuse a request before the connection phase when the fork blocks its host.
pub(crate) fn egress_check(uri: &ureq::http::Uri) -> Result<(), ureq::Error> {
    #[cfg(feature = "plus")]
    if let Some(host) = uri.host() {
        toolport_plus::egress::check(host).map_err(|message| ureq::Error::Other(message.into()))?;
    }
    #[cfg(not(feature = "plus"))]
    let _ = uri;
    Ok(())
}

#[cfg(all(test, feature = "plus"))]
mod tests {
    use super::egress_check;

    #[test]
    fn egress_check_refuses_upstream_service_hosts() {
        let refused = "https://updates.toolport.app/latest.json".parse().unwrap();
        assert!(egress_check(&refused).is_err());
        let allowed = "https://github.com/".parse().unwrap();
        assert!(egress_check(&allowed).is_ok());
    }

    #[test]
    fn http_agent_refuses_toolport_app_before_connecting() {
        let error = crate::http_client::agent()
            .get("https://toolport.app/")
            .call()
            .expect_err("the resolver must refuse the request");
        assert!(
            error.to_string().contains("egress to toolport.app"),
            "{error}"
        );
    }
}
