use crate::{
    context_loader::ContextLoader,
    core_context::{CORE_CONTEXT, HAS_VALUE_TYPE, core_context_document},
    error::ExpansionError,
    expanded_value_types::ExpandedValueTypes,
};
use cassiopeia_common::user_agent::UserAgent;
use cassiopeia_ngsi_ld::entity::{context::NgsiLdContext, value_type::ValueType};
use futures_executor::block_on;
use iri_rs::IriBuf;
use jsonld::{JsonLdProcessor, RemoteDocument};
use serde_json::{Map, Value as JsonValue, json};

/// The scheme of the identifiers that pair each probe node with the value type it carries.
const PROBE_ID: &str = "urn:cassiopeia:value-type:";

/// Expands value types against an entity's `@context`, the way a broker expands the `valueType` of
/// the attributes it receives.
///
/// Expansion runs the JSON-LD 1.1 expansion algorithm over a document that carries each value type as
/// the `valueType` of its own node, so the core `@context` definition of that member decides how the
/// text is coerced (`@type: @vocab`, ETSI GS CIM 009 v1.9.1 annex B): a term the `@context` defines
/// expands to its IRI, a compact IRI through its prefix, an absolute IRI to itself, and any other term
/// through the core `@context`'s `@vocab`. The `@context` is the entity's own, followed by the core
/// `@context`, which clause 4.4 treats as being in the last position. Every `@context` is loaded once
/// per expander, however many mappings name it.
pub struct ValueTypeExpander {
    /// Where every `@context` document comes from.
    loader: ContextLoader,
}

impl ValueTypeExpander {
    /// Builds an expander that fetches remote `@context` documents under `user_agent`.
    ///
    /// # Errors
    /// Returns [`ExpansionError::CoreContext`] when the bundled core `@context` cannot be read, and
    /// [`ExpansionError::Client`] when the HTTP client cannot be built.
    pub fn new(user_agent: &UserAgent) -> Result<ValueTypeExpander, ExpansionError> {
        let core = core_context_document().map_err(ExpansionError::CoreContext)?;
        let loader = ContextLoader::new(user_agent, core).map_err(ExpansionError::Client)?;

        Ok(ValueTypeExpander { loader })
    }

    /// Expands every value type in `value_types` against `context`, or against the core `@context`
    /// alone when the entity is written without one.
    ///
    /// # Errors
    /// Returns [`ExpansionError::Expansion`] when a `@context` cannot be loaded or processed, and
    /// [`ExpansionError::NotAnIri`] naming the first value type that expands to no absolute IRI.
    pub fn expand<'a>(
        &self,
        context: Option<&NgsiLdContext>,
        value_types: impl IntoIterator<Item = &'a ValueType>,
    ) -> Result<ExpandedValueTypes, ExpansionError> {
        let value_types: Vec<&ValueType> = value_types.into_iter().collect();
        let document = probe_document(context, &value_types)?;
        let expanded = block_on(document.expand(&self.loader)).map_err(ExpansionError::Expansion)?.into_serde_json();

        let mut iris = ExpandedValueTypes::default();
        for (index, value_type) in value_types.into_iter().enumerate() {
            let iri = expanded_iri(&expanded, index).ok_or_else(|| ExpansionError::NotAnIri {
                value_type: value_type.clone(),
            })?;
            // The expansion result is keyed by the value type as written, which the caller still holds.
            iris.insert(value_type.clone(), iri);
        }

        Ok(iris)
    }
}

/// The document expanded to read each value type's IRI: one node per value type, identified by its
/// position, under the entity's `@context` with the core `@context` last.
fn probe_document(context: Option<&NgsiLdContext>, value_types: &[&ValueType]) -> Result<RemoteDocument, ExpansionError> {
    let mut contexts = match context.map(serde_json::to_value).transpose().map_err(ExpansionError::Context)? {
        Some(JsonValue::Array(entries)) => entries,
        Some(entry) => vec![entry],
        None => Vec::new(),
    };
    contexts.push(JsonValue::String(CORE_CONTEXT.as_str().to_owned()));

    let nodes = value_types
        .iter()
        .enumerate()
        .map(|(index, value_type)| json!({"@id": format!("{PROBE_ID}{index}"), "valueType": value_type.as_str()}))
        .collect::<Vec<JsonValue>>();
    let mut document = Map::new();
    document.insert("@context".to_owned(), JsonValue::Array(contexts));
    document.insert("@graph".to_owned(), JsonValue::Array(nodes));

    Ok(RemoteDocument::from_serde_json(None, None, JsonValue::Object(document)))
}

