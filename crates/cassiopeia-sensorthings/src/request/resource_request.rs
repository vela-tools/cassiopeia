use crate::service_root::ServiceRoot;
use url::Url;

/// The request of the resource at the path `resource` under `root`, with the query `query`, both
/// already written as they are sent.
pub(crate) fn resource_request(root: &ServiceRoot, resource: &str, query: &str) -> Url {
    // The request is a URL of its own, built on the root the service keeps.
    let mut request = root.url().clone();
    request.set_path(&format!("{}/{resource}", root.url().path()));
    request.set_query(Some(query));
    request
}

#[cfg(test)]
mod tests {
    use crate::{request::resource_request::resource_request, service_root::ServiceRoot};
    use pretty_assertions::assert_eq;
    use url::Url;

    #[test]
    fn a_resource_is_requested_under_the_root_with_its_query_as_written() {
        let root = ServiceRoot::try_from(Url::parse("http://localhost:18080/FROST-Server/v1.1/").unwrap()).unwrap();

        let request = resource_request(&root, "Datastreams('a%20b')/Observations", "$top=1&$filter=a%20eq%201");

        assert_eq!(
            request.as_str(),
            "http://localhost:18080/FROST-Server/v1.1/Datastreams('a%20b')/Observations?$top=1&$filter=a%20eq%201"
        );
    }
}
