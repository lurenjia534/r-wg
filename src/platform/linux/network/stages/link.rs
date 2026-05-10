use rtnetlink::{Handle, LinkMessageBuilder, LinkUnspec};

use crate::core::config::InterfaceConfig;
use crate::log::events::net as log_net;

use super::super::NetworkError;

pub(in crate::platform::linux::network) async fn configure_link(
    handle: &Handle,
    link_index: u32,
    interface: &InterfaceConfig,
) -> Result<(), NetworkError> {
    // 设置 MTU 与 up 状态，确保隧道可用。
    if let Some(mtu) = interface.mtu {
        let message = LinkMessageBuilder::<LinkUnspec>::default()
            .index(link_index)
            .mtu(mtu.into())
            .build();
        handle.link().set(message).execute().await?;
    }

    let message = LinkMessageBuilder::<LinkUnspec>::default()
        .index(link_index)
        .up()
        .build();
    handle.link().set(message).execute().await?;

    // 写入接口地址（IPv4/IPv6）。
    for address in &interface.addresses {
        log_net::address_add(address.addr, address.cidr);
        handle
            .address()
            .add(link_index, address.addr, address.cidr)
            .execute()
            .await?;
    }

    Ok(())
}
