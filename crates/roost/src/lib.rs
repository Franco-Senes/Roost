pub mod apps;
pub mod auth;
pub mod db;
pub mod docker;
pub mod web;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Puerto por defecto en el que escucha Roost en la red local.
pub const DEFAULT_PORT: u16 = 8420;

/// Devuelve la dirección de escucha de Roost.
///
/// Usa `ROOST_ADDR` si está definida y es válida (por ejemplo `127.0.0.1:8420`).
/// En caso contrario escucha en `0.0.0.0:8420`.
pub fn bind_addr() -> SocketAddr {
    bind_addr_from(std::env::var("ROOST_ADDR").ok().as_deref())
}

fn bind_addr_from(raw: Option<&str>) -> SocketAddr {
    if let Some(addr) = raw.and_then(|value| value.parse::<SocketAddr>().ok()) {
        return addr;
    }
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bind_addr_uses_port_8420() {
        let addr = bind_addr_from(None);
        assert_eq!(addr, SocketAddr::from(([0, 0, 0, 0], 8420)));
    }

    #[test]
    fn custom_bind_addr_is_parsed() {
        let addr = bind_addr_from(Some("127.0.0.1:9000"));
        assert_eq!(addr, SocketAddr::from(([127, 0, 0, 1], 9000)));
    }

    #[test]
    fn invalid_bind_addr_falls_back_to_default() {
        let addr = bind_addr_from(Some("not-an-address"));
        assert_eq!(addr.port(), DEFAULT_PORT);
    }
}
