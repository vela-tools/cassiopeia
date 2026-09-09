use crate::{broker::http2_prior_knowledge::Http2PriorKnowledge, error::Result};
use cassiopeia_common::user_agent::UserAgent;
use reqwest::blocking::Client;
use std::time::Duration;

/// The transport knobs the blocking HTTP client is built with.
pub struct ClientSettings {
    /// The per-request timeout.
    pub timeout: Duration,
    /// How many idle connections are kept per host.
    pub pool_max_idle_per_host: usize,
    /// The TCP keep-alive interval.
    pub tcp_keepalive: Duration,
    /// The `User-Agent` every request carries.
    pub user_agent: UserAgent,
    /// Whether HTTP/2 is used without an upgrade negotiation.
    pub http2_prior_knowledge: Http2PriorKnowledge,
}

/// Builds the blocking HTTP client with the broker's transport tuning.
///
/// `http2_adaptive_window` enables BDP-based flow control whenever ALPN negotiates h2 over HTTPS and
/// is inert for HTTP/1.1; the blocking builder exposes only this subset of the h2 knobs, so that is
/// what is set.
///
/// # Errors
/// Returns [`WriterError::ClientInit`](crate::error::WriterError::ClientInit) when the client cannot
/// be constructed.
pub fn build_client(settings: &ClientSettings) -> Result<Client> {
    let mut builder = Client::builder()
        .timeout(settings.timeout)
        .pool_max_idle_per_host(settings.pool_max_idle_per_host)
        .tcp_keepalive(settings.tcp_keepalive)
        .user_agent(settings.user_agent.as_str())
        .gzip(true)
        .http2_adaptive_window(true);
    if settings.http2_prior_knowledge.is_on() {
        builder = builder.http2_prior_knowledge();
    }
    Ok(builder.build()?)
}

#[cfg(test)]
mod tests {
    use crate::broker::{
        client_builder::{ClientSettings, build_client},
        http2_prior_knowledge::Http2PriorKnowledge,
    };
    use cassiopeia_common::user_agent::UserAgent;
    use std::time::Duration;

    fn settings(http2_prior_knowledge: Http2PriorKnowledge) -> ClientSettings {
        ClientSettings {
            timeout: Duration::from_secs(30),
            pool_max_idle_per_host: 8,
            tcp_keepalive: Duration::from_secs(60),
            user_agent: UserAgent::from("cassiopeia".to_owned()),
            http2_prior_knowledge,
        }
    }

    #[test]
    fn a_client_builds_with_and_without_prior_knowledge() {
        assert!(build_client(&settings(Http2PriorKnowledge::Off)).is_ok());
        assert!(build_client(&settings(Http2PriorKnowledge::On)).is_ok());
    }
}
