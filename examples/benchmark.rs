//! End-to-end formatter timings with warmed caches and seven measured batches.
//! Run: cargo run --locked --release --example benchmark
#[path = "../src/parser.rs"]
mod parser;
#[path = "../src/printer.rs"]
mod printer;

use std::hint::black_box;
use std::time::Instant;

fn measure(name: &str, source: &str, iterations: usize) {
    let format = || parser::parse_and_format(black_box(source), 90, false, 4, name);
    for _ in 0..10 {
        black_box(format());
    }
    let mut samples = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..iterations {
            black_box(format());
        }
        samples.push(start.elapsed().as_secs_f64() * 1e6 / iterations as f64);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{name:24} {:7} bytes | median {:9.2} us",
        source.len(),
        samples[3]
    );
}

fn main() {
    let mut files: Vec<_> = std::fs::read_dir("tests/fixtures")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    for file in files {
        let name = file.file_name().unwrap().to_str().unwrap();
        let source = std::fs::read_to_string(&file).unwrap();
        measure(name, &source, 200);
    }
    for count in [100, 1000] {
        let attributes: String = (0..count)
            .map(|i| format!(" data-signals:k{i}=\"0\""))
            .collect();
        let source = format!("<div{attributes}></div>");
        measure(&format!("{count}-attributes.html"), &source, 20);
    }
}
