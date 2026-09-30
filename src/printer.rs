use oxc_formatter::JsFormatOptions;
use oxc_formatter_core::{IndentStyle, IndentWidth, LineWidth};

/// Format only Datastar attribute values. Host layout belongs to oxfmt.
pub fn format_via_splicing(
    source: &str,
    tree: &tree_sitter::Tree,
    line_width: usize,
    use_spaces: bool,
    tab_width: usize,
) -> String {
    let indent = if use_spaces {
        " ".repeat(tab_width)
    } else {
        "\t".to_string()
    };
    let newline = if source
        .find('\n')
        .is_some_and(|i| i > 0 && source.as_bytes()[i - 1] == b'\r')
    {
        "\r\n"
    } else {
        "\n"
    };
    let options = JsFormatOptions {
        indent_style: if use_spaces {
            IndentStyle::Space
        } else {
            IndentStyle::Tab
        },
        indent_width: IndentWidth::try_from(tab_width.min(IndentWidth::MAX as usize) as u8)
            .unwrap(),
        line_width: LineWidth::try_from(
            line_width.clamp(LineWidth::MIN as usize, LineWidth::MAX as usize) as u16,
        )
        .unwrap(),
        ..Default::default()
    };
    let mut replacements = Vec::new();
    collect_replacements(
        tree.root_node(),
        source,
        &options,
        &indent,
        newline,
        &mut replacements,
    );
    splice(source, replacements)
}

fn splice(source: &str, mut replacements: Vec<(usize, usize, String)>) -> String {
    replacements.sort_by_key(|r| r.0);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end, replacement) in replacements {
        out.push_str(&source[cursor..start]);
        out.push_str(&replacement);
        cursor = end;
    }
    out.push_str(&source[cursor..]);
    out
}

fn collect_replacements(
    node: tree_sitter::Node,
    source: &str,
    options: &JsFormatOptions,
    indent: &str,
    newline: &str,
    out: &mut Vec<(usize, usize, String)>,
) {
    if matches!(
        node.kind(),
        "start_tag" | "jsx_opening_element" | "self_closing_tag" | "jsx_self_closing_element"
    ) {
        if node.has_error() {
            return;
        }
        let Some(mut parser) = datastar_parser() else {
            return;
        };
        for attr in node.named_children(&mut node.walk()) {
            if !matches!(attr.kind(), "attribute" | "jsx_attribute") {
                continue;
            }
            let name = attr
                .child_by_field_name("name")
                .or_else(|| attr.named_child(0))
                .and_then(|name| name.utf8_text(source.as_bytes()).ok())
                .unwrap_or("");
            let Some(plugin) = parse_datastar_plugin_name(name, Some(&mut parser)) else {
                continue;
            };
            if !is_known_datastar_plugin(plugin) {
                continue;
            }
            let Some(value_node) = attr.named_children(&mut attr.walk()).find(|child| {
                matches!(
                    child.kind(),
                    "quoted_attribute_value" | "string" | "template_string" | "jsx_expression"
                )
            }) else {
                continue;
            };
            let Ok(value) = value_node.utf8_text(source.as_bytes()) else {
                continue;
            };
            let parts = ValueParts::parse(value);
            // Native JSX expressions already belong to oxfmt, including its quotes.
            if parts.open == "{" || is_literal_plugin(plugin) || contains_entity(value) {
                continue;
            }
            let Some(formatted) = parts.formatted(options) else {
                continue;
            };
            let tab_width = options.indent_width.value() as usize;
            let depth = depth_from_source(node.start_byte(), source.as_bytes(), tab_width) + 1;
            let replacement = parts.render(&formatted, indent, depth, newline);
            if replacement != value {
                out.push((value_node.start_byte(), value_node.end_byte(), replacement));
            }
        }
    } else {
        for child in node.named_children(&mut node.walk()) {
            collect_replacements(child, source, options, indent, newline, out);
        }
    }
}

