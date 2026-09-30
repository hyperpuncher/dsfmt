use clap::Parser;

/// Format Datastar expressions with OXC, leaving host layout to oxfmt.
#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Args {
    /// Files or directories to format. Reads stdin if not provided.
    pub paths: Vec<std::path::PathBuf>,

    /// Filename used to select the host language when reading stdin.
    #[arg(long, value_name = "PATH", conflicts_with = "paths")]
    pub stdin_filepath: Option<std::path::PathBuf>,

    /// Line width (default: 90)
    #[arg(long, default_value = "90", value_parser = positive_usize)]
    pub line_width: usize,

    /// Use spaces instead of tabs
    #[arg(long)]
    pub use_spaces: bool,

    /// Tab/indent width (default: 4)
    #[arg(long, default_value = "4", value_parser = positive_usize)]
    pub tab_width: usize,

    /// Check only: exit with non-zero if formatting would change
    #[arg(long, conflicts_with = "write")]
    pub check: bool,

    /// Write changes to files (otherwise prints to stdout)
    #[arg(short, long)]
    pub write: bool,
}

fn positive_usize(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| "must be a positive integer".to_string())
}
