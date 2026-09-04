use crate::output::{context_delivery::ContextDelivery, user_agent::UserAgent};
use cassiopeia_common::{
    attribute_overwrite::AttributeOverwrite,
    broker_atomicity::BrokerAtomicity,
    broker_header::BrokerHeaders,
    broker_operation::BrokerOperationKind,
    file_framing::FileFraming,
    tenant::Tenant,
    upsert_mode::UpsertMode,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

/// Where produced entities end up.
///
/// The two destinations take disjoint settings (a directory is meaningless to a broker, and a
/// tenant is meaningless to a file), so each carries only its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "kebab-case")]
pub enum Destination {
    /// Write entities to the file system.
    File {
        /// The directory the entity files are written into.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        directory: Option<PathBuf>,

        /// How each per-type file is framed (a JSON array or line-delimited entities).
        #[serde(default)]
        framing: FileFraming,
    },

    /// Send entities to an NGSI-LD Context Broker.
    #[serde(rename_all = "camelCase")]
    ContextBroker {
        /// The broker's base URL.
        url: Url,

        /// The value sent in the `NGSILD-Tenant` header.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tenant: Option<Tenant>,

        /// The value sent in the HTTP `User-Agent` header. Absent means the run-time default applies.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_agent: Option<UserAgent>,

        /// Extra HTTP headers attached to every request, typically carrying broker credentials.
        #[serde(default, skip_serializing_if = "BrokerHeaders::is_empty")]
        headers: BrokerHeaders,

        /// How `@context` is attached to each request.
        #[serde(default)]
        context_delivery: ContextDelivery,

        /// Which NGSI-LD operation each request performs.
        #[serde(default)]
        operation: BrokerOperationKind,

        /// The upsert reconciliation mode, applied only when the operation is `upsert`.
        #[serde(default)]
        upsert_mode: UpsertMode,

        /// The attribute-overwrite behaviour, applied only when the operation is `update`.
        #[serde(default)]
        attribute_overwrite: AttributeOverwrite,

        /// Whether entities stream to the broker or are spooled and pushed atomically.
        #[serde(default)]
        atomicity: BrokerAtomicity,
    },
}

impl Destination {
    /// Builds a file-system destination writing into `directory`, or the run-time default
    /// directory when none is named, framed as `framing` selects.
    #[must_use]
    pub const fn file(directory: Option<PathBuf>, framing: FileFraming) -> Destination {
        Destination::File { directory, framing }
    }
}

#[cfg(test)]
mod tests {
    use crate::output::{context_delivery::ContextDelivery, destination::Destination};
    use cassiopeia_common::{
        attribute_overwrite::AttributeOverwrite,
        broker_atomicity::BrokerAtomicity,
        broker_operation::BrokerOperationKind,
        file_framing::FileFraming,
        upsert_mode::UpsertMode,
    };
    use std::path::PathBuf;

    #[test]
    fn a_file_destination_reads_its_directory_and_defaults_its_framing() {
        let destination: Destination = serde_json::from_str(r#"{"target": "file", "directory": "out"}"#).unwrap();

        assert_eq!(
            destination,
            Destination::File {
                directory: Some(PathBuf::from("out")),
                framing: FileFraming::Array
            }
        );
    }

    #[test]
    fn a_file_destination_reads_its_framing() {
        let destination: Destination = serde_json::from_str(r#"{"target": "file", "directory": "out", "framing": "line-delimited"}"#).unwrap();

        assert_eq!(
            destination,
            Destination::File {
                directory: Some(PathBuf::from("out")),
                framing: FileFraming::LineDelimited
            }
        );
    }

    #[test]
    fn a_broker_destination_defaults_to_embedding_the_context_upserting_and_streaming() {
        let destination: Destination = serde_json::from_str(r#"{"target": "context-broker", "url": "http://localhost:1026/"}"#).unwrap();

        match destination {
            Destination::ContextBroker {
                context_delivery,
                tenant,
                user_agent,
                headers,
                operation,
                upsert_mode,
                attribute_overwrite,
                atomicity,
                ..
            } => {
                assert_eq!(context_delivery, ContextDelivery::Body);
                assert_eq!(tenant, None);
                assert_eq!(user_agent, None);
                assert!(headers.is_empty());
                assert_eq!(operation, BrokerOperationKind::Upsert);
                assert_eq!(upsert_mode, UpsertMode::Replace);
                assert_eq!(attribute_overwrite, AttributeOverwrite::Overwrite);
                assert_eq!(atomicity, BrokerAtomicity::Streaming);
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }
    }

    #[test]
    fn a_broker_destination_round_trips_an_updating_no_overwrite_operation() {
        let destination: Destination = serde_json::from_str(
            r#"{"target": "context-broker", "url": "http://localhost:1026/", "operation": "update", "attributeOverwrite": "no-overwrite"}"#,
        )
        .unwrap();

        match &destination {
            Destination::ContextBroker {
                operation,
                attribute_overwrite,
                ..
            } => {
                assert_eq!(*operation, BrokerOperationKind::Update);
                assert_eq!(*attribute_overwrite, AttributeOverwrite::NoOverwrite);
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }

        assert_eq!(
            serde_json::from_str::<Destination>(&serde_json::to_string(&destination).unwrap()).unwrap(),
            destination
        );
    }

    #[test]
    fn a_broker_destination_round_trips_an_updating_upsert_operation() {
        let destination: Destination =
            serde_json::from_str(r#"{"target": "context-broker", "url": "http://localhost:1026/", "operation": "upsert", "upsertMode": "update"}"#).unwrap();

        match &destination {
            Destination::ContextBroker { operation, upsert_mode, .. } => {
                assert_eq!(*operation, BrokerOperationKind::Upsert);
                assert_eq!(*upsert_mode, UpsertMode::Update);
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }

        assert_eq!(
            serde_json::from_str::<Destination>(&serde_json::to_string(&destination).unwrap()).unwrap(),
            destination
        );
    }

    #[test]
    fn a_broker_destination_without_a_url_is_rejected() {
        assert!(serde_json::from_str::<Destination>(r#"{"target": "context-broker"}"#).is_err());
    }

    #[test]
    fn the_file_constructor_carries_its_directory_and_framing() {
        assert_eq!(
            Destination::file(Some(PathBuf::from("out")), FileFraming::LineDelimited),
            Destination::File {
                directory: Some(PathBuf::from("out")),
                framing: FileFraming::LineDelimited
            }
        );
    }

    #[test]
    fn a_broker_destination_reads_and_round_trips_its_user_agent() {
        let destination: Destination =
            serde_json::from_str(r#"{"target": "context-broker", "url": "http://localhost:1026/", "userAgent": "acme/1.0"}"#).unwrap();

        match &destination {
            Destination::ContextBroker { user_agent, .. } => {
                assert_eq!(user_agent.as_ref().map(ToString::to_string), Some("acme/1.0".to_string()));
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }

        let encoded = serde_json::to_string(&destination).unwrap();
        assert!(encoded.contains(r#""userAgent":"acme/1.0""#));
        assert_eq!(serde_json::from_str::<Destination>(&encoded).unwrap(), destination);
    }

    #[test]
    fn a_broker_destination_reads_and_round_trips_its_headers() {
        let destination: Destination =
            serde_json::from_str(r#"{"target": "context-broker", "url": "http://localhost:1026/", "headers": {"Authorization": "Bearer t"}}"#).unwrap();

        match &destination {
            Destination::ContextBroker { headers, .. } => {
                let header = headers.iter().next().expect("one header");
                assert_eq!(header.name().as_str(), "authorization");
                assert_eq!(header.value().to_str().unwrap(), "Bearer t");
            }
            Destination::File { .. } => panic!("expected a broker destination"),
        }

        let encoded = serde_json::to_string(&destination).unwrap();
        assert_eq!(serde_json::from_str::<Destination>(&encoded).unwrap(), destination);
    }
}
