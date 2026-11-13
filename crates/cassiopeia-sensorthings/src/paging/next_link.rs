use crate::service_root::ServiceRoot;
use thiserror::Error;
use url::{ParseError, Url};

/// Why a page's `@iot.nextLink` is not followed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NextLinkError {
    /// The link resolves to no URL.
    #[error("the next link `{link}` is no URL")]
    Unparseable {
        /// The link as the server sent it.
        link: Box<str>,
        /// Why it resolves to no URL.
        #[source]
        source: ParseError,
    },

    /// The link names another scheme, host or port than the service root.
    #[error("the next link `{link}` leaves the service `{root}`, so it is not followed")]
    LeavesService {
        /// The link, resolved. Boxed so this error does not enlarge every `Result` that carries it.
        link: Box<Url>,
        /// The service root the read is built on. Boxed so this error does not enlarge every
        /// `Result` that carries it.
        root: Box<ServiceRoot>,
    },
}

/// The URL a page's `@iot.nextLink` leads to: the link resolved against the URL of the page it came
/// from (RFC 3986 section 5), and otherwise unchanged.
///
/// A client treats the link as opaque and appends no query option to it (OGC 18-088 section
/// 9.3.3.6, Req 32), so it is followed as the server wrote it. It must stay on the service root's
/// origin, because a read sends its input's headers, and so its credentials, with every request.
///
/// # Errors
///
/// Returns [`NextLinkError::Unparseable`] for a link that resolves to no URL, and
/// [`NextLinkError::LeavesService`] for one whose scheme, host or port differs from the root's.
pub fn resolve_next_link(page: &Url, link: &str, root: &ServiceRoot) -> Result<Url, NextLinkError> {
    let resolved = page.join(link).map_err(|source| NextLinkError::Unparseable { link: link.into(), source })?;
    if resolved.origin() != root.url().origin() {
        return Err(NextLinkError::LeavesService {
            link: Box::new(resolved),
            // The error names the root, while the read keeps its own.
            root: Box::new(root.clone()),
        });
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use crate::{
        paging::next_link::{NextLinkError, resolve_next_link},
        service_root::ServiceRoot,
    };
    use pretty_assertions::assert_eq;
    use url::Url;

    fn root() -> ServiceRoot {
        ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap()
    }

    fn page(url: &str) -> Url {
        Url::parse(url).unwrap()
    }

    #[test]
    fn an_absolute_next_link_on_the_service_origin_is_followed() {
        let next = resolve_next_link(
            &page("https://h.example/v1.1/Datastreams?$top=100"),
            "https://h.example/v1.1/Datastreams?$skip=100",
            &root(),
        )
        .unwrap();

        assert_eq!(next.as_str(), "https://h.example/v1.1/Datastreams?$skip=100");
    }

    #[test]
    fn a_relative_next_link_is_resolved_against_its_page() {
        let next = resolve_next_link(&page("https://h.example/v1.1/Datastreams?$top=100"), "Datastreams?$skip=100", &root()).unwrap();

        assert_eq!(next.as_str(), "https://h.example/v1.1/Datastreams?$skip=100");
    }

    #[test]
    fn a_next_link_to_another_origin_is_refused() {
        for link in [
            "https://other.example/v1.1/Datastreams?$skip=100",
            "http://h.example/v1.1/Datastreams?$skip=100",
            "https://h.example:8443/v1.1/Datastreams?$skip=100",
        ] {
            let error = resolve_next_link(&page("https://h.example/v1.1/Datastreams"), link, &root()).unwrap_err();

            assert_eq!(
                error,
                NextLinkError::LeavesService {
                    link: Box::new(Url::parse(link).unwrap()),
                    root: Box::new(root()),
                }
            );
            assert!(error.to_string().contains("leaves the service `https://h.example/v1.1`"), "{error}");
        }
    }

    #[test]
    fn a_next_link_on_the_default_port_written_out_stays_on_the_service() {
        let next = resolve_next_link(
            &page("https://h.example/v1.1/Datastreams"),
            "https://h.example:443/v1.1/Datastreams?$skip=1",
            &root(),
        );

        assert!(next.is_ok(), "{next:?}");
    }

    #[test]
    fn a_next_link_is_kept_opaque() {
        let next = resolve_next_link(
            &page("https://h.example/v1.1/Datastreams"),
            "https://h.example/v1.1/Datastreams?$top=100&$skip=100&x=1",
            &root(),
        )
        .unwrap();

        assert_eq!(next.query(), Some("$top=100&$skip=100&x=1"));
    }

    #[test]
    fn a_form_encoded_next_link_is_followed_as_written() {
        let link = "https://h.example/v1.1/Datastreams?$filter=startswith%28name%2C%27ILM_I68-%27%29&$orderby=id+asc&$top=1&$skip=1";

        let next = resolve_next_link(&page("https://h.example/v1.1/Datastreams"), link, &root()).unwrap();

        assert_eq!(next.as_str(), link);
    }

    #[test]
    fn a_next_link_that_is_no_url_is_refused() {
        let error = resolve_next_link(&page("https://h.example/v1.1/Datastreams"), "https://[::1", &root()).unwrap_err();

        assert!(matches!(error, NextLinkError::Unparseable { .. }), "{error}");
    }
}
