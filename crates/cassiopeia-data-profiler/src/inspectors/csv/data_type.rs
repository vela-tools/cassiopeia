use lazy_regex::regex_is_match;

/// Data types detected in CSV fields during dialect inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    Integer,
    Float,
    Boolean,
    Date,
    Time,
    DateTime,
    Email,
    Url,
    Phone,
    Currency,
    Percentage,
    Text,
    Empty,
}

/// The detection order, most specific first: a value matching several patterns takes the earliest.
const CHECK_ORDER: [DataType; 11] = [
    DataType::DateTime,
    DataType::Date,
    DataType::Time,
    DataType::Email,
    DataType::Url,
    DataType::Currency,
    DataType::Percentage,
    DataType::Phone,
    DataType::Boolean,
    DataType::Integer,
    DataType::Float,
];

impl DataType {
    /// Detects the most specific type that matches `value`, in priority order.
    ///
    /// Blank fields are [`DataType::Empty`]; anything matching no pattern is [`DataType::Text`].
    #[must_use]
    pub fn detect(value: &str) -> DataType {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return DataType::Empty;
        }

        CHECK_ORDER.into_iter().find(|dt| dt.matches(trimmed)).unwrap_or(DataType::Text)
    }

    /// Whether `value` matches this type's detection pattern.
    fn matches(self, value: &str) -> bool {
        match self {
            DataType::Integer => regex_is_match!(r"^[+-]?\d+$", value),
            DataType::Float => regex_is_match!(r"^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$", value),
            DataType::Boolean => regex_is_match!(r"^(?i)(true|false|yes|no|y|n|on|off)$", value),
            DataType::Date => regex_is_match!(r"^(\d{1,4}[-/]\d{1,2}[-/]\d{1,4}|\d{1,2}[-/]\d{1,2}[-/]\d{2,4})$", value),
            DataType::Time => regex_is_match!(r"^([01]?\d|2[0-3]):[0-5]\d(:[0-5]\d)?$", value),
            DataType::DateTime => regex_is_match!(r"^\d{4}[-/]\d{1,2}[-/]\d{1,2}[T ]\d{1,2}:\d{2}(:\d{2})?", value),
            DataType::Email => regex_is_match!(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$", value),
            DataType::Url => regex_is_match!(r"^https?://", value),
            DataType::Phone => regex_is_match!(r"^\+?\d[\d\s()-]{6,20}\d$", value),
            DataType::Currency => regex_is_match!(r"^[$€£¥₹]\s?\d", value),
            DataType::Percentage => regex_is_match!(r"^-?\d+\.?\d*\s?%$", value),
            DataType::Text | DataType::Empty => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::inspectors::csv::data_type::DataType;

    #[test]
    fn blank_fields_are_empty() {
        assert_eq!(DataType::detect("   "), DataType::Empty);
    }

    #[test]
    fn integers_and_floats_are_distinguished() {
        assert_eq!(DataType::detect("42"), DataType::Integer);
        assert_eq!(DataType::detect("3.14"), DataType::Float);
    }

    #[test]
    fn a_datetime_wins_over_a_bare_date() {
        assert_eq!(DataType::detect("2023-01-01T12:00:00"), DataType::DateTime);
        assert_eq!(DataType::detect("2023-01-01"), DataType::Date);
    }

    #[test]
    fn free_text_falls_through_to_text() {
        assert_eq!(DataType::detect("Ljubljana"), DataType::Text);
    }
}
