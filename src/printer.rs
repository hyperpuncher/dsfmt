/// Format source by replacing data-* tag spans with reformatted versions.
/// Everything outside those spans stays byte-for-byte identical.
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
    let bytes = source.as_bytes();
    let root = tree.root_node();

    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    collect_replacements(
        root,
        bytes,
        &indent,
        line_width,
        tab_width,
        &mut replacements,
    );

    if replacements.is_empty() {
        return source.to_string();
    }

    replacements.sort_by_key(|r| r.0);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end, repl) in &replacements {
        out.push_str(&source[cursor..*start]);
        out.push_str(repl);
        cursor = *end;
    }
    out.push_str(&source[cursor..]);
    out
}

fn collect_replacements(
    node: tree_sitter::Node,
    bytes: &[u8],
    indent: &str,
    line_width: usize,
    tab_width: usize,
    out: &mut Vec<(usize, usize, String)>,
) {
    match node.kind() {
        "start_tag" | "jsx_opening_element" | "self_closing_tag" | "jsx_self_closing_element" => {
            collect_data_attr_replacements(node, bytes, indent, line_width, tab_width, out);
        }
        _ => {
            for child in node.children(&mut node.walk()) {
                collect_replacements(child, bytes, indent, line_width, tab_width, out);
            }
        }
    }
}

/// Collect individual data-attr replacement spans for a tag.
/// Each data-* attr gets its own replacement span.
/// Non-data attrs between data-attrs are preserved byte-for-byte.
fn collect_data_attr_replacements(
    tag: tree_sitter::Node,
    bytes: &[u8],
    indent: &str,
    line_width: usize,
    tab_width: usize,
    out: &mut Vec<(usize, usize, String)>,
) {
    let data = collect_data_attrs(tag, bytes);
    if data.is_empty() {
        return;
    }

    let depth = depth_from_source(tag.start_byte(), bytes, tab_width) + 1;

    if !should_split_data_attrs(&data, line_width) {
        return;
    }

    let tag_src = &bytes[tag.start_byte()..tag.end_byte()];
    let was_multiline = tag_src.contains(&b'\n');

    for (i, a) in data.iter().enumerate() {
        let mut p = Printer::new(indent, depth);
        p.write(&a.name);
        if let Some(ref v) = a.value {
            p.write("=");
            format_value(&mut p, v, depth, line_width);
        }
        let formatted = p.finish();

        let replace_start = a.full_start_byte - count_leading_ws(a.full_start_byte, bytes);
        let attr_end = find_attr_node_end_single(tag, a);

        let mut repl = String::new();
        repl.push('\n');
        repl.push_str(&formatted);

        let replace_end = if i == data.len() - 1 {
            if was_multiline {
                repl.push_str(std::str::from_utf8(&bytes[attr_end..tag.end_byte()]).unwrap_or(""));
                tag.end_byte()
            } else {
                repl.push('\n');
                for _ in 0..depth_from_source(tag.start_byte(), bytes, tab_width) {
                    repl.push_str(indent);
                }
                let tag_end = tag.end_byte();
                if tag_end >= 2 && bytes[tag_end - 2] == b'/' {
                    repl.push_str("/>");
                } else {
                    repl.push('>');
                }
                tag.end_byte()
            }
        } else {
            attr_end
        };

        out.push((replace_start, replace_end, repl));
    }
}

/// Count consecutive whitespace bytes before `pos` (scanning backwards).
fn count_leading_ws(pos: usize, bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut p = pos;
    while p > 0 && bytes[p - 1].is_ascii_whitespace() {
        p -= 1;
        count += 1;
    }
    count
}

/// Should data attrs be split to separate lines?
fn should_split_data_attrs(data: &[AttrInfo], line_width: usize) -> bool {
    // Always split if any value needs multi-line formatting
    if data.iter().any(|a| value_needs_split(&a.value, line_width)) {
        return true;
    }
    // Need at least 2 data attrs to consider splitting
    if data.len() < 2 {
        return false;
    }
    // Check if total width of data attrs exceeds line width
    let total: usize = data
        .iter()
        .map(|a| 1 + a.name.len() + a.value.as_ref().map_or(0, |v| 1 + v.len()))
        .sum();
    total > line_width
}

