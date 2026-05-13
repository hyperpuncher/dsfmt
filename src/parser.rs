use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("parse error: {0}")]
    Parse(String),
}

/// Supported file types for parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Html,
    Tsx,
}

use tree_sitter::Parser;
use tree_sitter_html::LANGUAGE;
use tree_sitter_typescript::LANGUAGE_TSX;

/// Parse source text into a tree-sitter tree.
pub fn parse(source: &str, lang: Lang) -> Result<tree_sitter::Tree, Error> {
    let language = match lang {
        Lang::Html => LANGUAGE.into(),
        Lang::Tsx => LANGUAGE_TSX.into(),
    };

    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| Error::Parse(e.to_string()))?;

    parser
        .parse(source, None)
        .ok_or_else(|| Error::Parse("tree-sitter returned None".to_string()))
}

/// Format a source text. Currently supports HTML and TSX.
pub fn parse_and_format(
    input: &str,
    line_width: usize,
    use_spaces: bool,
    tab_width: usize,
    ext: &str,
) -> String {
    let lang = detect_lang(input, ext);

    match parse(input, lang) {
        Ok(tree) => {
            crate::printer::format_via_splicing(input, &tree, line_width, use_spaces, tab_width)
        }
        Err(_) => input.to_string(),
    }
}

fn detect_lang(input: &str, filename: &str) -> Lang {
    // Use file extension/pattern when available
    if let Some(lang) = lang_from_filename(filename) {
        return lang;
    }
    // Fallback: heuristics for stdin
    if input.contains("=>")
        || input.contains("function ")
        || input.contains("export ")
        || input.contains("import ")
        || input.contains("={`")
    {
        return Lang::Tsx;
    }
    Lang::Html
}

fn lang_from_filename(filename: &str) -> Option<Lang> {
    if filename.ends_with(".tsx") || filename.ends_with(".jsx") {
        return Some(Lang::Tsx);
    }
    if filename.ends_with(".html")
        || filename.ends_with(".htm")
        || filename.ends_with(".heex")
        || filename.ends_with(".templ")
        || filename.ends_with(".blade.php")
    {
        return Some(Lang::Html);
    }
    None
}
