#[cfg(test)]
mod tests;

mod config;
mod parser;
mod printer;

use std::io::Read;

use clap::Parser;
use config::Args;

fn main() {
    let args = Args::parse();

    if args.paths.is_empty() {
        // Read from stdin
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .expect("Failed to read stdin");
        let output = format_text(&input, &args);
        print!("{output}");
        return;
    }

    // Walk files
    for path in &args.paths {
        let meta = std::fs::metadata(path);
        match meta {
            Ok(m) if m.is_dir() => {
                format_dir(path, &args);
            }
            Ok(_) => {
                format_file(path, &args);
            }
            Err(e) => {
                eprintln!("dsfmt: {path}: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn format_text(input: &str, args: &Args) -> String {
    parser::parse_and_format(input, args.line_width, args.use_spaces, args.tab_width)
}

fn format_file(path: &str, args: &Args) {
    let input = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dsfmt: {path}: {e}");
            return;
        }
    };
    let output = format_text(&input, args);

    if args.check {
        if input != output {
            eprintln!("dsfmt: {path} would be reformatted");
            std::process::exit(1);
        }
    } else if args.write {
        if input != output {
            std::fs::write(path, output).unwrap_or_else(|e| {
                eprintln!("dsfmt: {path}: {e}");
            });
        }
    } else {
        print!("{output}");
    }
}

fn format_dir(dir: &str, args: &Args) {
    use ignore::types::TypesBuilder;
    use ignore::WalkBuilder;
    let mut types = TypesBuilder::new();
    types.add("html", "*.html").unwrap();
    types.add("jsx", "*.jsx").unwrap();
    types.add("tsx", "*.tsx").unwrap();
    types.add("templ", "*.templ").unwrap();
    types.add("heex", "*.heex").unwrap();
    types.add("blade", "*.blade.php").unwrap();
    let types = types.build().unwrap();
    for entry in WalkBuilder::new(dir).types(types).build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let path = entry.path().to_string_lossy().to_string();
            format_file(&path, args);
        }
    }
}
