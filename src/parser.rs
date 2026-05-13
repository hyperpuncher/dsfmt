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

impl Lang {
    #[allow(dead_code)]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "html" | "htm" | "heex" | "blade" | "templ" => Some(Lang::Html),
            "jsx" | "tsx" => Some(Lang::Tsx),
            _ => None,
        }
    }
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
) -> String {
    let lang = detect_lang(input);

    match parse(input, lang) {
        Ok(tree) => {
            crate::printer::format_via_splicing(input, &tree, line_width, use_spaces, tab_width)
        }
        Err(_) => input.to_string(),
    }
}

fn detect_lang(input: &str) -> Lang {
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
