fn fmt(input: &str, width: usize) -> String {
    crate::parser::parse_and_format(input, width, false, 4, "")
}

const W: usize = 90;

#[test]
fn single_attr_no_split() {
    let input = r#"<div data-bind:value="$foo">hello</div>"#;
    let output = fmt(input, W);
    assert_eq!(output, input);
}

#[test]
fn two_attrs_fit_inline() {
    // Fits on one line at width W, won't split
    let input = r#"<div data-on:click data-bind:value="$foo">hello</div>"#;
    let output = fmt(input, W);
    assert_eq!(output, input);
}

#[test]
fn self_closing_tag() {
    let input = r#"<input data-bind:value="$foo" data-attr:disabled="true" />"#;
    // Fits on one line at width W, won't split
    let output = fmt(input, W);
    assert_eq!(output, input);
}

#[test]
fn template_literal_split_tsx() {
    let input = "export const X = () => <div data-effect={`$a = 1; $b = 2;`}></div>";
    let output = fmt(input, W);
    assert!(output.contains("$a = 1;"), "missing a=1 in: {output}");
    assert!(output.contains("$b = 2;"), "missing b=2 in: {output}");
    assert!(output.contains("`}"), "missing backtick in: {output}");
}

#[test]
fn nested_elements() {
    let input =
        r#"<div data-on:click data-bind:value="$foo"><span data-text="$bar">text</span></div>"#;
    let output = fmt(input, W);
    assert!(output.contains("data-on:click"));
    assert!(output.contains("data-bind:value"));
    assert!(output.contains("<span data-text=\"$bar\">text</span>"));
}

#[test]
fn complex_effect_expression() {
    let input = "export const X = () => <div data-effect={`$${from} = Math.max(${min}, Math.min($${from}, $${to})); $${to} = Math.max($${from}, Math.min($${to}, ${max}));`}></div>";
    let output = fmt(input, W);
    assert!(output.contains("$${from}"), "missing from: {output}");
    assert!(output.contains("Math.max"), "missing Math.max: {output}");
    assert!(output.contains(";"), "missing semicolons: {output}");
    // Host interpolation is not standalone JavaScript. No handwritten fallback.
    assert_eq!(output, input);
}

#[test]
fn object_value_in_quotes() {
    let input = "<div data-signals=\"{percentage: 0, contents: foo, bar: baz}\"></div>";
    let output = fmt(input, 40);
    assert!(
        output.contains("data-signals=\"{\n\t\tpercentage: 0,"),
        "{output}"
    );
    assert!(output.contains("\n\t}\""), "{output}");
    assert_eq!(fmt(&output, 40), output);
    // The library controls object indentation and trailing commas.
    assert!(output.contains("percentage: 0,"), "missing item: {output}");
}

#[test]
fn template_statement_lists_get_an_indented_body() {
    let input = "export const X = () => <button\n\tdata-on:click={`$themeLabPreference = '${preference}';\n\twindow.piUi.themeLab.setMode('${preference}');\n\t$themeLabMode = window.piUi.themeLab.currentMode();`}\n/>";
    let output = fmt(input, W);
    assert_eq!(
        output,
        "export const X = () => <button\n\tdata-on:click={`\n\t\t$themeLabPreference = '${preference}';\n\t\twindow.piUi.themeLab.setMode('${preference}');\n\t\t$themeLabMode = window.piUi.themeLab.currentMode();\n\t`}\n/>"
    );
    assert_eq!(fmt(&output, W), output);
    let input = "<button data-on:click={`@post('/one'); @post('/two');`} />";
    let output = fmt(input, W);
    assert_eq!(
        output,
        "<button data-on:click={`\n\t\t@post('/one');\n\t\t@post('/two');\n\t`} />"
    );
    assert_eq!(fmt(&output, W), output);
}

