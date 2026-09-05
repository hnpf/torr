use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::os::unix::io::FromRawFd;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub name: String,
    pub ip: Option<IpAddr>,
    pub is_up: bool,
    pub is_loopback: bool,
    pub is_vpn: bool,
    pub vpn_type: Option<String>,
}

pub fn classify_vpn(name: &str) -> (bool, Option<String>) {
    let lower = name.to_ascii_lowercase();

    if lower.starts_with("proton") || lower.starts_with("pvpn") {
        return (true, Some("Proton VPN".to_string()));
    }
    if lower.starts_with("wg") || lower.starts_with("wireguard") {
        return (true, Some("WireGuard".to_string()));
    }
    if lower.starts_with("nordlynx") {
        return (true, Some("NordVPN".to_string()));
    }
    if lower.starts_with("mullvad") {
        return (true, Some("Mullvad".to_string()));
    }
    if lower.starts_with("tailscale") {
        return (true, Some("Tailscale".to_string()));
    }
    if lower.starts_with("ivpn") {
        return (true, Some("IVPN".to_string()));
    }
    if lower.starts_with("windscribe") {
        return (true, Some("Windscribe".to_string()));
    }
    if lower.starts_with("surfshark") {
        return (true, Some("Surfshark".to_string()));
    }
    if lower.starts_with("pia") {
        return (true, Some("Private Internet Access".to_string()));
    }
    if lower.starts_with("tun") || lower.starts_with("tap") {
        return (true, Some("OpenVPN / TUN".to_string()));
    }

    let sys_path = format!("/sys/class/net/{}/tun_flags", name);
    if Path::new(&sys_path).exists() {
        return (true, Some("Generic TUN/TAP".to_string()));
    }

    let type_path = format!("/sys/class/net/{}/type", name);
    if let Ok(content) = std::fs::read_to_string(&type_path) {
        let type_num = content.trim();
        if type_num == "65534" || type_num == "512" {
            return (true, Some("Point-to-Point VPN".to_string()));
        }
    }

    (false, None)
}

pub fn list_interfaces() -> Vec<InterfaceInfo> {
    let mut ifaces = Vec::new();
    let mut ifaddrs_ptr: *mut libc::ifaddrs = std::ptr::null_mut();

    unsafe {
        if libc::getifaddrs(&mut ifaddrs_ptr) != 0 || ifaddrs_ptr.is_null() {
            return ifaces;
        }

        let mut curr = ifaddrs_ptr;
        while !curr.is_null() {
            let ifa = &*curr;

            if !ifa.ifa_name.is_null() {
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name)
                    .to_string_lossy()
                    .into_owned();

                let is_up = (ifa.ifa_flags & (libc::IFF_UP as u32)) != 0;
                let is_loopback = (ifa.ifa_flags & (libc::IFF_LOOPBACK as u32)) != 0;

                let mut ip = None;
                if !ifa.ifa_addr.is_null() {
                    let family = (*ifa.ifa_addr).sa_family as libc::c_int;
                    if family == libc::AF_INET {
                        let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                        let octets = sin.sin_addr.s_addr.to_ne_bytes();
                        ip = Some(IpAddr::V4(Ipv4Addr::from(octets)));
                    } else if family == libc::AF_INET6 {
                        let sin6 = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                        let octets = sin6.sin6_addr.s6_addr;
                        ip = Some(IpAddr::V6(Ipv6Addr::from(octets)));
                    }
                }

                let (is_vpn, vpn_type) = classify_vpn(&name);

                if !ifaces.iter().any(|existing: &InterfaceInfo| existing.name == name && existing.ip == ip) {
                    ifaces.push(InterfaceInfo {
                        name,
                        ip,
                        is_up,
                        is_loopback,
                        is_vpn,
                        vpn_type,
                    });
                }
            }

            curr = ifa.ifa_next;
        }

        libc::freeifaddrs(ifaddrs_ptr);
    }

    ifaces
}

pub fn find_active_vpns() -> Vec<InterfaceInfo> {
    list_interfaces()
        .into_iter()
        .filter(|iface| iface.is_vpn && iface.is_up && iface.ip.is_some())
        .collect()
}

pub fn find_active_vpn() -> Option<InterfaceInfo> {
    find_active_vpns().into_iter().next()
}

pub fn resolve_bind_interface(target: &str) -> Result<InterfaceInfo, String> {
    let target_lower = target.trim().to_ascii_lowercase();

    if target_lower == "vpn" || target_lower == "auto" {
        return find_active_vpn()
            .ok_or_else(|| "no active VPN interface detected (ProtonVPN, WireGuard, tun0, etc.)".to_string());
    }

    let all_ifaces = list_interfaces();

    for iface in &all_ifaces {
        if iface.name.eq_ignore_ascii_case(&target_lower) && iface.ip.is_some() && iface.is_up {
            return Ok(iface.clone());
        }
    }

    if let Ok(parsed_ip) = target.parse::<IpAddr>() {
        for iface in &all_ifaces {
            if iface.ip == Some(parsed_ip) && iface.is_up {
                return Ok(iface.clone());
            }
        }
    }

    for iface in &all_ifaces {
        if iface.name.to_ascii_lowercase().contains(&target_lower) && iface.ip.is_some() && iface.is_up {
            return Ok(iface.clone());
        }
    }

    Err(format!(
        "could not find active network interface matching '{}'. Run 'torr vpn' to see available interfaces.",
        target
    ))
}

