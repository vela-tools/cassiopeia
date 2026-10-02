use lazy_regex::regex;

/// The statement tags that may surround a value expression without emitting text of their own.
///
/// A conditional only decides whether the expression renders, and an assignment only names a value
/// the expression can read, so neither changes what the template outputs. Every other statement can:
/// a loop repeats the expression, and a block, macro, or include brings in text of its own.
const GUARD_TAGS: [&str; 6] = ["if", "elif", "else", "endif", "set", "set_global"];

/// Rewrites a template whose whole output is one expression so that it renders the expression's
/// value as JSON, or returns `None` when the template outputs anything else.
///
/// Tera renders an expression's value to text, so an array or object a filter returns (`split`,
/// `json_decode`, `get`) would otherwise reach the caller as its printed form. Encoding the value
/// with `json_encode` and decoding the rendering keeps its type. That is only sound when the
/// rendering is the expression and nothing else, so the template must consist of exactly one `{{ }}`
/// expression, optionally behind [`GUARD_TAGS`] statements and comments, with only whitespace
/// between them. A guard that suppresses the expression renders nothing, which the caller reads as
/// an absent value.
///
/// The expression is parenthesised before the filter is applied, because a filter binds tighter
/// than an operator: `a ~ b | json_encode` would encode `b` alone.
pub(crate) fn typed_form(source: &str) -> Option<String> {
    let tag = regex!(r#"(?s)\{\{(-?)((?:[^"'`]|"[^"]*"|'[^']*'|`[^`]*`)*?)(-?)\}\}|\{%-?\s*([A-Za-z_]+)(?:[^"'`]|"[^"]*"|'[^']*'|`[^`]*`)*?-?%\}|\{#.*?#\}"#);

    let mut expression = None;
    let mut consumed = 0;
    for captures in tag.captures_iter(source) {
        let whole = captures.get(0)?;
        if !source[consumed..whole.start()].trim().is_empty() {
            return None;
        }
        consumed = whole.end();

        if let Some(body) = captures.get(2) {
            let open = captures.get(1).map_or("", |dash| dash.as_str());
            let close = captures.get(3).map_or("", |dash| dash.as_str());
            if expression.replace((whole.range(), open, body.as_str(), close)).is_some() {
                return None;
            }
        } else if let Some(keyword) = captures.get(4)
            && !GUARD_TAGS.contains(&keyword.as_str())
        {
            return None;
        }
    }
    if !source[consumed..].trim().is_empty() {
        return None;
    }

    let (range, open, body, close) = expression?;
    Some(format!(
        "{}{{{{{open} ({}) | json_encode {close}}}}}{}",
        &source[..range.start],
        body.trim(),
        &source[range.end..]
    ))
}

#[cfg(test)]
mod tests {
    use crate::template::value_expression::typed_form;

    #[test]
    fn a_lone_filtered_expression_is_encoded_as_json() {
        assert_eq!(
            typed_form(r#"{{ this[2] | split(pat=" ") }}"#).as_deref(),
            Some(r#"{{ (this[2] | split(pat=" ")) | json_encode }}"#)
        );
    }

    #[test]
    fn a_guarded_expression_keeps_its_guard() {
        assert_eq!(
            typed_form(r#"{% if this[2] %}{{ this[2] | split(pat=" ") }}{% endif %}"#).as_deref(),
            Some(r#"{% if this[2] %}{{ (this[2] | split(pat=" ")) | json_encode }}{% endif %}"#)
        );
    }

    #[test]
    fn whitespace_control_dashes_are_preserved() {
        assert_eq!(typed_form("{{- a | upper -}}").as_deref(), Some("{{- (a | upper) | json_encode -}}"));
    }

    #[test]
    fn an_assignment_before_the_expression_is_a_guard() {
        assert_eq!(
            typed_form("{% set parts = a | split(pat=',') %} {{ parts }}").as_deref(),
            Some("{% set parts = a | split(pat=',') %} {{ (parts) | json_encode }}")
        );
    }

    #[test]
    fn a_comment_beside_the_expression_is_allowed() {
        assert!(typed_form("{# codes #}{{ a | split(pat=' ') }}").is_some());
    }

    #[test]
    fn a_quoted_closing_delimiter_does_not_end_the_expression() {
        assert_eq!(
            typed_form(r#"{{ a | replace(from="}}", to="") }}"#).as_deref(),
            Some(r#"{{ (a | replace(from="}}", to="")) | json_encode }}"#)
        );
    }

    #[test]
    fn literal_text_beside_the_expression_keeps_the_template_textual() {
        assert_eq!(typed_form("Station-{{ id | upper }}"), None);
        assert_eq!(typed_form("{% if a %}{{ a | upper }}{% else %}none{% endif %}"), None);
    }

    #[test]
    fn two_expressions_keep_the_template_textual() {
        assert_eq!(typed_form("{{ a | upper }}{{ b | upper }}"), None);
        assert_eq!(typed_form("{% if a %}{{ a | upper }}{% else %}{{ b | upper }}{% endif %}"), None);
    }

    #[test]
    fn a_loop_keeps_the_template_textual() {
        assert_eq!(typed_form("{% for x in xs %}{{ x | upper }}{% endfor %}"), None);
    }

    #[test]
    fn a_template_with_no_expression_is_not_rewritten() {
        assert_eq!(typed_form("{% if a %}{% endif %}"), None);
    }
}
