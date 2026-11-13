use cassiopeia_collector::sensorthings::limit_reached::LimitReached;
use cassiopeia_sensorthings::offered_readings::offered_reading::OfferedReading;
use std::iter;

/// The number of columns of the readings table.
const COLUMNS: usize = 5;

/// The headings of the readings table, in column order.
const HEADINGS: [&str; COLUMNS] = ["reading", "unit", "unit code", "data streams", "things with repeats"];

/// What the unit columns show for a reading whose data streams have units without text.
const NO_UNIT: &str = "none";

/// The space between two columns.
const GAP: &str = "  ";

/// Renders `readings` as a table: a heading row, then one row per reading in the order given, with
/// every column as wide as its widest cell, so the columns align in a terminal.
///
/// A reading's units and unit codes are each listed once, joined by `, `, the codes in the order of
/// their text.
#[must_use]
pub fn render_readings(readings: &[OfferedReading]) -> String {
    let rows: Vec<[String; COLUMNS]> = iter::once(HEADINGS.map(str::to_owned)).chain(readings.iter().map(cells)).collect();
    let widths = rows.iter().fold([0; COLUMNS], |mut widths, row| {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
        widths
    });
    rows.iter().map(|row| line(row, &widths)).collect::<Vec<_>>().join("\n")
}

/// The note that `limit` stopped the read before the server's last page, so the listing covers only
/// the data streams read before it.
#[must_use]
pub fn render_limit_note(limit: &LimitReached) -> String {
    format!(
        "{} stopped the read of `{}` before the server's last page, so the listing covers only the data streams read before it; the next link `{}` was not followed. Select fewer data streams with `--datastreams`.",
        limit.limit(),
        limit.root(),
        limit.next()
    )
}

/// The cells of the row of `reading`.
fn cells(reading: &OfferedReading) -> [String; COLUMNS] {
    let mut codes: Vec<String> = reading.codes().iter().map(ToString::to_string).collect();
    codes.sort();
    let codes = codes;
    [
        reading.name().to_string(),
        listed(&reading.units().iter().map(ToString::to_string).collect::<Vec<_>>()),
        listed(&codes),
        reading.datastreams().to_string(),
        reading.things_with_repeats().to_string(),
    ]
}

/// `texts` joined by `, `, or [`NO_UNIT`] when there are none.
fn listed(texts: &[String]) -> String {
    if texts.is_empty() { NO_UNIT.to_owned() } else { texts.join(", ") }
}

/// The line of `row`, each cell padded to the width of its column, without trailing space.
fn line(row: &[String; COLUMNS], widths: &[usize; COLUMNS]) -> String {
    let padded: Vec<String> = row.iter().zip(widths).map(|(cell, &width)| format!("{cell:<width$}")).collect();
    padded.join(GAP).trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use crate::readings_table::{render_limit_note, render_readings};
    use cassiopeia_collector::sensorthings::limit_reached::{CollectLimit, LimitReached};
    use cassiopeia_sensorthings::{
        observed_property_name::ObservedPropertyName,
        offered_readings::{offered_reading::OfferedReading, unit_resolution::UnitResolution},
        service_root::ServiceRoot,
        settings::collect_limits::PageLimit,
    };
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;
    use url::Url;

    fn reading(name: &str, units: &[&str], codes: &[UnitResolution], datastreams: usize, things_with_repeats: usize) -> OfferedReading {
        OfferedReading::builder()
            .name(ObservedPropertyName::from(Box::<str>::from(name)))
            .units(units.iter().map(|&unit| Box::from(unit)).collect())
            .codes(codes.iter().copied().collect::<BTreeSet<_>>())
            .datastreams(datastreams)
            .things_with_repeats(things_with_repeats)
            .build()
    }

    #[test]
    fn every_column_is_as_wide_as_its_widest_cell() {
        let readings = [
            reading("no2", &["xyzzy"], &[UnitResolution::Unknown], 1, 0),
            reading("pm10_kal", &["ug/m3"], &[UnitResolution::Code(UnitCode::Gq)], 398, 12),
        ];

        assert_eq!(
            render_readings(&readings),
            "reading   unit   unit code  data streams  things with repeats\n\
             no2       xyzzy  unknown    1             0\n\
             pm10_kal  ug/m3  GQ         398           12"
        );
    }

    #[test]
    fn several_units_and_codes_are_listed_once_each() {
        let codes = [
            UnitResolution::Unknown,
            UnitResolution::Code(UnitCode::Kel),
            UnitResolution::Code(UnitCode::Cel),
        ];
        let rendered = render_readings(&[reading("temperature", &["K", "\u{b0}C", "x"], &codes, 3, 0)]);

        assert_eq!(rendered.lines().nth(1), Some("temperature  K, x, \u{b0}C  CEL, KEL, unknown  3             0"));
    }

    #[test]
    fn a_reading_without_unit_text_shows_none() {
        let rendered = render_readings(&[reading("status", &[], &[], 2, 1)]);

        assert_eq!(rendered.lines().nth(1), Some("status   none  none       2             1"));
    }

    #[test]
    fn a_server_without_readings_lists_the_headings_alone() {
        assert_eq!(render_readings(&[]), "reading  unit  unit code  data streams  things with repeats");
    }

    #[test]
    fn the_limit_note_names_the_limit_the_root_and_the_link_not_followed() {
        let root: ServiceRoot = "https://h.example/v1.1".parse().unwrap();
        let next = Url::parse("https://h.example/v1.1/Datastreams?$skip=1000").unwrap();
        let limit = LimitReached::new(root, CollectLimit::Pages(PageLimit::try_from(1000).unwrap()), next);

        assert_eq!(
            render_limit_note(&limit),
            "`limits.pages: 1000` stopped the read of `https://h.example/v1.1` before the server's last page, so the listing covers only the data streams read before it; the next link `https://h.example/v1.1/Datastreams?$skip=1000` was not followed. Select fewer data streams with `--datastreams`."
        );
    }
}