pub fn connect_bound(
    remote: SocketAddr,
    bind_ip: Option<IpAddr>,
    timeout: Duration,
) -> Result<TcpStream, String> {
    if bind_ip.is_none() {
        let stream = TcpStream::connect_timeout(&remote, timeout).map_err(|e| e.to_string())?;
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        return Ok(stream);
    }
    let local_ip = bind_ip.unwrap();

    let domain = match remote {
        SocketAddr::V4(_) => libc::AF_INET,
        SocketAddr::V6(_) => libc::AF_INET6,
    };

    let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(format!("socket creation failed: {}", std::io::Error::last_os_error()));
    }

    let opt: libc::c_int = 1;
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &opt as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }

    match (local_ip, remote) {
        (IpAddr::V4(v4), SocketAddr::V4(_)) => {
            let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sa.sin_family = libc::AF_INET as libc::sa_family_t;
            sa.sin_addr.s_addr = u32::from_ne_bytes(v4.octets());
            sa.sin_port = 0;
            let res = unsafe {
                libc::bind(
                    fd,
                    &sa as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            };
            if res < 0 {
                unsafe { libc::close(fd) };
                return Err(format!("failed to bind to local IP {v4}: {}", std::io::Error::last_os_error()));
            }
        }
        (IpAddr::V6(v6), SocketAddr::V6(_)) => {
            let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            sa.sin6_addr.s6_addr = v6.octets();
            sa.sin6_port = 0;
            let res = unsafe {
                libc::bind(
                    fd,
                    &sa as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                )
            };
            if res < 0 {
                unsafe { libc::close(fd) };
                return Err(format!("failed to bind to local IP {v6}: {}", std::io::Error::last_os_error()));
            }
        }
        _ => {
            unsafe { libc::close(fd) };
            return Err("local IP family and remote IP family do not match".into());
        }
    }

    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
    if flags < 0 {
        unsafe { libc::close(fd) };
        return Err(format!("fcntl F_GETFL failed: {}", std::io::Error::last_os_error()));
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        unsafe { libc::close(fd) };
        return Err(format!("fcntl F_SETFL failed: {}", std::io::Error::last_os_error()));
    }

    let (sockaddr_box, sockaddr_len) = match remote {
        SocketAddr::V4(v4) => {
            let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sa.sin_family = libc::AF_INET as libc::sa_family_t;
            sa.sin_addr.s_addr = u32::from_ne_bytes(v4.ip().octets());
            sa.sin_port = v4.port().to_be();
            (
                Box::new(sa) as Box<dyn std::any::Any>,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        }
        SocketAddr::V6(v6) => {
            let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            sa.sin6_addr.s6_addr = v6.ip().octets();
            sa.sin6_port = v6.port().to_be();
            (
                Box::new(sa) as Box<dyn std::any::Any>,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    };

    let sockaddr_ptr: *const libc::sockaddr = match remote {
        SocketAddr::V4(_) => {
            let sa_ref = sockaddr_box.downcast_ref::<libc::sockaddr_in>().unwrap();
            sa_ref as *const _ as *const libc::sockaddr
        }
        SocketAddr::V6(_) => {
            let sa_ref = sockaddr_box.downcast_ref::<libc::sockaddr_in6>().unwrap();
            sa_ref as *const _ as *const libc::sockaddr
        }
    };

    let ret = unsafe { libc::connect(fd, sockaddr_ptr, sockaddr_len) };

    if ret < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EINPROGRESS) {
            unsafe { libc::close(fd) };
            return Err(format!("connect error: {err}"));
        }

        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        let poll_ret = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as libc::c_int) };
        if poll_ret <= 0 {
            unsafe { libc::close(fd) };
            return Err("connection timed out".into());
        }

        let mut sock_err: libc::c_int = 0;
        let mut err_len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                &mut sock_err as *mut _ as *mut libc::c_void,
                &mut err_len,
            );
        }
        if sock_err != 0 {
            unsafe { libc::close(fd) };
            return Err(std::io::Error::from_raw_os_error(sock_err).to_string());
        }
    }

    unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };

    let stream = unsafe { TcpStream::from_raw_fd(fd) };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    Ok(stream)
}

#[derive(Debug, Clone)]
pub struct VpnMonitor {
    pub interface_name: String,
    pub expected_ip: Option<IpAddr>,
}

impl VpnMonitor {
    pub fn new(iface: &InterfaceInfo) -> Self {
        Self {
            interface_name: iface.name.clone(),
            expected_ip: iface.ip,
        }
    }

    pub fn is_healthy(&self) -> bool {
        let ifaces = list_interfaces();
        for iface in ifaces {
            if iface.name == self.interface_name {
                if !iface.is_up {
                    return false;
                }
                if let (Some(expected), Some(actual)) = (self.expected_ip, iface.ip) {
                    return expected == actual;
                }
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_vpn_detects_common_providers() {
        assert_eq!(classify_vpn("proton0"), (true, Some("Proton VPN".to_string())));
        assert_eq!(classify_vpn("pvpnksx123"), (true, Some("Proton VPN".to_string())));
        assert_eq!(classify_vpn("wg0"), (true, Some("WireGuard".to_string())));
        assert_eq!(classify_vpn("tun0"), (true, Some("OpenVPN / TUN".to_string())));
        assert_eq!(classify_vpn("nordlynx"), (true, Some("NordVPN".to_string())));
        assert_eq!(classify_vpn("tailscale0"), (true, Some("Tailscale".to_string())));
        assert_eq!(classify_vpn("eth0"), (false, None));
        assert_eq!(classify_vpn("wlan0"), (false, None));
    }

    #[test]
    fn list_interfaces_returns_loopback() {
        let ifaces = list_interfaces();
        assert!(!ifaces.is_empty());
        let lo = ifaces.iter().find(|i| i.is_loopback);
        assert!(lo.is_some());
    }

    #[test]
    fn connect_bound_to_loopback_works() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let client = connect_bound(
            addr,
            Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            Duration::from_secs(2),
        );

        assert!(client.is_ok());
    }
}
