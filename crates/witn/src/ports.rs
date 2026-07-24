//! Maps each PID to the TCP ports it is LISTENING on, via the IP Helper API
//! (`GetExtendedTcpTable`, the OWNER_PID_LISTENER table). Both IPv4 and IPv6 are
//! queried, since a dev server on `localhost` often binds `::1` and/or
//! `127.0.0.1`.

use std::collections::HashMap;
use std::ptr::null_mut;

use winapi::shared::minwindef::DWORD;
use winapi::shared::tcpmib::{MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID};
use winapi::shared::winerror::NO_ERROR;
use winapi::shared::ws2def::{AF_INET, AF_INET6};
use winapi::um::iphlpapi::GetExtendedTcpTable;

/// `TCP_TABLE_OWNER_PID_LISTENER` from the `TCP_TABLE_CLASS` enum — listening
/// sockets keyed by owning PID. Defined locally because winapi 0.3 doesn't
/// re-export this particular constant.
const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;

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
    let Some(buf) = query_table(AF_INET as u32) else {
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
    let Some(buf) = query_table(AF_INET6 as u32) else {
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
/// Returns the raw bytes of the OWNER_PID_LISTENER table, or `None` on failure.
fn query_table(family: u32) -> Option<Vec<u8>> {
    let mut size: DWORD = 0;
    unsafe {
        // First call sizes the buffer (returns ERROR_INSUFFICIENT_BUFFER).
        GetExtendedTcpTable(
            null_mut(),
            &mut size,
            0,
            family,
            TCP_TABLE_OWNER_PID_LISTENER,
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
            TCP_TABLE_OWNER_PID_LISTENER,
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