fn depth_from_source(start: usize, bytes: &[u8], tab_width: usize) -> usize {
    let line_start = bytes[..start]
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |i| i + 1);
    let width = bytes[line_start..start]
        .iter()
        .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
        .fold(0, |column, byte| {
            if *byte == b'\t' {
                column + tab_width - column % tab_width
            } else {
                column + 1
            }
        });
    width / tab_width
}

struct ValueParts<'a> {
    inner: &'a str,
    open: &'static str,
    close: &'static str,
}

impl<'a> ValueParts<'a> {
    fn parse(value: &'a str) -> Self {
        let (open, close) = [
            ("{`", "`}"),
            ("{", "}"),
            ("\"", "\""),
            ("'", "'"),
            ("`", "`"),
        ]
        .into_iter()
        .find(|(open, close)| {
            value.len() >= open.len() + close.len()
                && value.starts_with(open)
                && value.ends_with(close)
        })
        .unwrap_or(("", ""));
        Self {
            inner: &value[open.len()..value.len() - close.len()],
            open,
            close,
        }
    }

    fn formatted(&self, options: &JsFormatOptions) -> Option<FormattedExpression> {
        let mut options = options.clone();
        options.quote_style = if self.open == "'" {
            oxc_formatter::QuoteStyle::Double
        } else {
            oxc_formatter::QuoteStyle::Single
        };
        format_oxc_expression(self.inner.trim(), options).filter(|formatted| {
            !matches!(self.open, "'" | "\"") || !formatted.code.contains(self.open)
        })
    }

    fn render(
        &self,
        expression: &FormattedExpression,
        indent: &str,
        depth: usize,
        newline: &str,
    ) -> String {
        let formatted = &expression.code;
        if !formatted.contains('\n') {
            return format!("{}{}{}", self.open, formatted, self.close);
        }
        // OXC owns every line and relative indent. Only add the attribute envelope.
        let container = (formatted.starts_with("{\n") && formatted.ends_with("\n}"))
            || (formatted.starts_with("[\n") && formatted.ends_with("\n]"));
        let template = matches!(self.open, "{`" | "`");
        let attached = container || (template && !expression.statement_list);
        let base_depth = if attached { depth } else { depth + 1 };
        let mut output = self.open.to_string();
        for (i, line) in formatted.lines().enumerate() {
            if i > 0 || !attached {
                output.push_str(newline);
                output.push_str(&indent.repeat(base_depth));
            }
            output.push_str(line);
        }
        if !attached {
            output.push_str(newline);
            output.push_str(&indent.repeat(depth));
        }
        output.push_str(self.close);
        output
    }
}

struct FormattedExpression {
    code: String,
    statement_list: bool,
}

fn format_oxc_expression(expr: &str, options: JsFormatOptions) -> Option<FormattedExpression> {
    let protected = js_protected_ranges(expr)?;
    if expr.is_empty()
        || has_datastar_only_signal_identifier(expr, &protected)
        || protected
            .iter()
            .any(|range| expr[range.clone()].contains('\n'))
    {
        return None;
    }
    let prepared = replace_datastar_actions(expr, &protected)?;
    let wrapped = wrap_oxc_expression(&prepared.code);
    let allocator = oxc_allocator::Allocator::default();
    let formatted = oxc_formatter::format(
        &allocator,
        &wrapped.code,
        oxc_span::SourceType::mjs(),
        options,
    )
    .ok()?;
    let printed = formatted.print().ok()?.into_code();
    let inner = unwrap_oxc_expression(printed.trim(), wrapped.kind)?;
    // Inspect valid JavaScript before restoring @actions, not recovered DSL syntax.
    let tree = js_tree(&printed)?;
    let root = tree.root_node();
    let statement_list = root
        .named_children(&mut root.walk())
        .filter(|node| !matches!(node.kind(), "comment" | "empty_statement"))
        .count()
        > 1;
    Some(FormattedExpression {
        code: restore_datastar_actions(inner, &prepared.actions)?,
        statement_list,
    })
}

struct WrappedExpression {
    code: String,
    kind: WrappedExpressionKind,
}

