use clap::Parser;

/// Datastar attribute formatter — reflows data-* attributes and template expressions.
#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Args {
    /// Files or directories to format. Reads stdin if not provided.
    pub paths: Vec<String>,

    /// Line width (default: 90)
    #[arg(long, default_value = "90")]
    pub line_width: usize,

    /// Use spaces instead of tabs
    #[arg(long)]
    pub use_spaces: bool,

    /// Tab/indent width (default: 4)
    #[arg(long, default_value = "4")]
    pub tab_width: usize,

    /// Check only: exit with non-zero if formatting would change
    #[arg(long)]
    pub check: bool,

    /// Write changes to files (otherwise prints to stdout)
    #[arg(short, long)]
    pub write: bool,
}
