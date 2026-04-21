//! rustls 进程级初始化。
//!
//! 当前依赖图里既有 `gpui` 侧的 rustls/ring，也有我们自己的 HTTPS 客户端。
//! rustls 0.23 在同一进程内启用了多个 provider 特性时，不会自动决定默认实现，
//! 必须由应用在启动早期显式安装一个 `CryptoProvider`。

/// 安装进程级默认 rustls provider。
///
/// 这是幂等调用：如果默认 provider 已经存在，就直接复用；
/// 如果并发安装发生竞争，保留先安装成功的那一个。
pub fn install_default_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return;
    }

    let _ = rustls::crypto::ring::default_provider().install_default();
}