#[derive(Clone, Copy)]
enum WrappedExpressionKind {
    ExpressionStatement,
    Program,
    Parenthesized,
}

fn wrap_oxc_expression(expr: &str) -> WrappedExpression {
    if expr.starts_with('{') || expr.starts_with("function") || expr.starts_with("class") {
        WrappedExpression {
            code: format!("({expr});"),
            kind: WrappedExpressionKind::Parenthesized,
        }
    } else if expr.ends_with(';') {
        WrappedExpression {
            code: expr.to_string(),
            kind: WrappedExpressionKind::Program,
        }
    } else {
        WrappedExpression {
            code: format!("{expr};"),
            kind: WrappedExpressionKind::ExpressionStatement,
        }
    }
}

fn unwrap_oxc_expression(code: &str, kind: WrappedExpressionKind) -> Option<&str> {
    match kind {
        WrappedExpressionKind::ExpressionStatement => {
            Some(code.strip_suffix(';').unwrap_or(code).trim())
        }
        WrappedExpressionKind::Program => Some(code),
        WrappedExpressionKind::Parenthesized => {
            code.strip_prefix('(')?.strip_suffix(");").map(str::trim)
        }
    }
}

struct PreparedExpression {
    code: String,
    actions: Vec<(String, String)>,
}

fn replace_datastar_actions(
    expr: &str,
    protected: &[std::ops::Range<usize>],
) -> Option<PreparedExpression> {
    let mut prefix = "__dsfmt_action_".to_string();
    while expr.contains(&prefix) {
        prefix.push('_');
    }
    let mut code = String::with_capacity(expr.len());
    let mut actions = Vec::new();
    let mut chars = expr.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch != '@' || protected.iter().any(|range| range.contains(&idx)) {
            code.push(ch);
            continue;
        }
        let &(_, first) = chars.peek()?;
        if !is_js_ident_start(first) {
            return None;
        }
        let start = idx + ch.len_utf8();
        let mut end = start;
        while let Some(&(next_idx, next)) = chars.peek() {
            if !is_js_ident_continue(next) {
                break;
            }
            chars.next();
            end = next_idx + next.len_utf8();
        }
        let action = &expr[start..end];
        // Keep the action's width when a collision-free identifier is available.
        let placeholder = ["$", "_"]
            .into_iter()
            .map(|prefix| format!("{prefix}{action}"))
            .find(|candidate| !expr.contains(candidate))
            .unwrap_or_else(|| format!("{prefix}{}", actions.len()));
        actions.push((placeholder.clone(), format!("@{action}")));
        code.push_str(&placeholder);
    }
    Some(PreparedExpression { code, actions })
}

fn restore_datastar_actions(code: &str, actions: &[(String, String)]) -> Option<String> {
    if actions.is_empty() {
        return Some(code.to_string());
    }
    let tree = js_tree(code)?;
    let mut replacements = Vec::new();
    collect_action_replacements(tree.root_node(), code, actions, &mut replacements);
    Some(splice(code, replacements))
}

fn collect_action_replacements(
    node: tree_sitter::Node,
    code: &str,
    actions: &[(String, String)],
    out: &mut Vec<(usize, usize, String)>,
) {
    if node.kind() == "identifier"
        && let Ok(name) = node.utf8_text(code.as_bytes())
        && let Some((_, action)) = actions.iter().find(|(placeholder, _)| placeholder == name)
    {
        out.push((node.start_byte(), node.end_byte(), action.clone()));
    }
    for child in node.named_children(&mut node.walk()) {
        collect_action_replacements(child, code, actions, out);
    }
}

fn js_tree(code: &str) -> Option<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .ok()?;
    parser.parse(code, None)
}

fn js_protected_ranges(code: &str) -> Option<Vec<std::ops::Range<usize>>> {
    let tree = js_tree(code)?;
    let mut ranges = Vec::new();
    collect_protected_ranges(tree.root_node(), code, &mut ranges)?;
    Some(ranges)
}

