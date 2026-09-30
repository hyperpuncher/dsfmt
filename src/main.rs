#[cfg(test)]
mod tests;

mod config;
mod parser;
mod printer;

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use config::Args;

fn main() -> ExitCode {
    let args = Args::parse();
    let mut failed = false;
    if args.paths.is_empty() {
        match format_stdin(&args) {
            Ok(changed) => failed = changed,
            Err(error) => {
                eprintln!("dsfmt: stdin: {error}");
                failed = true;
            }
        }
    } else {
        for path in &args.paths {
            match std::fs::metadata(path) {
                Ok(metadata) if metadata.is_dir() => failed |= format_dir(path, &args),
                Ok(_) => failed |= process_file(path, &args),
                Err(error) => {
                    eprintln!("dsfmt: {}: {error}", path.display());
                    failed = true;
                }
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn format_text(input: &str, args: &Args, filename: &str) -> String {
    parser::parse_and_format(
        input,
        args.line_width,
        args.use_spaces,
        args.tab_width,
        filename,
    )
}

fn format_stdin(args: &Args) -> io::Result<bool> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let filename = args
        .stdin_filepath
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let output = format_text(&input, args, &filename);
    if args.check {
        let changed = input != output;
        if changed {
            eprintln!("dsfmt: stdin would be reformatted");
        }
        return Ok(changed);
    }
    io::stdout().write_all(output.as_bytes())?;
    Ok(false)
}

fn process_file(path: &Path, args: &Args) -> bool {
    match format_file(path, args) {
        Ok(changed) => changed,
        Err(error) => {
            eprintln!("dsfmt: {}: {error}", path.display());
            true
        }
    }
}

fn format_file(path: &Path, args: &Args) -> io::Result<bool> {
    let filename = path.file_name().unwrap_or_default().to_string_lossy();
    if parser::lang_from_filename(&filename).is_none() {
        return Ok(false);
    }
    let input = std::fs::read_to_string(path)?;
    let output = format_text(&input, args, &filename);
    let changed = input != output;
    if args.check {
        if changed {
            eprintln!("dsfmt: {} would be reformatted", path.display());
        }
        return Ok(changed);
    }
    if args.write {
        if changed {
            write_file(path, &output)?;
        }
    } else {
        io::stdout().write_all(output.as_bytes())?;
    }
    Ok(false)
}

fn write_file(path: &Path, output: &str) -> io::Result<()> {
    // Follow symlinks, preserve permissions, and never truncate the original on failure.
    let path = std::fs::canonicalize(path)?;
    let permissions = std::fs::metadata(&path)?.permissions();
    if permissions.readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is read-only",
        ));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(output.as_bytes())?;
    temporary.as_file().set_permissions(permissions)?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn format_dir(dir: &Path, args: &Args) -> bool {
    let mut failed = false;
    for entry in ignore::WalkBuilder::new(dir).build() {
        match entry {
            Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
                failed |= process_file(entry.path(), args);
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("dsfmt: {error}");
                failed = true;
            }
        }
    }
    failed
}
