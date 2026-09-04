use crate::core::download::ProgressInfo;

pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1024.0 {
        format!("{:.0} B/s", bytes_per_sec)
    } else if bytes_per_sec < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.1} MB/s", bytes_per_sec / (1024.0 * 1024.0))
    }
}

pub fn format_eta(seconds: Option<u64>) -> String {
    match seconds {
        Some(s) if s >= 3600 => format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60),
        Some(s) => format!("{:02}:{:02}", s / 60, s % 60),
        None => "--:--".into(),
    }
}

pub fn render_bar(percent: f64, width: usize) -> String {
    let filled = ((percent / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

pub fn render_hud(info: &ProgressInfo) -> String {
    let percent = if info.total_bytes > 0 {
        (info.downloaded_bytes as f64 / info.total_bytes as f64) * 100.0
    } else {
        0.0
    };

    let bar = render_bar(percent, 14);
    let speed = format_speed(info.speed_bps);
    let eta = format_eta(info.eta_seconds);
    let size_done = format_bytes(info.downloaded_bytes);
    let size_total = format_bytes(info.total_bytes);
    let peer_str = if info.active_peers == 1 { "peer" } else { "peers" };

    format!(
        "{} {:5.1}% • {}/{} • {} • ETA {} ({} {})",
        bar,
        percent,
        size_done,
        size_total,
        speed,
        eta,
        info.active_peers,
        peer_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_scales_properly() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.00 GB");
    }

    #[test]
    fn format_eta_formats_mmss_and_hhmmss() {
        assert_eq!(format_eta(None), "--:--");
        assert_eq!(format_eta(Some(45)), "00:45");
        assert_eq!(format_eta(Some(125)), "02:05");
        assert_eq!(format_eta(Some(3665)), "01:01:05");
    }

    #[test]
    fn render_bar_produces_expected_fill() {
        assert_eq!(render_bar(0.0, 10), "[░░░░░░░░░░]");
        assert_eq!(render_bar(50.0, 10), "[█████░░░░░]");
        assert_eq!(render_bar(100.0, 10), "[██████████]");
    }
}