/// Get the end byte of a single attr's tree-sitter node.
fn find_attr_node_end_single(tag: tree_sitter::Node, attr: &AttrInfo) -> usize {
    for child in tag.children(&mut tag.walk()) {
        if child.start_byte() == attr.full_start_byte {
            return child.end_byte();
        }
    }
    tag.end_byte()
}

// ── Printer ────────────────────────────────────────────────────────────────

struct Printer<'a> {
    indent: &'a str,
    output: String,
}

impl<'a> Printer<'a> {
    fn new(indent: &'a str, depth: usize) -> Self {
        let mut s = Self {
            indent,
            output: String::new(),
        };
        s.write_indent(depth);
        s
    }

    fn write(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn newline(&mut self, depth: usize) {
        self.output.push('\n');
        self.write_indent(depth);
    }

    fn write_indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.output.push_str(self.indent);
        }
    }

    fn finish(self) -> String {
        self.output
    }
}

// ── Value formatting ──────────────────────────────────────────────────────

fn format_value(p: &mut Printer, value: &str, depth: usize, line_width: usize) {
    let trimmed = value.trim();
    let value = ValueParts::parse(trimmed);
    let inner = value.inner.trim();

    if value.is_wrapped_object() || value.is_wrapped_array() {
        let content = &inner[1..inner.len() - 1];
        let items = parser_collection_items(inner)
            .unwrap_or_else(|| non_empty_parts(split_top_level(content, &[','])));
        if items.len() <= 1 && trimmed.len() <= line_width {
            p.write(trimmed);
            return;
        }
        p.write(value.open);
        p.write(if value.is_wrapped_object() { "{" } else { "[" });
        for item in &items {
            p.newline(depth + 1);
            format_object_item(p, item, depth + 1, line_width);
        }
        p.newline(depth);
        p.write(if value.is_wrapped_object() { "}" } else { "]" });
        p.write(value.close);
    } else {
        let parts = parser_sequence_parts(inner)
            .unwrap_or_else(|| non_empty_parts(split_top_level(inner, &[';', ','])));
        if parts.len() <= 1 {
            // Try splitting at logical operators
            let expr_parts =
                parser_logical_parts(inner).unwrap_or_else(|| split_at_operators(inner));
            if expr_parts.len() > 1 {
                p.write(value.open);
                for (part, op) in expr_parts.iter() {
                    p.newline(depth + 1);
                    p.write(part);
                    if !op.is_empty() {
                        p.write(" ");
                        p.write(op);
                    }
                }
                p.newline(depth);
                p.write(value.close);
                return;
            }
            // No ;/,/&&/||/?? to split — write inline (JSX expressions, long fn calls, etc.)
            p.write(trimmed);
            return;
        }
        // Multi-part: format as template statements
        p.write(value.open);
        for stmt in &parts {
            p.newline(depth + 1);
            p.write(stmt.trim());
            p.write(";");
        }
        p.newline(depth);
        p.write(value.close);
    }
}

