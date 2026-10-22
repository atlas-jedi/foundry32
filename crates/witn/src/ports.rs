//! Sockets by owning PID, via the IP Helper API: the TCP ports each PID is
//! LISTENING on (`GetExtendedTcpTable`, OWNER_PID_LISTENER) for the list, and
//! every TCP/UDP endpoint of one PID (`endpoints_of`) for the Details view.
//! Both IPv4 and IPv6 are queried, since a dev server on `localhost` often
//! binds `::1` and/or `127.0.0.1`.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::ptr::null_mut;

use winapi::shared::minwindef::DWORD;
use winapi::shared::tcpmib::{MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID};
use winapi::shared::udpmib::{MIB_UDP6TABLE_OWNER_PID, MIB_UDPTABLE_OWNER_PID};
use winapi::shared::winerror::NO_ERROR;
use winapi::shared::ws2def::{AF_INET, AF_INET6};
use winapi::um::iphlpapi::{GetExtendedTcpTable, GetExtendedUdpTable};

/// `TCP_TABLE_OWNER_PID_LISTENER` from the `TCP_TABLE_CLASS` enum — listening
/// sockets keyed by owning PID. Defined locally because winapi 0.3 doesn't
/// re-export this particular constant.
const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
/// `TCP_TABLE_OWNER_PID_ALL` — every TCP socket in any state, keyed by PID.
const TCP_TABLE_OWNER_PID_ALL: u32 = 5;
/// `UDP_TABLE_OWNER_PID` from `UDP_TABLE_CLASS`.
const UDP_TABLE_OWNER_PID: u32 = 1;

/// PID → sorted, de-duplicated list of listening TCP ports.
pub fn listening_by_pid() -> HashMap<u32, Vec<u16>> {
    let mut map: HashMap<u32, Vec<u16>> = HashMap::new();
    collect_v4(&mut map);
    collect_v6(&mut map);
    for ports in map.values_mut() {
        ports.sort_unstable();
        ports.dedup();
    }
    map
}

fn collect_v4(map: &mut HashMap<u32, Vec<u16>>) {
    let Some(buf) = query_table(AF_INET as u32, TCP_TABLE_OWNER_PID_LISTENER) else {
        return;
    };
    unsafe {
        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let rows = table.table.as_ptr();
        for i in 0..table.dwNumEntries as usize {
            let row = &*rows.add(i);
            map.entry(row.dwOwningPid)
                .or_default()
                .push(be_port(row.dwLocalPort));
        }
    }
}

fn collect_v6(map: &mut HashMap<u32, Vec<u16>>) {
    let Some(buf) = query_table(AF_INET6 as u32, TCP_TABLE_OWNER_PID_LISTENER) else {
        return;
    };
    unsafe {
        let table = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
        let rows = table.table.as_ptr();
        for i in 0..table.dwNumEntries as usize {
            let row = &*rows.add(i);
            map.entry(row.dwOwningPid)
                .or_default()
                .push(be_port(row.dwLocalPort));
        }
    }
}

/// Calls `GetExtendedTcpTable` twice: once to size the buffer, once to fill it.
/// Returns the raw bytes of the requested OWNER_PID table, or `None` on failure.
fn query_table(family: u32, class: u32) -> Option<Vec<u8>> {
    let mut size: DWORD = 0;
    unsafe {
        // First call sizes the buffer (returns ERROR_INSUFFICIENT_BUFFER).
        GetExtendedTcpTable(
            null_mut(),
            &mut size,
            0,
            family,
            class,
            0,
        );
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let ret = GetExtendedTcpTable(
            buf.as_mut_ptr().cast(),
            &mut size,
            0,
            family,
            class,
            0,
        );
        if ret != NO_ERROR {
            return None;
        }
        Some(buf)
    }
}

/// The table stores the local port in network byte order in the low 16 bits of
/// a DWORD; recover the host-order `u16`.
fn be_port(dw_local_port: DWORD) -> u16 {
    let bytes = dw_local_port.to_le_bytes();
    u16::from_be_bytes([bytes[0], bytes[1]])
}

/// One socket a process owns, in `netstat -ano` terms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// `TCP`, `TCPv6`, `UDP` or `UDPv6`.
    pub protocol: &'static str,
    pub local: String,
    /// `None` for UDP, which has no peer.
    pub remote: Option<String>,
    /// netstat's state name; `None` for UDP.
    pub state: Option<&'static str>,
}

/// `127.0.0.1:3939` from the table's IPv4 DWORD (network byte order, as laid
/// out in memory) and a host-order port.
pub fn format_v4(addr: DWORD, port: u16) -> String {
    format!("{}:{port}", Ipv4Addr::from(addr.to_le_bytes()))
}

