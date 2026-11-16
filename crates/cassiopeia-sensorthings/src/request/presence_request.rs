use crate::{request::resource_request::resource_request, service_root::ServiceRoot, stream_kind::StreamKind};
use url::Url;

/// The request for the service root document of `root`, which lists the collections the server
/// serves (OGC 18-088 section 9.2.1).
#[must_use]
pub fn service_document_request(root: &ServiceRoot) -> Url {
    // The request is a URL of its own, built on the root the service keeps.
    root.url().clone()
}

/// The request for at most one data stream of `kind`, which tells whether the server holds any.
#[must_use]
pub fn any_stream_request(root: &ServiceRoot, kind: StreamKind) -> Url {
    resource_request(root, kind.collection(), "$top=1")
}

#[cfg(test)]
mod tests {
    use crate::{
        request::presence_request::{any_stream_request, service_document_request},
        service_root::ServiceRoot,
        stream_kind::StreamKind,
    };
    use pretty_assertions::assert_eq;
    use url::Url;

    fn root() -> ServiceRoot {
        ServiceRoot::try_from(Url::parse("https://h.example/v1.1/").unwrap()).unwrap()
    }

    #[test]
    fn the_service_document_is_requested_at_the_root() {
        assert_eq!(service_document_request(&root()).as_str(), "https://h.example/v1.1");
    }

    #[test]
    fn one_multi_datastream_is_requested_from_its_collection() {
        assert_eq!(
            any_stream_request(&root(), StreamKind::MultiDatastream).as_str(),
            "https://h.example/v1.1/MultiDatastreams?$top=1"
        );
    }
}
