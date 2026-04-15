use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::{
    GetIpInterfaceEntry, InitializeIpInterfaceEntry, SetIpInterfaceEntry, MIB_IPINTERFACE_ROW,
};
use windows::Win32::Networking::WinSock::AF_INET;

use super::adapter::{self, AdapterInfo};
use super::NetworkError;

pub(crate) struct TemporaryIpv4Mtu {
    adapter: AdapterInfo,
    previous_mtu: u32,
}

impl TemporaryIpv4Mtu {
    pub(crate) fn restore(self) -> Result<(), NetworkError> {
        set_ipv4_mtu(self.adapter, self.previous_mtu)
    }
}

pub(crate) async fn lower_tunnel_ipv4_mtu(
    tun_name: &str,
    mtu: u32,
) -> Result<TemporaryIpv4Mtu, NetworkError> {
    let adapter = adapter::find_adapter_with_retry(tun_name).await?;
    let previous_mtu = get_ipv4_mtu(adapter)?;
    if previous_mtu != mtu {
        set_ipv4_mtu(adapter, mtu)?;
    }
    Ok(TemporaryIpv4Mtu {
        adapter,
        previous_mtu,
    })
}

fn get_ipv4_mtu(adapter: AdapterInfo) -> Result<u32, NetworkError> {
    Ok(ipv4_interface_row(adapter)?.NlMtu)
}

fn set_ipv4_mtu(adapter: AdapterInfo, mtu: u32) -> Result<(), NetworkError> {
    let mut row = ipv4_interface_row(adapter)?;
    row.NlMtu = mtu;
    row.SitePrefixLength = 0;

    let result = unsafe { SetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "SetIpInterfaceEntry",
            code: result,
        });
    }

    Ok(())
}

fn ipv4_interface_row(adapter: AdapterInfo) -> Result<MIB_IPINTERFACE_ROW, NetworkError> {
    let mut row: MIB_IPINTERFACE_ROW = unsafe { std::mem::zeroed() };
    unsafe {
        InitializeIpInterfaceEntry(&mut row);
    }
    row.Family = AF_INET;
    row.InterfaceLuid = adapter.luid;
    row.InterfaceIndex = adapter.if_index;

    let result = unsafe { GetIpInterfaceEntry(&mut row) };
    if result != NO_ERROR {
        return Err(NetworkError::Win32 {
            context: "GetIpInterfaceEntry",
            code: result,
        });
    }

    Ok(row)
}