/// `[::1]:3939` — brackets, as in a URL, so the port is not read as part of
/// the address.
pub fn format_v6(addr: [u8; 16], port: u16) -> String {
    format!("[{}]:{port}", Ipv6Addr::from(addr))
}

/// netstat's name for a `MIB_TCP_STATE` value.
pub fn tcp_state_name(state: DWORD) -> &'static str {
    match state {
        1 => "CLOSED",
        2 => "LISTENING",
        3 => "SYN_SENT",
        4 => "SYN_RECEIVED",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT_1",
        7 => "FIN_WAIT_2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        12 => "DELETE_TCB",
        _ => "UNKNOWN",
    }
}

/// Every TCP and UDP socket `pid` owns, IPv4 then IPv6, TCP then UDP.
pub fn endpoints_of(pid: u32) -> Vec<Endpoint> {
    let mut out = Vec::new();
    if let Some(buf) = query_table(AF_INET as u32, TCP_TABLE_OWNER_PID_ALL) {
        unsafe {
            let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            for i in 0..table.dwNumEntries as usize {
                let row = &*table.table.as_ptr().add(i);
                if row.dwOwningPid == pid {
                    out.push(Endpoint {
                        protocol: "TCP",
                        local: format_v4(row.dwLocalAddr, be_port(row.dwLocalPort)),
                        remote: Some(format_v4(row.dwRemoteAddr, be_port(row.dwRemotePort))),
                        state: Some(tcp_state_name(row.dwState)),
                    });
                }
            }
        }
    }
    if let Some(buf) = query_table(AF_INET6 as u32, TCP_TABLE_OWNER_PID_ALL) {
        unsafe {
            let table = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
            for i in 0..table.dwNumEntries as usize {
                let row = &*table.table.as_ptr().add(i);
                if row.dwOwningPid == pid {
                    out.push(Endpoint {
                        protocol: "TCPv6",
                        local: format_v6(row.ucLocalAddr, be_port(row.dwLocalPort)),
                        remote: Some(format_v6(row.ucRemoteAddr, be_port(row.dwRemotePort))),
                        state: Some(tcp_state_name(row.dwState)),
                    });
                }
            }
        }
    }
    if let Some(buf) = query_udp_table(AF_INET as u32) {
        unsafe {
            let table = &*(buf.as_ptr() as *const MIB_UDPTABLE_OWNER_PID);
            for i in 0..table.dwNumEntries as usize {
                let row = &*table.table.as_ptr().add(i);
                if row.dwOwningPid == pid {
                    out.push(Endpoint {
                        protocol: "UDP",
                        local: format_v4(row.dwLocalAddr, be_port(row.dwLocalPort)),
                        remote: None,
                        state: None,
                    });
                }
            }
        }
    }
    if let Some(buf) = query_udp_table(AF_INET6 as u32) {
        unsafe {
            let table = &*(buf.as_ptr() as *const MIB_UDP6TABLE_OWNER_PID);
            for i in 0..table.dwNumEntries as usize {
                let row = &*table.table.as_ptr().add(i);
                if row.dwOwningPid == pid {
                    out.push(Endpoint {
                        protocol: "UDPv6",
                        local: format_v6(row.ucLocalAddr, be_port(row.dwLocalPort)),
                        remote: None,
                        state: None,
                    });
                }
            }
        }
    }
    out
}

/// `GetExtendedUdpTable`'s OWNER_PID table, sized then filled like
/// [`query_table`].
fn query_udp_table(family: u32) -> Option<Vec<u8>> {
    let mut size: DWORD = 0;
    unsafe {
        GetExtendedUdpTable(null_mut(), &mut size, 0, family, UDP_TABLE_OWNER_PID, 0);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let ret =
            GetExtendedUdpTable(buf.as_mut_ptr().cast(), &mut size, 0, family, UDP_TABLE_OWNER_PID, 0);
        if ret != NO_ERROR {
            return None;
        }
        Some(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v4_address_is_read_in_memory_order() {
        let loopback = u32::from_le_bytes([127, 0, 0, 1]);
        assert_eq!(format_v4(loopback, 3939), "127.0.0.1:3939");
        assert_eq!(format_v4(0, 0), "0.0.0.0:0");
    }

    #[test]
    fn v6_address_is_bracketed() {
        let mut loopback = [0u8; 16];
        loopback[15] = 1;
        assert_eq!(format_v6(loopback, 5173), "[::1]:5173");
        assert_eq!(format_v6([0; 16], 80), "[::]:80");
    }

    #[test]
    fn tcp_states_use_netstat_names() {
        assert_eq!(tcp_state_name(2), "LISTENING");
        assert_eq!(tcp_state_name(5), "ESTABLISHED");
        assert_eq!(tcp_state_name(11), "TIME_WAIT");
        assert_eq!(tcp_state_name(8), "CLOSE_WAIT");
        assert_eq!(tcp_state_name(99), "UNKNOWN");
    }
}
