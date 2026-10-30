use url::{Host, Url};

/// The last label of a source's DNS host name, in lower-case ASCII: `at` for `www.wien.gv.at`.
///
/// An internationalised label is held in its Punycode form, as [`Url`] normalises a host to. The
/// encoding detector reads it as a hint to the region a legacy single-byte encoding is native to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopLevelDomain(String);

impl TopLevelDomain {
    /// The top-level domain of `url`'s host, or `None` for a URL with no DNS host name, such as an
    /// IP address or a `file:` URL.
    #[must_use]
    pub fn of(url: &Url) -> Option<TopLevelDomain> {
        let Some(Host::Domain(domain)) = url.host() else {
            return None;
        };
        let label = domain.trim_end_matches('.').rsplit('.').next()?;
        let is_dns_label = !label.is_empty() && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
        is_dns_label.then(|| TopLevelDomain(label.to_ascii_lowercase()))
    }

    /// The label as bytes, lower-case ASCII with no period, the form the encoding detector requires.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use crate::top_level_domain::TopLevelDomain;
    use pretty_assertions::assert_eq;
    use url::Url;

    fn tld_of(url: &str) -> Option<TopLevelDomain> {
        TopLevelDomain::of(&Url::parse(url).unwrap())
    }

    #[test]
    fn the_last_host_label_is_the_top_level_domain() {
        assert_eq!(tld_of("https://www.wien.gv.at/ma22-lgb/umweltgut/lumesakt-v2.csv").unwrap().as_bytes(), b"at");
    }

    #[test]
    fn an_upper_case_host_and_a_trailing_root_period_are_normalised() {
        assert_eq!(tld_of("https://DATA.EXAMPLE.SI./feed.csv").unwrap().as_bytes(), b"si");
    }

    #[test]
    fn an_internationalised_top_level_domain_is_held_in_punycode() {
        assert_eq!(tld_of("https://example.рф/feed.csv").unwrap().as_bytes(), b"xn--p1ai");
    }

    #[test]
    fn an_ip_address_or_file_url_has_no_top_level_domain() {
        assert!(tld_of("http://127.0.0.1:8080/feed.csv").is_none());
        assert!(tld_of("http://[::1]/feed.csv").is_none());
        assert!(tld_of("file:///data/feed.csv").is_none());
    }
}