/// Format a single object entry, recursing into nested objects/arrays.
fn format_object_item(p: &mut Printer, item: &str, depth: usize, line_width: usize) {
    let item = item.trim();
    let property_parts = parser_property_parts(item).or_else(|| {
        find_top_level_colon(item).map(|pos| (item[..pos].trim(), item[pos + 1..].trim()))
    });
    // If no key:value property found, treat as plain value (array element)
    let (key, value) = match property_parts {
        Some(parts) => parts,
        None => {
            // Plain value — check if it's a nested object/array to recurse into
            let trimmed_item = item.trim();
            if (trimmed_item.starts_with('{') && trimmed_item.ends_with('}'))
                || (trimmed_item.starts_with('[') && trimmed_item.ends_with(']'))
            {
                let is_obj = trimmed_item.starts_with('{');
                let inner = &trimmed_item[1..trimmed_item.len() - 1];
                let sub_items = parser_collection_items(trimmed_item)
                    .unwrap_or_else(|| non_empty_parts(split_top_level(inner, &[','])));
                let has_nested = sub_items.iter().any(|s| {
                    let s = s.trim();
                    (s.starts_with('{') && s.ends_with('}'))
                        || (s.starts_with('[') && s.ends_with(']'))
                });
                let total_len = trimmed_item.len() + depth * 4;
                if !has_nested && total_len <= line_width {
                    p.write(trimmed_item);
                } else {
                    p.write(if is_obj { "{" } else { "[" });
                    for si in &sub_items {
                        p.newline(depth + 1);
                        format_object_item(p, si, depth + 1, line_width);
                    }
                    p.newline(depth);
                    p.write(if is_obj { "}" } else { "]" });
                }
            } else {
                p.write(item);
            }
            p.write(",");
            return;
        }
    };

    p.write(key);
    p.write(": ");

    if (value.starts_with('{') && value.ends_with('}'))
        || (value.starts_with('[') && value.ends_with(']'))
    {
        let is_obj = value.starts_with('{');
        let inner = &value[1..value.len() - 1];
        let nested_items = parser_collection_items(value)
            .unwrap_or_else(|| non_empty_parts(split_top_level(inner, &[','])));
        let total_len = item.len() + depth * 4;
        // Check if items contain nested objects/arrays
        let has_sub_compound = nested_items.iter().any(|s| {
            let s = s.trim();
            (s.starts_with('{') && s.ends_with('}')) || (s.starts_with('[') && s.ends_with(']'))
        });
        // Keep simple arrays/objects inline if they fit
        if (!has_sub_compound || nested_items.len() <= 1) && total_len <= line_width {
            p.write(value);
        } else {
            p.write(if is_obj { "{" } else { "[" });
            for ni in &nested_items {
                p.newline(depth + 1);
                format_object_item(p, ni, depth + 1, line_width);
            }
            p.newline(depth);
            p.write(if is_obj { "}" } else { "]" });
        }
    } else {
        p.write(value);
    }
    p.write(",");
}

