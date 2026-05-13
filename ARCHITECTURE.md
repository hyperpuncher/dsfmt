# dsfmt Architecture Plan

## What We Learned From rustfmt and oxfmt

### rustfmt Approach

**Core Data Structures:**

- **`Indent`**: Tracks `block_indent` (tab width × level) + `alignment` (spaces for alignment like after `(`)
- **`Shape`**: Bundles `width` (remaining chars on current line, excluding indent), `indent`, and `offset` (already-emitted text on first line of current statement)
- **`Rewrite` trait**: `fn rewrite(&self, ctx, shape) -> Option<String>` — returns None if content doesn't fit in width
- **`ListFormatting`**: Config builder for formatting comma-separated lists. Has `tactic` (Horizontal/Vertical/Mixed), `separator`, `trailing_separator`, `shape`

**Key Patterns:**

1. **"Try short first, fall back"**: `struct_lit_shape()` computes horizontal and vertical shapes. `struct_lit_tactic()` tries horizontal first (`definitive_tactic` checks if total width ≤ limit). If horizontal doesn't fit, uses vertical.

2. **Composable formatting**: Each AST node implements `Rewrite`. Parent calls `child.rewrite(ctx, child_shape)` and composes results. Shape is passed down, narrowed at each level.

3. **`definitive_tactic()`**: The core algorithm. Takes items + width limit. Computes total width of all items on one line. If fits + no multiline items → Horizontal. Otherwise → Vertical (or Mixed for imports).

4. **`itemize_list()` / `write_list()`**: Iterator-based. `itemize_list` creates `ListItem` structs with pre/post comments. `write_list` formats them according to the tactic, handling separators, indentation, and alignment.

5. **Width tracking is manual but simple**: Each rewrite subtracts from `shape.width`. If width goes negative → error (exceeds max width).

### oxfmt Approach

**Core Data Structures:**

- **`FormatElement` enum**: IR nodes — `Space`, `Line(LineMode)`, `Token { text }`, `Text { text, width }`, `Interned(Interned)`, `BestFitting`, `Tag(Tag)`
- **`LineMode`**: `SoftOrSpace` (becomes space if group fits, newline if not), `Soft` (nothing if fits, newline if not), `Hard` (always newline), `Empty` (always blank line)
- **`Tag`**: `StartIndent`, `EndIndent`, `StartGroup(GroupId)`, `EndGroup`, `StartConditionalContent(PrintMode)`, etc.
- **`Buffer` trait**: Accumulates `FormatElement`s. `VecBuffer` is the main impl.
- **`Formatter`**: Thin wrapper around `Buffer` that provides `join()`, `join_with()`, `group_id()`
- **`Printer`**: Takes a `Document` (list of FormatElements) and prints to string. Handles group fitting, line breaking, indentation.

**Key Patterns:**

1. **IR-based**: Format functions build a tree of `FormatElement`s. The printer decides layout. Separation of concerns.

2. **Group/fits pattern**: `group()` wraps content in `StartGroup`/`EndGroup`. Printer tries flat first — if content exceeds line width, reprints in expanded mode where `SoftOrSpace` → newline.

3. **`best_fitting!` macro**: Provides multiple variants — try the first, if it doesn't fit, try the next, etc. Last variant is always the "fully expanded" fallback.

4. **Indent via tags**: `indent()` wraps content in `StartIndent`/`EndIndent`. Printer increments indent level between these tags.

5. **`join_with()`**: Builder pattern. `f.join_with(separator).entry(&item1).entry(&item2).finish()` — handles separator placement and group wrapping.

## What Our Formatter Needs

### Requirements

1. **Split elements with 2+ `data-*` attrs**: Each attribute on its own line
2. **Split long values**: Object/array values that exceed line width → multi-line
3. **Split template literals**: Multiple top-level `;`/`,` statements → one per line
4. **Preserve everything else**: Non-data attrs, children, surrounding code — untouched
5. **Idempotent**: Formatting twice produces same output

### Why We Don't Need oxfmt's Full IR

- We only reformat opening tags with `data-*` attrs. Everything else is preserved verbatim.
- We don't format arbitrary code — no need for group/fits/best-fitting.
- Our formatting decisions are simple: 2+ data attrs → always split. Value exceeds width → split.

