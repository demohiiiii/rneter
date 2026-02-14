use rneter::session::{NormalizeOptions, SessionRecorder};
use std::env;
use std::fs;
use std::process;

fn print_usage() {
    eprintln!(
        "Usage: cargo run --example normalize_fixture -- <input.jsonl> <output.jsonl> [--keep-raw] [--keep-prompt] [--drop-state]"
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        print_usage();
        process::exit(2);
    }

    let input = &args[1];
    let output = &args[2];

    let mut options = NormalizeOptions::default();
    for flag in args.iter().skip(3) {
        match flag.as_str() {
            "--keep-raw" => options.keep_raw_chunks = true,
            "--keep-prompt" => options.keep_prompt_changed = true,
            "--drop-state" => options.keep_state_changed = false,
            "--help" | "-h" => {
                print_usage();
                process::exit(0);
            }
            unknown => {
                eprintln!("Unknown flag: {unknown}");
                print_usage();
                process::exit(2);
            }
        }
    }

    let input_content = match fs::read_to_string(input) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Failed to read input file '{input}': {err}");
            process::exit(1);
        }
    };

    let normalized = match SessionRecorder::normalize_jsonl(&input_content, options) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Failed to normalize recording: {err}");
            process::exit(1);
        }
    };

    if let Err(err) = fs::write(output, normalized) {
        eprintln!("Failed to write output file '{output}': {err}");
        process::exit(1);
    }

    println!("Normalized fixture written to {output}");
}
