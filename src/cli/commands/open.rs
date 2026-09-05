use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

pub fn run(source: &str, interactive_flag: bool, location: Option<&str>) -> Result<(), String> {
    let in_terminal = io::stdin().is_terminal();

    if !in_terminal && !interactive_flag {
        let (term, term_args) = find_terminal()
            .ok_or_else(|| "no terminal emulator found (kitty, alacritty, foot, etc.)".to_string())?;

        let self_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("torr"));

        let mut cmd = Command::new(&term);
        for arg in &term_args {
            cmd.arg(arg);
        }
        cmd.arg(self_exe);
        cmd.arg("open");
        cmd.arg("--interactive");
        cmd.arg(source);
        if let Some(loc) = location {
            cmd.arg("-l");
            cmd.arg(loc);
        }

        cmd.spawn()
            .map_err(|e| format!("failed to launch terminal {term}: {e}"))?;

        process::exit(0);
    }

    println!("torr - torrent launcher");
    if source.starts_with("magnet:") {
        if let Ok(m) = crate::core::magnet::parse_magnet_uri(source) {
            if let Some(name) = m.display_name {
                println!("Name:   {}", name);
            }
            let hash_hex: String = m.info_hash.iter().map(|b| format!("{:02x}", b)).collect();
            println!("Hash:   {}", hash_hex);
        }
    } else {
        println!("Source: {}", source);
    }
    println!();

    let default_dir = default_download_dir();
    let mut chosen_location: Option<String> = location.map(|s| s.to_string());
    let mut chosen_bind: Option<String> = None;

    let active_vpns = crate::core::vpn::find_active_vpns();
    if !active_vpns.is_empty() {
        let vpn_descs: Vec<String> = active_vpns
            .iter()
            .map(|v| {
                let ip_str = v.ip.map(|i| format!(" - {}", i)).unwrap_or_default();
                format!("{} [{}]{}", v.name, v.vpn_type.as_deref().unwrap_or("VPN"), ip_str)
            })
            .collect();
        println!("Detected VPN: {}", vpn_descs.join(", "));
    }
    println!();

    if chosen_location.is_none() {
        println!("Default download directory: {}", default_dir.display());
        println!();
        println!("Options:");
        println!("  [Enter] Download to {}", default_dir.display());
        if !active_vpns.is_empty() {
            println!("  [v]     Download via VPN ({}) with killswitch", active_vpns[0].name);
        }
        println!("  [1]     Download to current directory (.)");
        println!("  [2]     Specify custom download path");
        println!("  [3]     Inspect status / info only");
        println!("  [q]     Cancel");
        println!();
        print!("Select [default: Enter]: ");
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            let trimmed = input.trim();
            match trimmed {
                "" => {
                    chosen_location = Some(default_dir.to_string_lossy().to_string());
                }
                "v" | "V" if !active_vpns.is_empty() => {
                    chosen_location = Some(default_dir.to_string_lossy().to_string());
                    chosen_bind = Some(active_vpns[0].name.clone());
                }
                "1" => {
                    chosen_location = Some(".".to_string());
                }
                "2" => {
                    print!("Enter destination folder: ");
                    let _ = io::stdout().flush();
                    let mut custom = String::new();
                    let _ = io::stdin().read_line(&mut custom);
                    let custom_trimmed = custom.trim();
                    if !custom_trimmed.is_empty() {
                        chosen_location = Some(custom_trimmed.to_string());
                    } else {
                        chosen_location = Some(default_dir.to_string_lossy().to_string());
                    }
                }
                "3" => {
                    let res = crate::cli::commands::status::run(source);
                    if interactive_flag {
                        pause_before_exit();
                    }
                    return res;
                }
                "q" | "Q" | "exit" => {
                    println!("Aborted.");
                    return Ok(());
                }
                other => {
                    println!("Unknown option {:?}, using default directory...", other);
                    chosen_location = Some(default_dir.to_string_lossy().to_string());
                }
            }
        } else {
            chosen_location = Some(default_dir.to_string_lossy().to_string());
        }
    }

    let download_res = crate::cli::commands::add::run(
        source,
        chosen_location.as_deref(),
        chosen_bind.as_deref(),
    );

    if let Err(ref e) = download_res {
        eprintln!("\nError: {e}");
    }

    if interactive_flag {
        pause_before_exit();
    }

    download_res
}

fn pause_before_exit() {
    print!("\nPress Enter to close window...");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}

pub fn default_download_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_DOWNLOAD_DIR") {
        let p = PathBuf::from(dir);
        if p.exists() {
            return p;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join("Downloads");
        if p.exists() {
            return p;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn find_terminal() -> Option<(String, Vec<String>)> {
    if let Ok(term) = std::env::var("TERMINAL") {
        if which(&term) {
            return Some((term, vec!["-e".to_string()]));
        }
    }

    let candidates: [(&str, &[&str]); 9] = [
        ("kitty", &["-T", "torr", "-e"]),
        ("alacritty", &["-T", "torr", "-e"]),
        ("foot", &["-T", "torr"]),
        ("wezterm", &["start", "--title", "torr", "--"]),
        ("gnome-terminal", &["--title=torr", "--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-T", "torr", "-e"]),
        ("x-terminal-emulator", &["-e"]),
        ("xterm", &["-T", "torr", "-e"]),
    ];

    for (term, args) in candidates {
        if which(term) {
            let str_args = args.iter().map(|s| s.to_string()).collect();
            return Some((term.to_string(), str_args));
        }
    }

    None
}

fn which(cmd: &str) -> bool {
    if cmd.contains('/') {
        return Path::new(cmd).is_file();
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(cmd);
            if candidate.is_file() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_standard_utilities() {
        assert!(which("sh"));
    }

    #[test]
    fn default_download_dir_returns_existing_path() {
        let dir = default_download_dir();
        assert!(dir.exists());
    }
}
