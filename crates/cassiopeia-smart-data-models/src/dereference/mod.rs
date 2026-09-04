pub mod document_ref;
pub mod pointer;
pub mod resolver;

use crate::{
    dereference::{document_ref::DocumentRef, resolver::SchemaResolver},
    error::{Result, SdmError},
};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{Map, Value};
use std::sync::Arc;

/// Whether a `$ref` naming another document is followed.
pub enum ExternalReferences<'a> {
    /// Follow it, reading the other document through this resolver.
    Follow(&'a dyn SchemaResolver),

    /// Refuse it, so a schema that reaches outside itself fails rather than silently losing part of
    /// its shape.
    Reject,
}

/// What happens when a `$ref` eventually points back at itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CircularReferences {
    /// Replace the reference that closes the cycle with `null` and carry on.
    ///
    /// Several published Smart Data Models describe recursive structures, so this is the default:
    /// the expansion stops at the point the cycle closes instead of failing the whole schema.
    #[default]
    Break,

    /// Refuse the schema.
    Reject,
}

/// Produces a copy of `schema` in which every `$ref` has been replaced by what it points at.
///
/// The result is a new value rather than an in-place rewrite, because expanding a reference needs
/// the unexpanded document to read from while the copy is being built.
///
/// # Errors
/// Returns an [`SdmError`] when a reference cannot be resolved: an external reference under
/// [`ExternalReferences::Reject`], a cycle under [`CircularReferences::Reject`], a malformed JSON
/// pointer, or a pointer that names nothing.
pub fn dereference(schema: &Value, external: &ExternalReferences<'_>, circular: CircularReferences) -> Result<Value> {
    let mut expansion = Expansion {
        external,
        circular,
        resolved: FxHashMap::default(),
        expanding: FxHashSet::default(),
        documents: FxHashMap::default(),
    };

    expansion.expand(schema, schema)
}

/// The state carried through one expansion: what has been expanded, what is being expanded, and
/// which other documents have been read.
struct Expansion<'a> {
    external: &'a ExternalReferences<'a>,
    circular: CircularReferences,
    resolved: FxHashMap<Arc<str>, Arc<Value>>,
    expanding: FxHashSet<Arc<str>>,
    documents: FxHashMap<String, Arc<Value>>,
}

impl Expansion<'_> {
    /// Copies `node`, expanding every `$ref` beneath it against `document`.
    fn expand(&mut self, node: &Value, document: &Value) -> Result<Value> {
        match node {
            Value::Object(members) => match members.get("$ref") {
                Some(Value::String(reference)) => self.expand_reference(Arc::from(reference.as_str()), document),
                Some(_) | None => self.expand_members(members, document),
            },
            Value::Array(items) => items.iter().map(|item| self.expand(item, document)).collect(),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(node.clone()),
        }
    }

    /// Copies an object, expanding each of its members.
    fn expand_members(&mut self, members: &Map<String, Value>, document: &Value) -> Result<Value> {
        members
            .iter()
            .map(|(key, value)| Ok((key.clone(), self.expand(value, document)?)))
            .collect::<Result<Map<String, Value>>>()
            .map(Value::Object)
    }

    /// Replaces one `$ref` with what it points at.
    fn expand_reference(&mut self, reference: Arc<str>, document: &Value) -> Result<Value> {
        if let Some(expanded) = self.resolved.get(&reference) {
            // The cache holds the fully expanded subtree; the caller needs an owned copy of it.
            return Ok((**expanded).clone());
        }

        if self.expanding.contains(&reference) {
            return match self.circular {
                CircularReferences::Break => Ok(Value::Null),
                CircularReferences::Reject => Err(SdmError::CircularReference {
                    reference: reference.to_string(),
                }),
            };
        }

        let (uri, fragment) = split_reference(&reference);

        // The reference is marked in-progress across the whole expansion so a `$ref` that points
        // back at it is caught as a cycle rather than expanded forever.
        self.expanding.insert(Arc::clone(&reference));
        let expanded = match self.external_document(DocumentRef::new(uri)) {
            // A local reference reads from the document being expanded without copying it.
            Ok(None) => self.expand_pointer(fragment, document),
            Ok(Some(target)) => self.expand_pointer(fragment, &target),
            Err(error) => Err(error),
        };
        self.expanding.remove(&reference);

        let expanded = expanded?;
        self.resolved.insert(reference, Arc::new(expanded.clone()));

        Ok(expanded)
    }

    /// Resolves `fragment` inside `document` and expands what it points at against that document.
    fn expand_pointer(&mut self, fragment: &str, document: &Value) -> Result<Value> {
        let pointed_at = pointer::resolve(document, fragment)?.clone();

        self.expand(&pointed_at, document)
    }

    /// The externally resolved document a reference points into, or `None` when the reference names
    /// the document currently being expanded.
    fn external_document(&mut self, reference: DocumentRef<'_>) -> Result<Option<Arc<Value>>> {
        if reference.is_empty() {
            return Ok(None);
        }

        let uri = reference.as_str();
        if let Some(already_read) = self.documents.get(uri) {
            return Ok(Some(Arc::clone(already_read)));
        }

        let resolver = match self.external {
            ExternalReferences::Follow(resolver) => resolver,
            ExternalReferences::Reject => {
                return Err(SdmError::ExternalReferenceRejected { reference: uri.to_string() });
            }
        };

        let document = resolver.resolve(reference)?;
        self.documents.insert(uri.to_string(), Arc::clone(&document));

        Ok(Some(document))
    }
}

