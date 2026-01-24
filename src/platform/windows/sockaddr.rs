//! IPv4/IPv6 与 SOCKADDR_* 的互转工具。
//!
//! 注意：IPv4 地址在 Windows 结构体中使用网络字节序存放。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, IN6_ADDR, IN6_ADDR_0, IN_ADDR, IN_ADDR_0, SOCKADDR_IN, SOCKADDR_IN6,
    SOCKADDR_INET, SOCKET_ADDRESS,
};

pub(super) fn sockaddr_inet_from_ip(addr: IpAddr) -> SOCKADDR_INET {
    // 根据地址族构造 SOCKADDR_INET。
    match addr {
        IpAddr::V4(addr) => sockaddr_inet_v4(addr),
        IpAddr::V6(addr) => sockaddr_inet_v6(addr),
    }
}

fn sockaddr_inet_v4(addr: Ipv4Addr) -> SOCKADDR_INET {
    // IPv4 的 S_addr 需要保持网络字节序布局。
    let mut sockaddr: SOCKADDR_IN = unsafe { std::mem::zeroed() };
    sockaddr.sin_family = AF_INET;
    let in_addr = IN_ADDR {
        S_un: IN_ADDR_0 {
            S_addr: u32::from_ne_bytes(addr.octets()),
        },
    };
    sockaddr.sin_addr = in_addr;
    SOCKADDR_INET { Ipv4: sockaddr }
}

fn sockaddr_inet_v6(addr: Ipv6Addr) -> SOCKADDR_INET {
    // IPv6 直接拷贝 16 字节即可。
    let mut sockaddr: SOCKADDR_IN6 = unsafe { std::mem::zeroed() };
    sockaddr.sin6_family = AF_INET6;
    sockaddr.sin6_addr = IN6_ADDR {
        u: IN6_ADDR_0 { Byte: addr.octets() },
    };
    SOCKADDR_INET { Ipv6: sockaddr }
}

pub(super) fn ip_from_sockaddr_inet(addr: &SOCKADDR_INET) -> Option<IpAddr> {
    // 从 SOCKADDR_INET 还原 IpAddr。
    unsafe {
        if addr.si_family == AF_INET {
            let value = addr.Ipv4.sin_addr.S_un.S_addr;
            return Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(value))));
        }
        if addr.si_family == AF_INET6 {
            let bytes = addr.Ipv6.sin6_addr.u.Byte;
            return Some(IpAddr::V6(Ipv6Addr::from(bytes)));
        }
    }
    None
}

pub(super) fn ip_from_socket_address(addr: &SOCKET_ADDRESS) -> Option<IpAddr> {
    // 从 SOCKET_ADDRESS 读取 IP（用于枚举现有地址）。
    if addr.lpSockaddr.is_null() {
        return None;
    }
    unsafe {
        let sockaddr = &*addr.lpSockaddr;
        if sockaddr.sa_family == AF_INET {
            let sin = &*(addr.lpSockaddr as *const SOCKADDR_IN);
            let value = sin.sin_addr.S_un.S_addr;
            return Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(value))));
        }
        if sockaddr.sa_family == AF_INET6 {
            let sin6 = &*(addr.lpSockaddr as *const SOCKADDR_IN6);
            let bytes = sin6.sin6_addr.u.Byte;
            return Some(IpAddr::V6(Ipv6Addr::from(bytes)));
        }
    }
    None
}
