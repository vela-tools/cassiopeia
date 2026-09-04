use cassiopeia_ngsi_ld::data_model::DataModel;
use ratatui::widgets::ListState;

/// An entry that can appear in a [`ModelPicker`] list and be matched against a search query.
pub trait PickerEntry {
    /// Whether this entry survives a search for `lowercased_query` (already lower-cased).
    fn matches_query(&self, lowercased_query: &str) -> bool;
}

impl PickerEntry for DataModel {
    fn matches_query(&self, lowercased_query: &str) -> bool {
        self.to_string().to_lowercase().contains(lowercased_query)
    }
}

/// A searchable, selectable list of model entries, shared by the wizard and the explorer.
///
/// It owns the full entry list, the query-narrowed view, the search text, and the highlighted-row
/// state, so both screens drive their model list through the same type instead of duplicating it.
pub struct ModelPicker<T> {
    /// Every entry, before the search query is applied.
    pub all: Vec<T>,
    /// The entries matching the current search query.
    pub filtered: Vec<T>,
    /// The current search query text.
    pub search_query: String,
    /// Which filtered row is highlighted.
    pub list_state: ListState,
}

impl<T: Clone + PickerEntry> ModelPicker<T> {
    /// Builds a picker over `entries` with no row highlighted yet.
    #[must_use]
    pub fn new(entries: Vec<T>) -> ModelPicker<T> {
        ModelPicker {
            filtered: entries.clone(),
            all: entries,
            search_query: String::new(),
            list_state: ListState::default(),
        }
    }

    /// Highlights the first filtered row.
    pub const fn select_first(&mut self) {
        self.list_state.select(Some(0));
    }

    /// Narrows the filtered view to the entries matching the search query, and re-highlights the
    /// first row.
    pub fn update_search(&mut self) {
        let query = self.search_query.to_lowercase();
        self.filtered = self.all.iter().filter(|entry| entry.matches_query(&query)).cloned().collect();
        self.list_state.select(Some(0));
    }

    /// Moves the highlight to the next filtered row, wrapping at the end.
    pub fn move_down(&mut self) {
        let current = self.list_state.selected().unwrap_or(0);
        let next = (current + 1) % self.filtered.len().max(1);
        self.list_state.select(Some(next));
    }

    /// Moves the highlight to the previous filtered row, wrapping at the start.
    pub fn move_up(&mut self) {
        let current = self.list_state.selected().unwrap_or(0);
        let previous = if current == 0 { self.filtered.len().saturating_sub(1) } else { current - 1 };
        self.list_state.select(Some(previous));
    }

    /// The highlighted filtered entry, if any row is highlighted.
    #[must_use]
    pub fn selected_entry(&self) -> Option<&T> {
        self.list_state.selected().and_then(|index| self.filtered.get(index))
    }
}

#[cfg(test)]
mod tests {
    use crate::model_picker::{ModelPicker, PickerEntry};

    #[derive(Clone, PartialEq, Eq, Debug)]
    struct Entry(String);

    impl PickerEntry for Entry {
        fn matches_query(&self, lowercased_query: &str) -> bool {
            self.0.to_lowercase().contains(lowercased_query)
        }
    }

    fn picker() -> ModelPicker<Entry> {
        ModelPicker::new(vec![Entry("Alpha".to_string()), Entry("Beta".to_string()), Entry("Gamma".to_string())])
    }

    #[test]
    fn a_search_narrows_the_filtered_view() {
        let mut picker = picker();
        picker.search_query = "am".to_string();
        picker.update_search();
        assert_eq!(picker.filtered, vec![Entry("Gamma".to_string())]);
    }

    #[test]
    fn movement_wraps_around_the_filtered_rows() {
        let mut picker = picker();
        picker.select_first();
        picker.move_up();
        assert_eq!(picker.list_state.selected(), Some(2));
        picker.move_down();
        assert_eq!(picker.list_state.selected(), Some(0));
    }
}