fn find_top_level_colon(s: &str) -> Option<usize> {
    let mut depth = 0u32;
    for (i, c) in s.char_indices() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn non_empty_parts(parts: Vec<&str>) -> Vec<&str> {
    parts.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

// ── Source helpers ─────────────────────────────────────────────────────────

fn depth_from_source(start_byte: usize, bytes: &[u8], tab_width: usize) -> usize {
    let line_start = find_line_start(start_byte, bytes);
    let leading = &bytes[line_start..start_byte];
    let tabs = leading.iter().filter(|&&b| b == b'\t').count();
    if tabs > 0 {
        return tabs;
    }
    let spaces = leading.iter().take_while(|&&b| b == b' ').count();
    spaces / tab_width
}

fn find_line_start(mut pos: usize, bytes: &[u8]) -> usize {
    while pos > 0 {
        pos -= 1;
        if bytes[pos] == b'\n' {
            return pos + 1;
        }
    }
    0
}

// ── Value analysis ─────────────────────────────────────────────────────────

fn value_needs_split(value: &Option<String>, line_width: usize) -> bool {
    let Some(v) = value else { return false };
    let trimmed = v.trim();
    if trimmed.len() > line_width {
        return true;
    }
    let inner = ValueParts::parse(trimmed).inner.trim();
    parser_sequence_parts(inner)
        .map(|parts| parts.len() > 1)
        .unwrap_or_else(|| non_empty_parts(split_top_level(inner, &[';', ','])).len() > 1)
}

fn parser_sequence_parts(content: &str) -> Option<Vec<&str>> {
    let mut parser = datastar_parser()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();
    if root.has_error() {
        return None;
    }

    let sequence = top_level_sequence(root)?;
    let mut parts = Vec::new();
    for child in sequence.named_children(&mut sequence.walk()) {
        let part = content[child.start_byte()..child.end_byte()].trim();
        if !part.is_empty() {
            parts.push(part);
        }
    }

    if parts.len() > 1 { Some(parts) } else { None }
}

fn parser_logical_parts(content: &str) -> Option<Vec<(&str, &str)>> {
    let mut parser = datastar_parser()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();
    if root.has_error() {
        return None;
    }

    let expression = top_level_expression(root)?;
    let mut operands = Vec::new();
    let mut operators = Vec::new();
    flatten_logical_expression(expression, content, &mut operands, &mut operators)?;
    if operators.is_empty() {
        return None;
    }

    let mut parts = Vec::with_capacity(operands.len());
    for (i, operand) in operands.into_iter().enumerate() {
        parts.push((operand.trim(), operators.get(i).copied().unwrap_or("")));
    }
    Some(parts)
}

fn flatten_logical_expression<'a>(
    node: tree_sitter::Node,
    content: &'a str,
    operands: &mut Vec<&'a str>,
    operators: &mut Vec<&'static str>,
) -> Option<()> {
    if node.kind() != "binary_expression" {
        operands.push(content[node.start_byte()..node.end_byte()].trim());
        return Some(());
    }

    let left = node.child(0)?;
    let op = node.child(1)?.kind();
    let right = node.child(2)?;
    let op = match op {
        "&&" => "&&",
        "||" => "||",
        "??" => "??",
        _ => {
            operands.push(content[node.start_byte()..node.end_byte()].trim());
            return Some(());
        }
    };

    flatten_logical_expression(left, content, operands, operators)?;
    operators.push(op);
    flatten_logical_expression(right, content, operands, operators)
}

fn parser_collection_items(content: &str) -> Option<Vec<&str>> {
    let mut parser = datastar_parser()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();
    if root.has_error() {
        return None;
    }

    let collection = top_level_collection(root)?;
    let mut items = Vec::new();
    for child in collection.named_children(&mut collection.walk()) {
        let kind = child.kind();
        if matches!(kind, "property" | "spread_element")
            || (collection.kind() == "array" && kind != "ERROR")
        {
            let item = content[child.start_byte()..child.end_byte()].trim();
            if !item.is_empty() {
                items.push(item);
            }
        }
    }

    Some(items)
}

fn parser_property_parts(item: &str) -> Option<(&str, &str)> {
    let wrapped = format!("{{{item}}}");
    let mut parser = datastar_parser()?;
    let tree = parser.parse(&wrapped, None)?;
    let root = tree.root_node();
    if root.has_error() {
        return None;
    }

    let object = top_level_collection(root)?;
    if object.kind() != "object" {
        return None;
    }

    let property = object
        .named_children(&mut object.walk())
        .find(|child| child.kind() == "property")?;
    let value = property.named_child(1)?;
    let colon = wrapped[..value.start_byte()].rfind(':')?;

    let key_end = colon.saturating_sub(1);
    let key = item.get(..key_end)?.trim();
    let value_start = value.start_byte().saturating_sub(1);
    let value_end = value.end_byte().saturating_sub(1);
    let value = item.get(value_start..value_end)?.trim();

    Some((key, value))
}

fn datastar_parser() -> Option<tree_sitter::Parser> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_datastar::LANGUAGE.into())
        .ok()?;
    Some(parser)
}

fn top_level_expression(root: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let mut candidate = None;
    for child in root.named_children(&mut root.walk()) {
        candidate = Some(child);
        if child.start_byte() == 0 && child.end_byte() == root.end_byte() {
            break;
        }
    }

    let mut node = candidate?;
    while node.named_child_count() == 1
        && matches!(
            node.kind(),
            "expression_statement" | "primary_expression" | "parenthesized_expression"
        )
    {
        node = node.named_child(0)?;
    }
    Some(node)
}

fn top_level_sequence(root: tree_sitter::Node) -> Option<tree_sitter::Node> {
    if root.kind() == "sequence_expression" {
        return Some(root);
    }

    root.named_children(&mut root.walk()).find(|child| {
        child.kind() == "sequence_expression"
            && child.start_byte() == 0
            && child.end_byte() == root.end_byte()
    })
}

