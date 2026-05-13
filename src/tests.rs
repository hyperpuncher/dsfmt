fn fmt(input: &str, width: usize) -> String {
    crate::parser::parse_and_format(input, width, false, 4)
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
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() > 3, "expected multi-line: {output}");
}

#[test]
fn object_value_in_quotes() {
    let input = "<div data-signals=\"{percentage: 0, contents: foo, bar: baz}\"></div>";
    let output = fmt(input, 40);
    // "{ opens inline, }" closes inline
    assert!(
        output.contains("data-signals=\"{"),
        "missing inline open: {output}"
    );
    assert!(output.contains("}\""), "missing inline close: {output}");
    // Items indented one level deeper than {}
    assert!(output.contains("percentage: 0,"), "missing item: {output}");
}

#[test]
fn template_with_statements() {
    let input = "<div data-effect={`$a = 1, $b = 2`}></div>";
    let output = fmt(input, W);
    assert!(output.contains("$a = 1;"), "missing a=1: {output}");
    assert!(output.contains("$b = 2;"), "missing b=2: {output}");
    // Backtick inline on open
    assert!(output.contains("={`"), "missing backtick: {output}");
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
fn two_attrs_split_narrow() {
    // Too wide at width 40, must split
    let input = r#"<div data-bind:value="$foo" data-show="$visible">hi</div>"#;
    let output = fmt(input, 40);
    assert_eq!(
        output,
        "<div\n\tdata-bind:value=\"$foo\"\n\tdata-show=\"$visible\">hi</div>"
    );
}

#[cfg(test)]
mod bench {
    use std::time::Instant;

    fn fmt(input: &str, width: usize) -> String {
        crate::parser::parse_and_format(input, width, false, 4)
    }

    fn load_fixture(name: &str) -> String {
        std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap()
    }

    #[test]
    fn bench_all_fixtures() {
        let fixtures = [
            "simple.html",
            "complex.html",
            "many_attrs.html",
            "slider.tsx",
            "form.tsx",
        ];

        for name in fixtures {
            let input = load_fixture(name);
            let start = Instant::now();
            let _output = fmt(&input, 90);
            let elapsed = start.elapsed();
            println!("{name}: {:.2?} ({} bytes)", elapsed, input.len());
        }
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
