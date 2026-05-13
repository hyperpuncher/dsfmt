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
    collect_replacements(root, bytes, &indent, line_width, &mut replacements);

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
    out: &mut Vec<(usize, usize, String)>,
) {
    match node.kind() {
        "start_tag" | "jsx_opening_element" | "self_closing_tag" | "jsx_self_closing_element" => {
            if let Some(r) = format_tag(node, bytes, indent, line_width) {
                out.push(r);
            }
        }
        _ => {
            for child in node.children(&mut node.walk()) {
                collect_replacements(child, bytes, indent, line_width, out);
            }
        }
    }
}

// ── Tag formatting ────────────────────────────────────────────────────────

/// Collect data-attrs, non-data-attrs, and tag metadata once, then decide
/// whether to split. If splitting, build the reformatted tag.
fn format_tag(
    node: tree_sitter::Node,
    bytes: &[u8],
    indent: &str,
    line_width: usize,
) -> Option<(usize, usize, String)> {
    let data = collect_data_attrs(node, bytes);
    if data.is_empty() {
        return None;
    }

    let tag_name = find_tag_name(node, bytes).unwrap_or("");
    let non_data = collect_non_data_attrs(node, &data, bytes);
    let depth = depth_from_source(node.start_byte(), bytes);
    let line_start = find_line_start(node.start_byte(), bytes);
    let self_closing = bytes[node.start_byte()..node.end_byte()].ends_with(b"/>");

    // Decide: split or not?
    if !should_split_tag(&data, &non_data, tag_name, depth, line_width) {
        return None;
    }

    // Expand span to include leading whitespace so we own the indent
    let leading = &bytes[line_start..node.start_byte()];
    let start = if leading.iter().all(|b| b.is_ascii_whitespace()) {
        line_start
    } else {
        node.start_byte()
    };

    let mut p = Printer::new(indent, depth);

    // Opening tag
    p.write("<");
    p.write(tag_name);

    // Non-data attrs inline
    for a in &non_data {
        p.write(" ");
        p.write(a);
    }

    // Data attrs, one per line
    for a in &data {
        p.newline(depth + 1);
        p.write(&a.name);
        if let Some(ref v) = a.value {
            p.write("=");
            format_value(&mut p, v, depth + 1, line_width);
        }
    }

    if self_closing {
        p.write(" />");
    } else {
        p.write(">");
    }

    Some((start, node.end_byte(), p.finish()))
}

/// Returns true when the tag should be reformatted (split across lines).
fn should_split_tag(
    data: &[AttrInfo],
    non_data: &[String],
    tag_name: &str,
    depth: usize,
    line_width: usize,
) -> bool {
    // Always split if any value needs multi-line formatting
    if data.iter().any(|a| value_needs_split(&a.value, line_width)) {
        return true;
    }

    // With < 2 data attrs, no reason to split
    if data.len() < 2 {
        return false;
    }

    // Compute total width if everything were on one line
    let data_w: usize = data
        .iter()
        .map(|a| 1 + a.name.len() + a.value.as_ref().map_or(0, |v| 1 + v.trim().len()))
        .sum();
    let non_data_w: usize = non_data.iter().map(|a| a.len() + 1).sum();
    let total = depth + 1 + tag_name.len() + non_data_w + data_w + 1; // indent + <tag ... >
    total > line_width
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
    let (is_obj, is_arr) = classify_inner(trimmed, inner);

    if is_obj || is_arr {
        let content = &inner[1..inner.len() - 1];
        let items = non_empty_parts(split_top_level(content, &[',']));
        let (open, close) = quote_wrap(trimmed);

        if items.len() <= 1 && trimmed.len() <= line_width {
            p.write(trimmed);
            return;
        }

        p.write(open);
        p.write(if is_obj { "{" } else { "[" });
        for item in &items {
            p.newline(depth + 1);
            p.write(item.trim());
            p.write(",");
        }
        p.newline(depth);
        p.write(if is_obj { "}" } else { "]" });
        p.write(close);
    } else {
        let parts = non_empty_parts(split_top_level(inner, &[';', ',']));
        if parts.len() <= 1 && trimmed.len() <= line_width {
            p.write(trimmed);
            return;
        }
        let (open, close) = quote_wrap(trimmed);
        p.write(open);
        for stmt in &parts {
            p.newline(depth + 1);
            p.write(stmt.trim());
            p.write(";");
        }
        p.newline(depth);
        p.write(close);
    }
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

fn depth_from_source(start_byte: usize, bytes: &[u8]) -> usize {
    let line_start = find_line_start(start_byte, bytes);
    bytes[line_start..start_byte]
        .iter()
        .filter(|&&b| b == b'\t')
        .count()
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

fn find_tag_name<'a>(node: tree_sitter::Node<'a>, bytes: &'a [u8]) -> Option<&'a str> {
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "tag_name" | "jsx_tag_name" | "jsx_identifier" | "identifier" => {
                return child.utf8_text(bytes).ok();
            }
            _ => {}
        }
    }
    None
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

fn collect_non_data_attrs(tag: tree_sitter::Node, data: &[AttrInfo], bytes: &[u8]) -> Vec<String> {
    let data_starts: Vec<usize> = data.iter().map(|a| a.full_start_byte).collect();
    let mut out = Vec::new();
    for child in tag.children(&mut tag.walk()) {
        if matches!(child.kind(), "attribute" | "jsx_attribute")
            && !data_starts.contains(&child.start_byte())
        {
            if let Ok(t) = child.utf8_text(bytes) {
                out.push(t.to_string());
            }
        }
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
