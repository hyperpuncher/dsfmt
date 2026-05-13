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
    let inner = unwrap_value(trimmed).trim();
    let (open_quote, close_quote) = quote_wrap(trimmed);
    let (is_obj, is_arr) = classify_inner(trimmed, inner);

    if is_obj || is_arr {
        let content = &inner[1..inner.len() - 1];
        let items = non_empty_parts(split_top_level(content, &[',']));
        if items.len() <= 1 && trimmed.len() <= line_width {
            p.write(trimmed);
            return;
        }
        p.write(open_quote);
        p.write(if is_obj { "{" } else { "[" });
        for item in &items {
            p.newline(depth + 1);
            format_object_item(p, item, depth + 1, line_width);
        }
        p.newline(depth);
        p.write(if is_obj { "}" } else { "]" });
        p.write(close_quote);
    } else {
        let parts = non_empty_parts(split_top_level(inner, &[';', ',']));
        if parts.len() <= 1 && trimmed.len() <= line_width {
            p.write(trimmed);
            return;
        }
        p.write(open_quote);
        for stmt in &parts {
            p.newline(depth + 1);
            p.write(stmt.trim());
            p.write(";");
        }
        p.newline(depth);
        p.write(close_quote);
    }
}

/// Format a single object entry, recursing into nested objects/arrays.
fn format_object_item(p: &mut Printer, item: &str, depth: usize, line_width: usize) {
    let item = item.trim();
    let colon = find_top_level_colon(item);
    let (key, value) = match colon {
        Some(pos) => (item[..pos].trim(), item[pos + 1..].trim()),
        None => {
            p.write(item);
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
        let nested_items = non_empty_parts(split_top_level(inner, &[',']));
        let total_len = item.len() + depth * 4;
        if nested_items.len() <= 1 && total_len <= line_width {
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

fn classify_inner(trimmed: &str, inner: &str) -> (bool, bool) {
    let wrapped = trimmed.starts_with('"')
        || trimmed.starts_with('\'')
        || trimmed.starts_with("{\"")
        || trimmed.starts_with("{`")
        || trimmed.starts_with('`');
    let is_obj = wrapped && inner.starts_with('{') && inner.ends_with('}');
    let is_arr = wrapped && inner.starts_with('[') && inner.ends_with(']');
    (is_obj, is_arr)
}

fn non_empty_parts(parts: Vec<&str>) -> Vec<&str> {
    parts.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

fn quote_wrap(trimmed: &str) -> (&str, &str) {
    if trimmed.starts_with("{`") {
        ("{`", "`}")
    } else if trimmed.starts_with('`') {
        ("`", "`")
    } else if trimmed.starts_with("{\"") {
        ("{\"", "\"}")
    } else if trimmed.starts_with('"') {
        ("\"", "\"")
    } else if trimmed.starts_with('\'') {
        ("'", "'")
    } else {
        ("", "")
    }
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
    let inner = unwrap_value(trimmed).trim();
    non_empty_parts(split_top_level(inner, &[';', ','])).len() > 1
}

fn unwrap_value(v: &str) -> &str {
    match v {
        _ if v.starts_with("{\"") && v.ends_with("\"}") => &v[2..v.len() - 2],
        _ if v.starts_with("{`") && v.ends_with("`}") => &v[2..v.len() - 2],
        _ if v.starts_with('"') && v.ends_with('"') => &v[1..v.len() - 1],
        _ if v.starts_with('\'') && v.ends_with('\'') => &v[1..v.len() - 1],
        _ if v.starts_with('`') && v.ends_with('`') => &v[1..v.len() - 1],
        _ => v,
    }
}

fn split_top_level<'a>(content: &'a str, seps: &[char]) -> Vec<&'a str> {
    let mut parts: Vec<&'a str> = Vec::new();
    let mut depth = 0u32;
    let mut last = 0;
    for (i, c) in content.char_indices() {
        match c {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 && seps.contains(&c) => {
                parts.push(&content[last..i]);
                last = i + c.len_utf8();
            }
            _ => {}
        }
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
    let mut out = Vec::new();

    for i in 0..children.len() {
        let child = children[i];
        if !matches!(child.kind(), "attribute" | "jsx_attribute") {
            continue;
        }
        let name = extract_attr_name(child, bytes);
        if !is_data_attr(&name) {
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
                break
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

fn is_data_attr(name: &str) -> bool {
    const KNOWN: &[&str] = &[
        "data-bind",
        "data-on",
        "data-show",
        "data-text",
        "data-html",
        "data-class",
        "data-attr",
        "data-indicator",
        "data-persist",
        "data-ref",
        "data-store",
        "data-computed",
        "data-effect",
        "data-signals",
        "data-intersects",
        "data-scroll-into-view",
        "data-view-transition",
        "data-header",
        "data-replace-url",
        "data-style",
    ];
    name.starts_with("data-") && KNOWN.iter().any(|p| name.starts_with(p))
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
