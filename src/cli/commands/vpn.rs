use crate::core::vpn;

pub fn run() -> Result<(), String> {
    let ifaces = vpn::list_interfaces();
    println!("torr - network interfaces & VPN status\n");

    if ifaces.is_empty() {
        println!("No network interfaces found.");
        return Ok(());
    }

    println!("{:<4} {:<16} {:<18} {:<6} {}", "", "INTERFACE", "IP ADDRESS", "STATUS", "TYPE");
    println!("{:<4} {:<16} {:<18} {:<6} {}", "", "---------", "----------", "------", "----");

    for iface in &ifaces {
        let ip_str = iface.ip.map(|i| i.to_string()).unwrap_or_else(|| "-".to_string());
        let status = if iface.is_up { "UP" } else { "DOWN" };
        let type_desc = if let Some(ref vt) = iface.vpn_type {
            format!("{vt} (VPN)")
        } else if iface.is_loopback {
            "Loopback".to_string()
        } else {
            "Standard Interface".to_string()
        };

        let marker = if iface.is_vpn && iface.is_up && iface.ip.is_some() {
            "🔒"
        } else {
            "  "
        };

        println!("{:<4} {:<16} {:<18} {:<6} {}", marker, iface.name, ip_str, status, type_desc);
    }

    let active_vpns = vpn::find_active_vpns();
    println!();
    if !active_vpns.is_empty() {
        println!("Active VPN detected: {}", active_vpns.iter().map(|v| v.name.as_str()).collect::<Vec<_>>().join(", "));
        println!("Use '--vpn' or '--bind <interface>' to bind downloads with automatic killswitch.");
    } else {
        println!("No active VPN interface currently detected.");
    }

    Ok(())
}