#[test]
fn template_statement_and_action_boundaries_stay_attached() {
    let input = "export const X = () => <form\n\tdata-on:submit={`if(el.reportValidity()){$_worktreeError='';@post('${endpoints.branchCreate}',{payload:{branch:$branchName},retryMaxCount:0});}`}\n/>";
    let output = fmt(input, W);
    assert!(
        output.contains("data-on:submit={`if (el.reportValidity()) {\n\t\t$_worktreeError = '';"),
        "{output}"
    );
    assert!(output.contains("\n\t}`}"), "{output}");
    assert_eq!(fmt(&output, W), output);
    assert!(
        !crate::parser::parse(&output, crate::parser::Lang::Tsx)
            .unwrap()
            .root_node()
            .has_error()
    );

    let input = "export const X = () => <button\n\tdata-on:click={`@post('${endpoints.branchDelete}',{payload:{branch:$branchDeleteName},retryMaxCount:0})`}\n/>";
    let output = fmt(input, W);
    assert!(
        output.contains("data-on:click={`@post('${endpoints.branchDelete}', {\n"),
        "{output}"
    );
    assert!(output.contains("\n\t})`}"), "{output}");
    assert_eq!(fmt(&output, W), output);
    let tree = crate::parser::parse(&output, crate::parser::Lang::Tsx).unwrap();
    assert!(!tree.root_node().has_error(), "{output}");
}

#[test]
fn multiline_containers_attach_to_attribute_delimiters() {
    for (open, close) in [("\"", "\""), ("'", "'"), ("{`", "`}")] {
        for expr in [
            "{_worktreeRemovePath: 0, _worktreeRemoveLabel: 0, _worktreeRemoveReady: false, _worktreeRemoveCount: 0}",
            "[firstLongVariableName, secondLongVariableName, thirdLongVariableName]",
        ] {
            let input = format!("<div data-signals__ifmissing={open}{expr}{close} />");
            let output = fmt(&input, 40);
            let (start, end) = if expr.starts_with('{') {
                ('{', '}')
            } else {
                ('[', ']')
            };
            assert!(output.contains(&format!("={open}{start}\n")), "{output}");
            assert!(output.contains(&format!("\n\t{end}{close}")), "{output}");
            assert!(!output.contains(&format!("={open}\n")), "{output}");
            assert_eq!(fmt(&output, 40), output);
        }
    }
}

#[test]
fn object_property_boundaries_use_parser() {
    let input = "<div data-signals=\"{[foo + ':bar']: call(one, two), plain: true}\"></div>";
    let output = fmt(input, 40);
    assert!(
        output.contains("[foo + ':bar']: call(one, two),"),
        "bad computed key or call args: {output}"
    );
    assert!(
        output.contains("plain: true,"),
        "missing plain key: {output}"
    );
}

#[test]
fn comma_expression_keeps_its_semantics() {
    let input = "<div data-effect={`$a = 1, $b = 2`}></div>";
    let output = fmt(input, W);
    assert!(output.contains("(($a = 1), ($b = 2))"), "{output}");
    assert!(
        !output.contains(';'),
        "commas must not become statements: {output}"
    );
    assert_eq!(fmt(&output, W), output);
    // Backtick inline on open
    assert!(output.contains("={`"), "missing backtick: {output}");
}

#[test]
fn oxc_sequence_keeps_nested_commas() {
    let input = "<div data-effect={`@foo($a, $b), $c = {one: 1, two: 2}`}></div>";
    let output = fmt(input, W);
    assert!(
        output.contains("@foo($a, $b),"),
        "split call args: {output}"
    );
    assert!(
        output.contains("$c = { one: 1, two: 2 }"),
        "split object literal: {output}"
    );
}

#[test]
fn parser_backed_logical_split_keeps_parenthesized_expression() {
    let input = "<div data-show=\"$a && ($b || $c) && $d\"></div>";
    let output = fmt(input, 20);
    assert!(output.contains("$a &&"), "missing first operand: {output}");
    assert!(
        output.contains("($b || $c) &&"),
        "split nested logical expression: {output}"
    );
    assert!(output.contains("$d"), "missing final operand: {output}");
}

#[test]
fn trims_and_formats_inline_action_expression() {
    let input = r#"<button data-on:click="   @post('/items')   ">Save</button>"#;
    let output = fmt(input, W);
    assert_eq!(
        output,
        r#"<button data-on:click="@post('/items')">Save</button>"#
    );
}

