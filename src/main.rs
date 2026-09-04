mod cli;
mod core;

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        process::exit(1);
    }

    let mut location: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-l" | "--location" | "-o" | "--output" => {
                if i + 1 >= args.len() {
                    eprintln!("error: missing argument for {}", args[i]);
                    process::exit(1);
                }
                location = Some(args[i + 1].clone());
                i += 2;
            }
            "-h" | "--help" | "help" => {
                print_usage();
                return;
            }
            arg => {
                positional.push(arg.to_string());
                i += 1;
            }
        }
    }

    if positional.is_empty() {
        print_usage();
        process::exit(1);
    }

    let result = match positional[0].as_str() {
        "download" | "add" => {
            if positional.len() < 2 {
                eprintln!("usage: torr download [-l <location>] <torrent_source>");
                process::exit(1);
            }
            cli::commands::add::run(&positional[1], location.as_deref())
        }
        "status" | "info" => {
            if positional.len() < 2 {
                eprintln!("usage: torr status <torrent_source>");
                process::exit(1);
            }
            cli::commands::status::run(&positional[1])
        }
        "peers" => {
            if positional.len() < 2 {
                eprintln!("usage: torr peers <torrent_source>");
                process::exit(1);
            }
            cli::commands::peers::run(&positional[1])
        }
        "verify" => {
            if positional.len() < 3 {
                eprintln!("usage: torr verify <torrent_source> <file_path>");
                process::exit(1);
            }
            cli::commands::verify::run(&positional[1], &positional[2])
        }
        source => {
            cli::commands::add::run(source, location.as_deref())
        }
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn print_usage() {
    println!("torr - bittorrent client");
    println!();
    println!("usage:");
    println!("  torr [-l <location>] <file_or_url>     download torrent directly");
    println!("  torr download [-l <location>] <source> download torrent");
    println!("  torr status <source>                   show torrent metadata");
    println!("  torr peers <source>                    list peers from tracker");
    println!("  torr verify <source> <file>            verify downloaded file");
}
