use cefact_units::UnitCode;

/// How a notation that the sources give to more than one unit is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ruling {
    /// The notation denotes this code.
    Code(UnitCode),
    /// The notation denotes several units in common use, so it resolves to none of them.
    Ambiguous,
}

/// The ruling for every symbol, UCUM code, or UDUNITS code that the sources give to more than one
/// current code, keyed by the notation as written.
///
/// A ruling is needed only after a Recommendation No. 20 code has been preferred to a Recommendation
/// No. 21 package code, so no ruling here settles that. Tests check that each entry still names a
/// notation the sources contest, and that each contested notation has an entry.
pub(crate) const SYMBOL_RULINGS: [(&str, Ruling); 29] = [
    // Recommendation No. 20 and UCUM both give `B` to the bel; only QUDT gives it to the byte, whose
    // Recommendation No. 20 symbol is `byte`.
    ("B", Ruling::Code(UnitCode::M72)),
    // `Bd` is the baud; QUDT's UDUNITS code for bit per second repeats it in error.
    ("Bd", Ruling::Code(UnitCode::J38)),
    // `E16` is a million Btu(IT) per hour; Recommendation No. 20 copies the symbol of `2I` onto it.
    ("BtuIT/h", Ruling::Code(UnitCode::Code2i)),
    ("D", Ruling::Ambiguous),
    ("Gb", Ruling::Ambiguous),
    // The litre, whose symbol `L` the SI Brochure lists, against QUDT's symbol for the lambert.
    ("L", Ruling::Code(UnitCode::Ltr)),
    // `MNJ` is per normalised cubic metre, a reference state its symbol leaves out.
    ("MJ/m\u{B3}", Ruling::Code(UnitCode::Jm)),
    ("MTON", Ruling::Ambiguous),
    // The SI megawatt against the module width Recommendation No. 20 also writes as `MW`.
    ("MW", Ruling::Code(UnitCode::Maw)),
    // `Q27` is newton metre per metre; Recommendation No. 20 gives it the symbol of `M34`.
    ("N\u{B7}m/m\u{B2}", Ruling::Code(UnitCode::M34)),
    // The volt, against the volt AC and volt DC Recommendation No. 20 also writes as `V`.
    ("V", Ruling::Code(UnitCode::Vlt)),
    // UCUM's `a` is the Julian year.
    ("a", Ruling::Code(UnitCode::Ann)),
    // Recommendation No. 20, UCUM, and UDUNITS give `b` to the barn; only QUDT also gives it to the bit.
    ("b", Ruling::Code(UnitCode::A14)),
    ("dwt", Ruling::Ambiguous),
    // The grain, against the gross Recommendation No. 20 also writes as `gr`.
    ("gr", Ruling::Code(UnitCode::Grn)),
    // UCUM's `cal` is the thermochemical calorie, and QUDT agrees; the mean kilocalorie is rare in data.
    ("kcal", Ruling::Code(UnitCode::K53)),
    // `E41` is kilogram-force per square millimetre; Recommendation No. 20 gives it the symbol of `E44`.
    ("kgf\u{B7}m/cm\u{B2}", Ruling::Code(UnitCode::E44)),
    // Knot in UDUNITS, kilotonne in Recommendation No. 20 and UCUM.
    ("kt", Ruling::Ambiguous),
    // `MAR` is the megavar; Recommendation No. 20 gives it the symbol of the kilovar.
    ("kvar", Ruling::Code(UnitCode::Kvr)),
    // The thousandth of an inch, against the angular mil.
    ("mil", Ruling::Code(UnitCode::Code77)),
    // Acidity, against the picohenry: sources that measure what Cassiopeia maps report pH.
    ("pH", Ruling::Code(UnitCode::Q30)),
    // Recommendation No. 20 and UCUM give `pc` to the parsec; only QUDT also gives it to the pica.
    ("pc", Ruling::Code(UnitCode::C63)),
    ("pk", Ruling::Ambiguous),
    ("pt", Ruling::Ambiguous),
    // Recommendation No. 20 lists revolution per minute twice; QUDT reads `rpm` and `RPM` as `M46`.
    ("r/min", Ruling::Code(UnitCode::M46)),
    // The SI radian, against the rad, a unit of absorbed dose the SI no longer accepts.
    ("rad", Ruling::Code(UnitCode::C81)),
    // Recommendation No. 20 lists second per cubic metre radian twice, with no ground to prefer either.
    ("s/(rad\u{B7}m\u{B3})", Ruling::Ambiguous),
    ("st", Ruling::Ambiguous),
    // The tonne, whose symbol `t` the SI Brochure lists; QUDT's UDUNITS code for the short ton repeats it.
    ("t", Ruling::Code(UnitCode::Tne)),
];

/// The ruling for every unit name that the sources give to more than one current code, keyed by the
/// name as written.
pub(crate) const NAME_RULINGS: [(&str, Ruling); 2] = [
    // `WCD` is the cord by name; `M68`, a cord of 128 cubic feet, is the same unit listed again.
    ("cord", Ruling::Code(UnitCode::Wcd)),
    // `A49` is the textile denier QUDT also reads as `denier`; `M83` lists it again.
    ("denier", Ruling::Code(UnitCode::A49)),
];
