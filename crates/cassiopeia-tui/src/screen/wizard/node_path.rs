use crate::screen::wizard::editor_key::EditorKey;

/// A path from the root of the wizard's editing tree to one node.
///
/// The wizard both navigates the tree by this path and keys its per-node schema details on it, so
/// the path is a typed sequence of [`EditorKey`]s rather than a bare `Vec<String>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct NodePath(Vec<EditorKey>);

impl NodePath {
    /// Builds an empty path pointing at the tree root.
    #[must_use]
    pub const fn new() -> NodePath {
        NodePath(Vec::new())
    }

    /// Whether the path points at the tree root.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The last segment of the path, or `None` when it points at the root.
    #[must_use]
    pub fn last(&self) -> Option<&EditorKey> {
        self.0.last()
    }

    /// Appends `key`, descending one level deeper.
    pub fn push(&mut self, key: EditorKey) {
        self.0.push(key);
    }

    /// Removes and returns the last segment, ascending one level.
    pub fn pop(&mut self) -> Option<EditorKey> {
        self.0.pop()
    }

    /// The path reached by descending from this one into `key`.
    #[must_use]
    pub fn child(&self, key: EditorKey) -> NodePath {
        let mut child = self.clone();
        child.push(key);
        child
    }

    /// The path segments, root first.
    #[must_use]
    pub fn segments(&self) -> &[EditorKey] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::screen::wizard::{editor_key::EditorKey, node_path::NodePath};

    fn field(name: &str) -> EditorKey {
        EditorKey::SchemaField(name.to_string())
    }

    #[test]
    fn a_new_path_is_empty_and_grows_and_shrinks() {
        let mut path = NodePath::new();
        assert!(path.is_empty());
        path.push(field("address"));
        assert_eq!(path.last(), Some(&field("address")));
        assert_eq!(path.pop(), Some(field("address")));
        assert!(path.is_empty());
    }

    #[test]
    fn child_descends_without_mutating_the_parent() {
        let root = NodePath::new();
        let child = root.child(field("address"));
        assert!(root.is_empty());
        assert_eq!(child.segments(), &[field("address")]);
    }
}