#[test]
fn formats_assignment_without_extra_parentheses() {
    let input = r#"<button data-on:click="   $open = !$open   ">Toggle</button>"#;
    let output = fmt(input, W);
    assert_eq!(
        output,
        r#"<button data-on:click="$open = !$open">Toggle</button>"#
    );
}

#[test]
fn indents_multiline_block_expression_relative_to_attribute() {
    let input = r#"<input data-on:keydown="
	if (evt.key === 'Enter' && $value) {
		@post('/items');
		$value = '';
	};
" />"#;
    let output = fmt(input, W);
    assert_eq!(
        output,
        "<input data-on:keydown=\"\n\t\tif (evt.key === 'Enter' && $value) {\n\t\t\t@post('/items');\n\t\t\t$value = '';\n\t\t}\n\t\" />"
    );
}

#[test]
fn expression_layout_matches_the_oxc_engine() {
    use oxc_formatter::{JsFormatOptions, QuoteStyle};
    use oxc_formatter_core::{IndentStyle, IndentWidth, LineWidth};

    for (spaces, width) in [(false, 4), (true, 2), (true, 4)] {
        let indent = if spaces {
            " ".repeat(width)
        } else {
            "\t".to_string()
        };
        for expr in [
            "$one && ($two || $three) && $four",
            "$a = 1, call($b, { one: 1, two: 2 }), $c = 3",
            "[$first, { one: 1, two: [1, 2, 3] }, $last]",
            "$longConditionName && $anotherLongConditionName ? 'true' : 'false'",
        ] {
            let allocator = oxc_allocator::Allocator::default();
            let options = JsFormatOptions {
                indent_style: if spaces {
                    IndentStyle::Space
                } else {
                    IndentStyle::Tab
                },
                indent_width: IndentWidth::try_from(width as u8).unwrap(),
                line_width: LineWidth::try_from(40).unwrap(),
                quote_style: QuoteStyle::Single,
                ..Default::default()
            };
            let source = format!("{expr};");
            let printed =
                oxc_formatter::format(&allocator, &source, oxc_span::SourceType::mjs(), options)
                    .unwrap()
                    .print()
                    .unwrap()
                    .into_code();
            let body = printed.trim().strip_suffix(';').unwrap();
            let value = if body.starts_with("[\n") && body.ends_with("\n]") {
                format!(
                    "\"{}\"",
                    body.lines()
                        .enumerate()
                        .map(|(i, line)| if i == 0 {
                            line.to_string()
                        } else {
                            format!("{indent}{line}")
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            } else if body.contains('\n') {
                format!(
                    "\"\n{}\n{}\"",
                    body.lines()
                        .map(|line| format!("{}{line}", indent.repeat(2)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    indent
                )
            } else {
                format!("\"{body}\"")
            };
            let input = format!("<div data-text=\"{expr}\" />");
            let output = crate::parser::parse_and_format(&input, 40, spaces, width, "sample.html");
            assert_eq!(output, format!("<div data-text={value} />"));
            assert_eq!(
                crate::parser::parse_and_format(&output, 40, spaces, width, "sample.html"),
                output
            );
        }
    }
}

#[test]
fn multiline_ternary_uses_separate_quote_boundaries() {
    let input = "export const X = () => <button data-attr:aria-pressed=\"$workspaceReviewPreferences.tab === 'files' || !$_workspaceReviewGitAvailable ? 'true' : 'false'\" />";
    let output = fmt(input, W);
    assert_eq!(
        output,
        "export const X = () => <button data-attr:aria-pressed=\"\n\t\t$workspaceReviewPreferences.tab === 'files' || !$_workspaceReviewGitAvailable\n\t\t\t? 'true'\n\t\t\t: 'false'\n\t\" />"
    );
    assert_eq!(fmt(&output, W), output);
}

#[test]
fn native_jsx_expression_layout_is_owned_by_oxfmt() {
    for input in [
        r#"export const X = ({ item }) => <input data-bind={"item-" + item.id + "-name"} />"#,
        "export const X = () => <div\n\tdata-on:keydown__window={keybindActions(\n\t\t[\"focus-files\", \"window.focusFiles();\"],\n\t\t[\"focus-editor\", \"window.focusEditor();\"],\n\t)}\n/>;",
    ] {
        assert_eq!(fmt(input, W), input);
    }
}

#[test]
fn skips_oxc_for_hyphenated_signal_identifier() {
    let input = r#"<div data-text="   $foo-bar   "></div>"#;
    let output = fmt(input, W);
    assert_eq!(output, input);
}

#[test]
fn preserves_parent_structure() {
    let input = "import { X } from 'y';\nexport const Foo = () => <div data-on:click data-bind:value=\"$x\">hi</div>;\nconst x = 1;";
    let output = fmt(input, W);
    assert!(output.starts_with("import "), "lost imports: {output}");
    assert!(output.contains("export const"), "lost export: {output}");
    assert!(
        output.contains("const x = 1;"),
        "lost trailing code: {output}"
    );
}

#[test]
fn preserves_non_data_attr_multiline_layout() {
    let input = "<button\n    class=\"btn\"\n    data-on:click=\"a\"\n    data-bind:value=\"b\">\n";
    let output = fmt(input, 90);
    assert!(
        !output.contains("<button class"),
        "should not collapse: {output}"
    );
    assert!(
        output.contains("<button\n"),
        "should stay multiline: {output}"
    );
    assert!(
        output.contains("\n    class="),
        "class on own line: {output}"
    );
}

#[test]
fn host_attribute_layout_is_not_reflowed() {
    // oxfmt, not dsfmt, owns host tag layout.
    let input = r#"<div data-bind:value="$foo" data-show="$visible">hi</div>"#;
    let output = fmt(input, 40);
    assert_eq!(output, input);
}

#[test]
fn formats_parser_backed_current_attrs() {
    let input = r#"<div data-match-media:dark="(prefers-color-scheme: dark)" data-signals:user.name="'Ada'">hi</div>"#;
    let output = fmt(input, 40);
    assert_eq!(output, input);
}

#[test]
fn formats_rocket_structural_attrs() {
    let input = r#"<template data-if="$open" data-for="item in $items"></template>"#;
    let output = fmt(input, 40);
    assert_eq!(output, input);
}

#[test]
fn ignores_non_datastar_data_attrs() {
    let input = r#"<div data-testid="save" data-foo="bar">Save</div>"#;
    let output = fmt(input, 20);
    assert_eq!(output, input);
}

#[test]
fn current_and_aliased_attributes_are_formatted() {
    for plugin in [
        "init",
        "on-intersect",
        "on-interval",
        "on-signal-patch",
        "on-signal-patch-filter",
        "on-raf",
        "on-resize",
        "query-string",
        "animate",
        "custom-validity",
        "json-signals",
        "ignore",
        "ignore-morph",
        "preserve-attr",
        "nonce",
    ] {
        for prefix in ["data-", "data-star-"] {
            let input =
                format!("<div {prefix}{plugin}=\"   $count+1   \" data-text=\"$count\"></div>");
            let output = fmt(&input, W);
            let literal = matches!(
                plugin,
                "ignore" | "ignore-morph" | "preserve-attr" | "nonce"
            );
            let value = if literal {
                "   $count+1   "
            } else {
                "$count + 1"
            };
            assert!(
                output.contains(&format!("{prefix}{plugin}=\"{value}\"")),
                "{input}: {output}"
            );
            assert_eq!(fmt(&output, W), output);
        }
    }
}

#[test]
fn three_sequence_parts_keep_nested_calls_and_objects() {
    let input = "<div data-effect={`$a = 1, @post('/items', {a: 1, b: 2}), $b = 2`}></div>";
    let output = fmt(input, W);
    assert!(output.contains("($a = 1),"), "{output}");
    assert!(
        output.contains("@post('/items', { a: 1, b: 2 }),"),
        "{output}"
    );
    assert!(output.contains("($b = 2)"), "{output}");
    assert!(!output.contains(';'), "{output}");
    assert_eq!(fmt(&output, W), output);
}

#[test]
fn retired_and_unrelated_data_attributes_are_untouched() {
    for name in [
        "data-header",
        "data-html",
        "data-store",
        "data-intersects",
        "data-rocket",
        "data-testid",
    ] {
        let input = format!("<div {name}=\" a, b; c \" />");
        assert_eq!(fmt(&input, 10), input);
    }
}

#[test]
fn trailing_ordinary_attributes_are_not_deleted() {
    let input = r#"<div data-text="$veryLongSignalName" class="keep" title="🙂" />"#;
    let output = fmt(input, 10);
    assert!(output.contains("class=\"keep\" title=\"🙂\""), "{output}");
    assert!(output.ends_with("/>"));
    assert_eq!(fmt(&output, 10), output);
}

#[test]
fn unquoted_values_and_literal_attributes_are_preserved() {
    for input in [
        r#"<div data-text=🙂 data-show=$open />"#,
        r#"<input data-bind="search-input" />"#,
        r#"<div data-ref="search-input" />"#,
        r#"<div data-indicator="in-flight" />"#,
        r#"<div data-match-media="(min-width: 600px), (max-width: 900px)" />"#,
        r#"<div data-nonce="abc-def+123/==" />"#,
        r#"<template data-for="(item, index) of $items" />"#,
    ] {
        let output = fmt(input, 30);
        if input.contains("data-text=") {
            assert!(output.contains("data-text=🙂"), "{output}");
            assert!(output.contains("data-show=$open"), "{output}");
        } else {
            assert_eq!(output, input);
        }
        assert_eq!(fmt(&output, 30), output);
    }
}

#[test]
fn single_quoted_html_attributes_keep_valid_delimiters() {
    let input = r#"<button data-on:click=' @post("/items") '>save</button>"#;
    let output = fmt(input, W);
    assert_eq!(
        output,
        r#"<button data-on:click='@post("/items")'>save</button>"#
    );
    assert_eq!(fmt(&output, W), output);
}

#[test]
fn action_placeholders_do_not_replace_literal_or_user_identifiers() {
    let input = r#"<div data-on:click="@post('/items','__dsfmt_action_0','email@example')+__dsfmt_action_0()" />"#;
    let output = fmt(input, 320);
    assert_ne!(output, input);
    assert!(output.contains("'__dsfmt_action_0'"), "{output}");
    assert!(output.contains("__dsfmt_action_0()"), "{output}");
    assert!(output.contains("'email@example'"), "{output}");
    assert!(output.contains("@post("), "{output}");
    assert_eq!(fmt(&output, 320), output);
}

#[test]
fn more_than_ten_actions_round_trip_without_prefix_matches() {
    let names = [
        "get", "post", "put", "patch", "delete", "foo", "bar", "baz", "qux", "zot", "finalize",
        "finish",
    ];
    let calls = names.map(|name| format!("@{name}()"));
    let input = format!("<div data-text=\"{}\" />", calls.join("+"));
    let output = fmt(&input, 320);
    assert_ne!(output, input);
    for call in calls {
        assert!(output.contains(&call), "{output}");
    }
    assert!(!output.contains("__dsfmt_action"), "{output}");
    assert_eq!(fmt(&output, 320), output);
}

#[test]
fn comments_and_multiline_literal_contents_are_not_lost() {
    let input = r#"<div data-signals="{a: 1, /* keep @literal */ b: 2}" />"#;
    let output = fmt(input, 30);
    assert!(output.contains("/* keep @literal */"), "{output}");
    assert_eq!(fmt(&output, 30), output);
    let input = "<div data-text=\"`hello\n    world`\" />";
    let output = fmt(input, 20);
    assert!(output.contains("`hello\n    world`"), "{output}");
    assert_eq!(fmt(&output, 20), output);
}

#[test]
fn jsx_expression_braces_do_not_receive_statement_semicolons() {
    let input = "export const X = () => <div data-attr:pair={$a, $b} />";
    let output = fmt(input, 10);
    let tree = crate::parser::parse(&output, crate::parser::Lang::Tsx).unwrap();
    assert!(!tree.root_node().has_error(), "{output}");
    assert!(output.contains("$a, $b"), "{output}");
    assert!(!output.contains(';'), "{output}");
    assert_eq!(fmt(&output, 10), output);
}

#[test]
fn encoded_expressions_and_malformed_tags_are_left_alone() {
    for input in [
        r#"<div data-text="&quot;$count&quot;" />"#,
        r#"<div data-signals="{a:1,b:2""#,
    ] {
        assert_eq!(fmt(input, 10), input);
    }
}

#[test]
fn delimiters_inside_calls_and_regexes_are_not_statement_boundaries() {
    for input in [
        r#"<div data-text="fn('), hello; && world')" />"#,
        r#"<div data-show="/),hello;&&/.test($value)" />"#,
    ] {
        let output = fmt(input, W);
        assert!(!output.contains('\n'), "{output}");
        assert_eq!(fmt(&output, W), output);
        assert!(
            output.contains("),hello;&&") || output.contains("), hello; && world"),
            "{output}"
        );
    }
}

#[test]
fn jsx_spreads_do_not_become_boolean_attribute_values() {
    let input = "export const X = () => <div data-ignore {...props} data-show=\"$open\" />";
    let output = fmt(input, 20);
    assert_eq!(output.matches("{...props}").count(), 1, "{output}");
    assert!(!output.contains("data-ignore="), "{output}");
    assert!(
        !crate::parser::parse(&output, crate::parser::Lang::Tsx)
            .unwrap()
            .root_node()
            .has_error(),
        "{output}"
    );
    assert_eq!(fmt(&output, 20), output);
}

#[test]
fn generated_lines_keep_crlf_endings() {
    let input = "\r\n<div data-effect=\"$a=1; $b=2;\" />\r\n";
    let output = fmt(input, 30);
    assert!(output.contains("\r\n\t\t$a = 1;"), "{output:?}");
    assert!(!output.replace("\r\n", "").contains('\n'), "{output:?}");
    assert_eq!(fmt(&output, 30), output);
}

#[test]
fn ordinary_tag_layout_is_preserved_at_any_width() {
    let input = r#"<custom-element class="long-css-class" data-show="$a" data-text="$b" />"#;
    let output = fmt(input, 60);
    assert_eq!(output, input);
    assert_eq!(fmt(&output, 60), output);
}

#[test]
fn indentation_uses_only_leading_whitespace_and_respects_mixed_tabs() {
    let input = "\t    <div data-effect=\"$a=1; $b=2;\" />";
    let output = fmt(input, 30);
    assert!(output.contains("\n\t\t\t\t$a = 1;"), "{output:?}");
    assert_eq!(fmt(&output, 30), output);
    let input =
        "\texport const label = '\t'; export const X = () => <div data-effect=\"$a=1; $b=2;\" />";
    let output = fmt(input, 30);
    assert!(output.contains("\n\t\t\t$a = 1;"), "{output:?}");
    assert!(!output.contains("\n\t\t\t\t$a = 1;"), "{output:?}");
}

#[test]
fn unicode_values_use_character_width_not_utf8_byte_length() {
    let input = format!(
        "<div data-text=\"'{}'\" data-show=\"$ok\" />",
        "🙂".repeat(15)
    );
    assert_eq!(fmt(&input, input.chars().count()), input);
}

#[cfg(test)]
mod fixtures {
    use super::fmt;

    fn load_fixture(name: &str) -> String {
        std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap()
    }

    #[test]
    fn fixtures_idempotent() {
        let fixtures = [
            "simple.html",
            "complex.html",
            "many_attrs.html",
            "slider.tsx",
            "form.tsx",
        ];

        for name in fixtures {
            let input = load_fixture(name);
            let pass1 = fmt(&input, 90);
            let pass2 = fmt(&pass1, 90);
            assert_eq!(
                pass1, pass2,
                "{name} is not idempotent!\n--- pass1 ---\n{pass1}\n--- pass2 ---\n{pass2}"
            );
        }
    }
}