fn top_level_collection(root: tree_sitter::Node) -> Option<tree_sitter::Node> {
    for child in root.named_children(&mut root.walk()) {
        if matches!(child.kind(), "object" | "array")
            && child.start_byte() == 0
            && child.end_byte() == root.end_byte()
        {
            return Some(child);
        }
        for grandchild in child.named_children(&mut child.walk()) {
            if matches!(grandchild.kind(), "object" | "array")
                && grandchild.start_byte() == 0
                && grandchild.end_byte() == root.end_byte()
            {
                return Some(grandchild);
            }
        }
    }

    None
}

struct ValueParts<'a> {
    inner: &'a str,
    open: &'static str,
    close: &'static str,
    wrapped: bool,
}

impl<'a> ValueParts<'a> {
    fn parse(value: &'a str) -> Self {
        match value {
            _ if value.starts_with("{\"") && value.ends_with("\"}") => Self {
                inner: &value[2..value.len() - 2],
                open: "{\"",
                close: "\"}",
                wrapped: true,
            },
            _ if value.starts_with("{`") && value.ends_with("`}") => Self {
                inner: &value[2..value.len() - 2],
                open: "{`",
                close: "`}",
                wrapped: true,
            },
            _ if value.starts_with('"') && value.ends_with('"') => Self {
                inner: &value[1..value.len() - 1],
                open: "\"",
                close: "\"",
                wrapped: true,
            },
            _ if value.starts_with('\'') && value.ends_with('\'') => Self {
                inner: &value[1..value.len() - 1],
                open: "'",
                close: "'",
                wrapped: true,
            },
            _ if value.starts_with('`') && value.ends_with('`') => Self {
                inner: &value[1..value.len() - 1],
                open: "`",
                close: "`",
                wrapped: true,
            },
            _ => Self {
                inner: value,
                open: "",
                close: "",
                wrapped: false,
            },
        }
    }

    fn is_wrapped_object(&self) -> bool {
        self.wrapped && self.inner.starts_with('{') && self.inner.ends_with('}')
    }

    fn is_wrapped_array(&self) -> bool {
        self.wrapped && self.inner.starts_with('[') && self.inner.ends_with(']')
    }
}

/// Split a bare expression at `&&`, `||`, `??` (depth-aware).
/// Returns (part, trailing_operator) pairs.
fn split_at_operators(content: &str) -> Vec<(&str, &str)> {
    let mut parts: Vec<(&str, &str)> = Vec::new();
    let mut depth = 0u32;
    let mut last = 0;
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            b'&' | b'|' | b'?' if depth == 0 => {
                let op = match (c, bytes.get(i + 1)) {
                    (b'&', Some(b'&')) => "&&",
                    (b'|', Some(b'|')) => "||",
                    (b'?', Some(b'?')) => "??",
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                let part = content[last..i].trim();
                parts.push((part, op));
                i += 2;
                last = i;
                continue;
            }
            b'"' | b'\'' | b'`' if depth == 0 => {
                let quote = c;
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let last_part = content[last..].trim();
    if !last_part.is_empty() || parts.is_empty() {
        parts.push((last_part, ""));
    }
    parts
}

fn split_top_level<'a>(content: &'a str, seps: &[char]) -> Vec<&'a str> {
    let mut parts: Vec<&'a str> = Vec::new();
    let mut depth = 0u32;
    let mut last = 0;
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            b'"' | b'\'' | b'`' if depth == 0 => {
                let quote = c;
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ if depth == 0 && seps.contains(&(c as char)) => {
                parts.push(&content[last..i]);
                last = i + (c as char).len_utf8();
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&content[last..]);
    parts
}

// ── Attribute parsing ──────────────────────────────────────────────────────

struct AttrInfo {
    name: String,
    value: Option<String>,
    full_start_byte: usize,
}

fn collect_data_attrs(node: tree_sitter::Node, bytes: &[u8]) -> Vec<AttrInfo> {
    let children: Vec<_> = node.children(&mut node.walk()).collect();
    let mut datastar_parser = datastar_attr_parser();
    let mut out = Vec::new();

    for i in 0..children.len() {
        let child = children[i];
        if !matches!(child.kind(), "attribute" | "jsx_attribute") {
            continue;
        }
        let name = extract_attr_name(child, bytes);
        if !is_data_attr(&name, datastar_parser.as_mut()) {
            continue;
        }
        let value =
            find_attr_value(child, bytes).or_else(|| find_value_in_siblings(i, &children, bytes));
        out.push(AttrInfo {
            name,
            value,
            full_start_byte: child.start_byte(),
        });
    }
    out
}

fn extract_attr_name(node: tree_sitter::Node, bytes: &[u8]) -> String {
    if let Some(n) = node.child_by_field_name("name") {
        return n.utf8_text(bytes).unwrap_or("").to_string();
    }

    let mut name = String::new();
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "property_identifier" | ":" | "identifier" => {
                name.push_str(child.utf8_text(bytes).unwrap_or(""));
            }
            "=" | "jsx_expression" | "string" | "template_string" | "quoted_attribute_value" => {
                break;
            }
            _ => {}
        }
    }

    if name.is_empty() {
        let raw = node.utf8_text(bytes).unwrap_or("");
        name = raw.split('=').next().unwrap_or("").trim().to_string();
    }
    name
}

