pub mod ipc;
pub mod models;
pub mod parser;
pub mod ping;
pub mod singbox;
pub mod storage;
pub mod subscription;
pub mod xray;

pub const DEFAULT_SOCKET_PATH: &str = "/run/omavpn/omavpnd.sock";
pub const DEFAULT_SINGBOX_BIN: &str = "sing-box";
pub const DEFAULT_XRAY_BIN: &str = "xray";
pub const DEFAULT_TUN_INTERFACE: &str = "omavpn0";
pub const DEFAULT_XRAY_BRIDGE_PORT: u16 = 10809;