/// The absolute IRI the probe node at `index` holds as its value type, when it holds one.
///
/// The expanded value of a `@vocab`-coerced member is a node reference, `{"@id": iri}`; a value that
/// expands to nothing leaves the node without the member.
fn expanded_iri(expanded: &JsonValue, index: usize) -> Option<IriBuf> {
    let id = format!("{PROBE_ID}{index}");
    let node = expanded
        .as_array()?
        .iter()
        .find(|node| node.get("@id").and_then(JsonValue::as_str) == Some(id.as_str()))?;
    let reference = node.get(HAS_VALUE_TYPE.as_str())?.as_array()?.first()?;

    IriBuf::new(reference.get("@id")?.as_str()?.to_owned()).ok()
}

#[cfg(test)]
mod tests {
    use crate::{error::ExpansionError, value_type_expander::ValueTypeExpander};
    use cassiopeia_common::user_agent::UserAgent;
    use cassiopeia_ngsi_ld::entity::{context::NgsiLdContext, value_type::ValueType};
    use pretty_assertions::assert_eq;

    fn expander() -> ValueTypeExpander {
        ValueTypeExpander::new(&UserAgent::from("test".to_owned())).unwrap()
    }

    fn value_type(text: &str) -> ValueType {
        ValueType::new(text).unwrap()
    }

    /// What `text` expands to under `context`, followed by the core `@context`.
    fn expanded(context: Option<&NgsiLdContext>, text: &str) -> String {
        let value_type = value_type(text);
        expander().expand(context, [&value_type]).unwrap().iri(&value_type).unwrap().as_str().to_owned()
    }

    #[test]
    fn the_time_data_types_expand_through_the_core_context_whichever_way_they_are_spelt() {
        for spelling in ["DateTime", "ngsi-ld:DateTime", "https://uri.etsi.org/ngsi-ld/DateTime"] {
            assert_eq!(expanded(None, spelling), "https://uri.etsi.org/ngsi-ld/DateTime");
        }
        assert_eq!(expanded(None, "Date"), "https://uri.etsi.org/ngsi-ld/Date");
        assert_eq!(expanded(None, "Time"), "https://uri.etsi.org/ngsi-ld/Time");
    }

    #[test]
    fn lang_string_expands_to_the_rdf_datatype_the_core_context_maps_it_to() {
        assert_eq!(expanded(None, "langString"), "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString");
    }

    #[test]
    fn a_term_no_context_defines_expands_through_the_core_vocabulary_mapping() {
        assert_eq!(expanded(None, "PostalAddress"), "https://uri.etsi.org/ngsi-ld/default-context/PostalAddress");
    }

    #[test]
    fn an_absolute_iri_expands_to_itself() {
        assert_eq!(
            expanded(None, "http://www.w3.org/2001/XMLSchema#duration"),
            "http://www.w3.org/2001/XMLSchema#duration"
        );
    }

    #[test]
    fn a_user_context_term_and_prefix_are_expanded_as_annex_c_10_defines_them() {
        let context: NgsiLdContext = serde_json::from_value(serde_json::json!({
            "example": "http://example.org/",
            "xsd": "http://www.w3.org/2001/XMLSchema#",
            "PostalAddress": "example:PostalAddress"
        }))
        .unwrap();

        assert_eq!(expanded(Some(&context), "PostalAddress"), "http://example.org/PostalAddress");
        assert_eq!(expanded(Some(&context), "xsd:decimal"), "http://www.w3.org/2001/XMLSchema#decimal");
    }

    #[test]
    fn the_core_context_in_the_last_position_wins_over_a_user_definition_of_its_terms() {
        let context: NgsiLdContext = serde_json::from_value(serde_json::json!({"DateTime": "http://example.org/MyDateTime"})).unwrap();

        assert_eq!(expanded(Some(&context), "DateTime"), "https://uri.etsi.org/ngsi-ld/DateTime");
    }

    #[test]
    fn several_value_types_are_expanded_in_one_pass() {
        let date_time = value_type("DateTime");
        let address = value_type("PostalAddress");

        let expanded = expander().expand(None, [&date_time, &address]).unwrap();

        assert!(expanded.iri(&date_time).is_some());
        assert!(expanded.iri(&address).is_some());
        assert!(!expanded.same_datatype(&date_time, &address));
    }

    #[test]
    fn a_term_a_user_context_maps_to_a_blank_node_expands_to_no_iri_and_is_named() {
        let context: NgsiLdContext = serde_json::from_value(serde_json::json!({"Anonymous": "_:b0"})).unwrap();
        let anonymous = value_type("Anonymous");

        let error = expander().expand(Some(&context), [&anonymous]).unwrap_err();

        assert!(matches!(error, ExpansionError::NotAnIri { value_type } if value_type == anonymous));
    }
}
