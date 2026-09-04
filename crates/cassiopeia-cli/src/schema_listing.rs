use cassiopeia_terminal_style::{
    paint::{join, paint},
    palette::{ACCENT, MUTED, PRIMARY},
    rendering::Rendering,
};

/// Which listing produced a set of schema names: the whole catalog, or the matches for a query.
///
/// Chooses the report's heading and, when the names are empty, keeps that heading with a `no
/// schemas` count rather than falling silent.
#[derive(Debug, Clone, Copy)]
pub enum ListingSource<'a> {
    /// Every schema in the stored catalog.
    All,
    /// The matches for a search query.
    Query(&'a str),
}

/// Renders a schema listing: a heading line naming the source and the count, then one indented name
/// per line. Parameterised over [`Rendering`] so the plain form (for a pipe) and the coloured form
/// (for a terminal) are both testable.
#[must_use]
pub fn render_schema_listing(source: ListingSource, names: &[String], rendering: Rendering) -> String {
    let label = match source {
        ListingSource::All => "Smart Data Models".to_owned(),
        ListingSource::Query(query) => format!("matches for '{query}'"),
    };
    let heading = join(
        &[paint(PRIMARY, &label, rendering), paint(MUTED, &count_label(names.len()), rendering)],
        rendering,
    );

    let mut lines = vec![heading];
    for name in names {
        lines.push(format!("  {}", paint(ACCENT, name, rendering)));
    }
    lines.join("\n")
}

/// The count portion of the heading, pluralised, or `no schemas` when the listing is empty.
fn count_label(count: usize) -> String {
    match count {
        0 => "no schemas".to_owned(),
        1 => "1 schema".to_owned(),
        many => format!("{many} schemas"),
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_listing::{ListingSource, render_schema_listing};
    use cassiopeia_terminal_style::rendering::Rendering;

    const ESCAPE: char = '\u{1b}';

    fn names() -> Vec<String> {
        vec!["Building".to_owned(), "Device".to_owned()]
    }

    #[test]
    fn the_full_catalog_heads_with_the_count_then_indents_each_name() {
        let listing = render_schema_listing(ListingSource::All, &names(), Rendering::Plain);

        assert_eq!(listing, "Smart Data Models \u{b7} 2 schemas\n  Building\n  Device");
    }

    #[test]
    fn a_query_heads_with_the_matches_phrasing() {
        let matches = vec!["WeatherObserved".to_owned()];
        let listing = render_schema_listing(ListingSource::Query("weather"), &matches, Rendering::Plain);

        assert_eq!(listing, "matches for 'weather' \u{b7} 1 schema\n  WeatherObserved");
    }

    #[test]
    fn an_empty_catalog_keeps_the_heading_with_no_schemas() {
        assert_eq!(
            render_schema_listing(ListingSource::All, &[], Rendering::Plain),
            "Smart Data Models \u{b7} no schemas"
        );
    }

    #[test]
    fn an_empty_query_keeps_the_matches_heading_with_no_schemas() {
        assert_eq!(
            render_schema_listing(ListingSource::Query("absent"), &[], Rendering::Plain),
            "matches for 'absent' \u{b7} no schemas"
        );
    }

    #[test]
    fn a_coloured_listing_carries_escape_codes() {
        assert!(render_schema_listing(ListingSource::All, &names(), Rendering::Colored).contains(ESCAPE));
    }
}
