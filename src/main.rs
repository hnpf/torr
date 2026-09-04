mod cli;
mod core;

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let result = match args[1].as_str() {
        "download" | "add" => {
            if args.len() < 3 {
                eprintln!("usage: tc download <torrent_path> [output_path]");
                process::exit(1);
            }
            let output = if args.len() > 3 { Some(&args[3]) } else { None };
            cli::commands::add::run(&args[2], output)
        }
        "status" | "info" => {
            if args.len() < 3 {
                eprintln!("usage: tc status <torrent_path>");
                process::exit(1);
            }
            cli::commands::status::run(&args[2])
        }
        "peers" => {
            if args.len() < 3 {
                eprintln!("usage: tc peers <torrent_path>");
                process::exit(1);
            }
            cli::commands::peers::run(&args[2])
        }
        "verify" => {
            if args.len() < 4 {
                eprintln!("usage: tc verify <torrent_path> <file_path>");
                process::exit(1);
            }
            cli::commands::verify::run(&args[2], &args[3])
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        unknown => {
            eprintln!("unknown command: {unknown}");
            print_usage();
            process::exit(1);
        }
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn print_usage() {
    println!("tc - bittorrent client");
    println!();
    println!("usage:");
    println!("  tc download <file.torrent> [output]  download a torrent");
    println!("  tc status <file.torrent>             show torrent metadata");
    println!("  tc peers <file.torrent>              fetch and list peers from tracker");
    println!("  tc verify <file.torrent> <file>      verify an existing downloaded file");
}