/// Splits a `$ref` into the document it names and the pointer into that document.
fn split_reference(reference: &str) -> (&str, &str) {
    match reference.split_once('#') {
        Some((uri, fragment)) => (uri, fragment),
        None => (reference, ""),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        dereference::{CircularReferences, ExternalReferences, dereference, document_ref::DocumentRef, resolver::SchemaResolver},
        error::{Result, SdmError},
    };
    use serde_json::{Value, json};
    use std::sync::Arc;

    struct OneDocument {
        uri: &'static str,
        document: Value,
    }

    impl SchemaResolver for OneDocument {
        fn resolve(&self, reference: DocumentRef<'_>) -> Result<Arc<Value>> {
            if reference.as_str() == self.uri {
                Ok(Arc::new(self.document.clone()))
            } else {
                Err(SdmError::ExternalReferenceRejected {
                    reference: reference.as_str().to_string(),
                })
            }
        }
    }

    #[test]
    fn a_local_reference_is_replaced_by_what_it_points_at() {
        let schema = json!({
            "definitions": {"Name": {"type": "string"}},
            "properties": {"name": {"$ref": "#/definitions/Name"}},
        });

        let expanded = dereference(&schema, &ExternalReferences::Reject, CircularReferences::Break).unwrap();

        assert_eq!(expanded["properties"]["name"], json!({"type": "string"}));
    }

    #[test]
    fn a_reference_inside_an_array_is_expanded_too() {
        let schema = json!({
            "definitions": {"Name": {"type": "string"}},
            "allOf": [{"$ref": "#/definitions/Name"}],
        });

        let expanded = dereference(&schema, &ExternalReferences::Reject, CircularReferences::Break).unwrap();

        assert_eq!(expanded["allOf"][0], json!({"type": "string"}));
    }

    #[test]
    fn an_external_reference_is_read_through_the_resolver() {
        let schema = json!({"properties": {"location": {"$ref": "Point.json#/properties/coordinates"}}});
        let resolver = OneDocument {
            uri: "Point.json",
            document: json!({"properties": {"coordinates": {"type": "array"}}}),
        };

        let expanded = dereference(&schema, &ExternalReferences::Follow(&resolver), CircularReferences::Break).unwrap();

        assert_eq!(expanded["properties"]["location"], json!({"type": "array"}));
    }

    #[test]
    fn an_external_reference_is_refused_when_external_resolution_is_off() {
        let schema = json!({"$ref": "Point.json"});

        assert!(dereference(&schema, &ExternalReferences::Reject, CircularReferences::Break).is_err());
    }

    #[test]
    fn a_cycle_is_broken_rather_than_expanded_forever() {
        let schema = json!({
            "definitions": {"Node": {"properties": {"child": {"$ref": "#/definitions/Node"}}}},
            "properties": {"root": {"$ref": "#/definitions/Node"}},
        });

        let expanded = dereference(&schema, &ExternalReferences::Reject, CircularReferences::Break).unwrap();

        assert_eq!(expanded["properties"]["root"]["properties"]["child"], Value::Null);
    }

    #[test]
    fn a_cycle_can_be_made_an_error_instead() {
        let schema = json!({"definitions": {"Node": {"child": {"$ref": "#/definitions/Node"}}}, "root": {"$ref": "#/definitions/Node"}});

        assert!(dereference(&schema, &ExternalReferences::Reject, CircularReferences::Reject).is_err());
    }

    #[test]
    fn a_reference_to_a_missing_definition_is_rejected() {
        let schema = json!({"properties": {"name": {"$ref": "#/definitions/Absent"}}});

        assert!(dereference(&schema, &ExternalReferences::Reject, CircularReferences::Break).is_err());
    }

    #[test]
    fn a_schema_without_references_is_copied_unchanged() {
        let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});

        let expanded = dereference(&schema, &ExternalReferences::Reject, CircularReferences::Break).unwrap();

        assert_eq!(expanded, schema);
    }
}