fn collect_protected_ranges(
    node: tree_sitter::Node,
    code: &str,
    ranges: &mut Vec<std::ops::Range<usize>>,
) -> Option<()> {
    if node.kind() == "identifier" && node.utf8_text(code.as_bytes()).ok()?.contains('\\') {
        return None;
    }
    if matches!(
        node.kind(),
        "string" | "string_fragment" | "regex" | "comment"
    ) {
        ranges.push(node.start_byte()..node.end_byte());
    } else {
        for child in node.named_children(&mut node.walk()) {
            collect_protected_ranges(child, code, ranges)?;
        }
    }
    Some(())
}

fn is_js_ident_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
}
fn is_js_ident_continue(ch: char) -> bool {
    is_js_ident_start(ch) || ch.is_ascii_digit()
}

fn has_datastar_only_signal_identifier(expr: &str, protected: &[std::ops::Range<usize>]) -> bool {
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' || protected.iter().any(|range| range.contains(&i)) {
            i += 1;
            continue;
        }
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i < bytes.len()
            && bytes[i] == b'-'
            && i + 1 < bytes.len()
            && (bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_')
        {
            return true;
        }
    }
    false
}

fn contains_entity(value: &str) -> bool {
    value
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'&' && (pair[1].is_ascii_alphabetic() || pair[1] == b'#'))
}

fn datastar_parser() -> Option<tree_sitter::Parser> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_datastar::LANGUAGE.into())
        .ok()?;
    Some(parser)
}

fn parse_datastar_plugin_name<'a>(
    name: &'a str,
    parser: Option<&mut tree_sitter::Parser>,
) -> Option<&'a str> {
    if !name.starts_with("data-") {
        return None;
    }
    // Modifiers do not change the plugin. Parse the base name so a bare
    // plugin followed by __modifiers cannot be lexed as a JS identifier.
    let name = name.split("__").next()?;
    // The published grammar predates nonce and the data-star alias.
    if matches!(name, "data-nonce" | "data-star-nonce") {
        return name
            .strip_prefix("data-star-")
            .or_else(|| name.strip_prefix("data-"));
    }
    let canonical = name
        .strip_prefix("data-star-")
        .map(|suffix| format!("data-{suffix}"));
    let parsed_name = canonical.as_deref().unwrap_or(name);
    let offset = name.len() - parsed_name.len();
    let tree = parser?.parse(parsed_name, None)?;
    let root = tree.root_node();
    if root.has_error() {
        return None;
    }
    let attr = root.named_child(0)?;
    if attr.kind() != "datastar_attribute"
        || attr.start_byte() != 0
        || attr.end_byte() != parsed_name.len()
    {
        return None;
    }
    for child in attr.named_children(&mut attr.walk()) {
        if child.kind() == "plugin_name" {
            return name.get(child.start_byte() + offset..child.end_byte() + offset);
        }
    }
    None
}

fn is_literal_plugin(plugin: &str) -> bool {
    matches!(
        plugin,
        "bind"
            | "ref"
            | "indicator"
            | "match-media"
            | "nonce"
            | "preserve-attr"
            | "ignore"
            | "ignore-morph"
            | "for"
    )
}

fn is_known_datastar_plugin(plugin: &str) -> bool {
    const KNOWN: &[&str] = &[
        "animate",
        "attr",
        "bind",
        "class",
        "computed",
        "custom-validity",
        "effect",
        "else",
        "else-if",
        "for",
        "ignore",
        "ignore-morph",
        "if",
        "indicator",
        "init",
        "json-signals",
        "match-media",
        "nonce",
        "on",
        "on-intersect",
        "on-interval",
        "on-raf",
        "on-resize",
        "on-signal-patch",
        "on-signal-patch-filter",
        "persist",
        "preserve-attr",
        "query-string",
        "ref",
        "replace-url",
        "scroll-into-view",
        "show",
        "signals",
        "style",
        "text",
        "view-transition",
    ];
    KNOWN.contains(&plugin)
}