fn datastar_attr_parser() -> Option<tree_sitter::Parser> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_datastar::LANGUAGE.into())
        .ok()?;
    Some(parser)
}

fn is_data_attr(name: &str, parser: Option<&mut tree_sitter::Parser>) -> bool {
    let Some(plugin) = parse_datastar_plugin_name(name, parser) else {
        return false;
    };

    is_known_datastar_plugin(plugin)
}

fn parse_datastar_plugin_name<'a>(
    name: &'a str,
    parser: Option<&mut tree_sitter::Parser>,
) -> Option<&'a str> {
    if !name.starts_with("data-") {
        return None;
    }

    let parser = parser?;
    let tree = parser.parse(name, None)?;
    let root = tree.root_node();
    if root.has_error() {
        return None;
    }

    let attr = root.named_child(0)?;
    if attr.kind() != "datastar_attribute"
        || attr.start_byte() != 0
        || attr.end_byte() != name.len()
    {
        return None;
    }

    for child in attr.named_children(&mut attr.walk()) {
        if child.kind() == "plugin_name" {
            return child.utf8_text(name.as_bytes()).ok();
        }
    }

    None
}

fn is_known_datastar_plugin(plugin: &str) -> bool {
    const KNOWN: &[&str] = &[
        "attr",
        "bind",
        "class",
        "computed",
        "effect",
        "else",
        "else-if",
        "for",
        "header",
        "html",
        "if",
        "indicator",
        "intersects",
        "match-media",
        "on",
        "persist",
        "ref",
        "replace-url",
        "scroll-into-view",
        "show",
        "signals",
        "store",
        "style",
        "text",
        "view-transition",
    ];

    KNOWN.contains(&plugin)
}

fn find_attr_value(node: tree_sitter::Node, bytes: &[u8]) -> Option<String> {
    let children: Vec<_> = node.children(&mut node.walk()).collect();
    if let Some(v) = value_from_children(&children, bytes) {
        return Some(v);
    }
    // JSX: value may be sibling after "="
    if let Some(parent) = node.parent() {
        let siblings: Vec<_> = parent.children(&mut parent.walk()).collect();
        let pos = siblings.iter().position(|s| s.id() == node.id())?;
        find_value_in_siblings(pos, &siblings, bytes)
    } else {
        None
    }
}

fn find_value_in_siblings(
    idx: usize,
    children: &[tree_sitter::Node],
    bytes: &[u8],
) -> Option<String> {
    let mut j = idx + 1;
    if j < children.len() && children[j].kind() == "=" {
        j += 1;
    }
    if j < children.len() {
        value_text(children[j], bytes)
    } else {
        None
    }
}

fn value_text(node: tree_sitter::Node, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "quoted_attribute_value" | "string" | "template_string" => {
            node.utf8_text(bytes).ok().map(|s| s.to_string())
        }
        "jsx_expression" => Some(node.utf8_text(bytes).unwrap_or("").to_string()),
        _ => None,
    }
}

fn value_from_children(children: &[tree_sitter::Node], bytes: &[u8]) -> Option<String> {
    for child in children {
        if let Some(v) = value_text(*child, bytes) {
            return Some(v);
        }
    }
    None
}
