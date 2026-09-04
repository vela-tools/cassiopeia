/// A path of tree labels from the root of the explorer's schema tree to one node.
///
/// The explorer's nodes are labelled by display strings supplied to the tree widget, so this path is
/// a sequence of those labels. It is a distinct concept from the wizard's editable
/// [`NodePath`](crate::screen::wizard::node_path::NodePath): the explorer never edits its nodes, so
/// its segments are plain display labels rather than editor keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TreePath(Vec<String>);

impl TreePath {
    /// Builds an empty path pointing at the tree root.
    #[must_use]
    pub const fn new() -> TreePath {
        TreePath(Vec::new())
    }

    /// Builds a path from its label segments, root first.
    #[must_use]
    pub const fn from_segments(segments: Vec<String>) -> TreePath {
        TreePath(segments)
    }

    /// The last label of the path, or `None` when it points at the root.
    #[must_use]
    pub fn last(&self) -> Option<&str> {
        self.0.last().map(String::as_str)
    }

    /// Appends `label`, descending one level deeper.
    pub fn push(&mut self, label: String) {
        self.0.push(label);
    }

    /// The path reached by descending from this one into `label`.
    #[must_use]
    pub fn child(&self, label: String) -> TreePath {
        let mut child = self.clone();
        child.push(label);
        child
    }
}

#[cfg(test)]
mod tests {
    use crate::screen::explorer::tree_path::TreePath;

    #[test]
    fn a_path_reports_its_tail_label() {
        let path = TreePath::from_segments(vec!["Root".to_string(), "address".to_string()]);
        assert_eq!(path.last(), Some("address"));
    }

    #[test]
    fn child_descends_without_mutating_the_parent() {
        let root = TreePath::from_segments(vec!["Root".to_string()]);
        let child = root.child("address".to_string());
        assert_eq!(root.last(), Some("Root"));
        assert_eq!(child.last(), Some("address"));
    }
}