### Why We Don't Need rustfmt's Full Rewrite Trait

- We don't recursively format AST nodes — we only format one level (tag attributes).
- No need for composable `fn rewrite(shape) -> Option<String>` — our formatting always succeeds.

## Proposed Architecture

### Core Data Structure: `Printer`

```rust
struct Printer {
    indent: String,        // "\t" or "    "
    base_depth: usize,     // indent level of the <tag (from source)
    output: String,        // accumulated output
    col: usize,            // current column position
}
```

Key insight from rustfmt: **`Indent` with `block_indent` + `alignment`**. But for dsfmt, we only need block_indent (we don't do visual alignment like after `(` in function calls). Simplified: just track depth as integer.

### Key Methods

- `newline(&mut self, depth: usize)` — writes `\n` + `indent.repeat(depth)`. Updates `col = depth`.
- `write(&mut self, s: &str)` — writes string. Updates `col += s.width()`.
- Writing `<tag` starts at `base_depth`. Attributes at `base_depth + 1`. Value content at `base_depth + 2`.

### Formatting Flow

```
format_tag(node, source_byte_range):
    depth = count_tabs_before(node)
    out = Printer::new(indent, depth)

    out.write("<")
    out.write(tag_name)

    // Non-data attrs inline
    for attr in non_data_attrs:
        out.write(" ")
        out.write(attr)

    // Data attrs, each on new line
    for attr in data_attrs:
        out.newline(depth + 1)
        out.write(attr.name)
        if attr.has_value:
            out.write("=")
            format_value(&mut out, attr.value, depth + 1)

    out.write(">")
    return out.finish()
```

### Value Formatting

```
format_value(out, value, depth):
    unwrapped = strip_quotes(value.trim())

    // Check if object/array in quoted string
    if is_quoted AND unwrapped starts with { or [:
        format_object(out, unwrapped, depth + 1)
    else if has_top_level_separators(unwrapped):
        format_statements(out, unwrapped, depth + 1)
    else:
        out.write(value.trim())  // unchanged
```

### format_object

```
format_object(out, content, depth):
    // content = "a: 1, b: 2, c: 3"
    items = split_top_level(content, [','])  // handles {} and () nesting
    out.write("{")
    out.newline(depth + 1)
    for item in items:
        out.write(item.trim())
        out.write(",")
        out.newline(depth + 1)
    out.write("}")
```

### format_statements

```
format_statements(out, content, depth):
    stmts = split_top_level(content, [';', ','])
    if stmts.len() == 1:
        out.write(content.trim())
        return
    out.newline(depth + 1)
    for stmt in stmts:
        out.write(stmt.trim())
        out.write(";")
        out.newline(depth + 1)
```

### split_top_level

Already implemented and working. Splits on separators only at depth 0 (outside `()`, `{}`, `[]`).

## What We Remove

- ~~`Buf` struct with `at_line_start` boolean~~ → `Printer` with explicit `newline()` + `col` tracking
- ~~`set_level()` / `save_level`~~ → pass depth as parameter to each formatting function
- ~~`detect_depth` duplication~~ → computed once in `format_tag`, passed down
- ~~`format!` with embedded newlines~~ → sequential `write()` + `newline()` calls
- ~~Object wrapping special cases~~ → one `format_object` function, one `format_statements`
- ~~`close_indent` / `content_indent` confusion~~ → depth passed explicitly, always `base_depth + N`

## Simplicity

The entire formatting logic becomes ~200 lines instead of ~400. Each function receives a `Printer` (mutable) and a `depth` (integer). Writes are sequential, indentation is explicit via `newline(depth)`. No state machine, no level tracking, no format strings with embedded newlines.

## Migration Plan

1. Write `Printer` struct with `new`, `write`, `newline`, `finish`
2. Rewrite `format_tag` to use Printer
3. Move value formatting into `format_value`, `format_object`, `format_statements`
4. Keep `split_top_level`, `unwrap_value`, `find_attr_value`, `collect_data_attrs`, etc. as-is
5. Delete old `Buf`, `format_attr_value`, `format_value_into`, `format_template_content`, `format_object_content`
6. Run tests, fix any regressions
