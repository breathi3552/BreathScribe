use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::network::system_proxy;
use crate::settings::{ProxyMode, ProxyProtocol, ProxySettings};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Effective proxy configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedProxy {
    Direct,
    Http {
        host: String,
        port: u16,
        auth: Option<(String, String)>,
    },
    Socks5 {
        host: String,
        port: u16,
        auth: Option<(String, String)>,
    },
}

pub fn resolve_effective_proxy(settings: &ProxySettings) -> ResolvedProxy {
    match settings.mode {
        ProxyMode::Direct => ResolvedProxy::Direct,
        ProxyMode::System => {
            if let Some(detected) = system_proxy::get_system_proxy() {
                match detected.protocol {
                    ProxyProtocol::Http => ResolvedProxy::Http {
                        host: detected.host,
                        port: detected.port,
                        auth: None,
                    },
                    ProxyProtocol::Socks5 => ResolvedProxy::Socks5 {
                        host: detected.host,
                        port: detected.port,
                        auth: None,
                    },
                }
            } else {
                ResolvedProxy::Direct
            }
        }
        ProxyMode::Manual => {
            let auth = if settings.auth_enabled {
                let user = settings.username.clone().unwrap_or_default();
                let pass = settings.password.clone().unwrap_or_default();
                if !user.is_empty() {
                    Some((user, pass))
                } else {
                    None
                }
            } else {
                None
            };
            match settings.protocol {
                ProxyProtocol::Http => ResolvedProxy::Http {
                    host: settings.host.clone(),
                    port: settings.port,
                    auth,
                },
                ProxyProtocol::Socks5 => ResolvedProxy::Socks5 {
                    host: settings.host.clone(),
                    port: settings.port,
                    auth,
                },
            }
        }
    }
}

pub async fn establish_http_connect_tunnel(
    stream: &mut TcpStream,
    target_host: &str,
    target_port: u16,
    auth: Option<&(String, String)>,
) -> Result<(), String> {
    let mut req = format!(
        "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\nProxy-Connection: Keep-Alive\r\n",
        target_host, target_port, target_host, target_port
    );
    if let Some((user, pass)) = auth {
        let creds = BASE64.encode(format!("{}:{}", user, pass));
        req.push_str(&format!("Proxy-Authorization: Basic {}\r\n", creds));
    }
    req.push_str("\r\n");

    tokio::time::timeout(CONNECT_TIMEOUT, stream.write_all(req.as_bytes()))
        .await
        .map_err(|_| "Timed out sending HTTP CONNECT directive".to_string())?
        .map_err(|e| format!("Failed to send HTTP CONNECT directive: {}", e))?;

    let mut header_buf = Vec::with_capacity(1024);
    let mut byte_buf = [0u8; 1];
    loop {
        let n = tokio::time::timeout(CONNECT_TIMEOUT, stream.read(&mut byte_buf))
            .await
            .map_err(|_| "Timed out reading HTTP CONNECT response".to_string())?
            .map_err(|e| format!("Failed to read HTTP CONNECT response: {}", e))?;
        if n == 0 {
            return Err(
                "HTTP proxy closed connection before completing CONNECT handshake".to_string(),
            );
        }
        header_buf.push(byte_buf[0]);
        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if header_buf.len() > 8192 {
            return Err("HTTP proxy response header exceeded 8KB limit".to_string());
        }
    }

    let header_str = String::from_utf8_lossy(&header_buf);
    let first_line = header_str.lines().next().unwrap_or_default().trim();

    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(format!("Invalid HTTP proxy response line: {}", first_line));
    }
    let status_code: u16 = parts[1]
        .parse()
        .map_err(|_| format!("Failed to parse HTTP status code: {}", parts[1]))?;

    if !(200..=299).contains(&status_code) {
        return Err(format!(
            "HTTP CONNECT handshake failed with status {}: {}",
            status_code, first_line
        ));
    }

    Ok(())
}

pub async fn establish_socks5_tunnel(
    stream: &mut TcpStream,
    target_host: &str,
    target_port: u16,
    auth: Option<&(String, String)>,
) -> Result<(), String> {
    let (greeting, has_auth) = if let Some((user, _pass)) = auth {
        if !user.is_empty() {
            (vec![0x05, 0x02, 0x00, 0x02], true)
        } else {
            (vec![0x05, 0x01, 0x00], false)
        }
    } else {
        (vec![0x05, 0x01, 0x00], false)
    };

    tokio::time::timeout(CONNECT_TIMEOUT, stream.write_all(&greeting))
        .await
        .map_err(|_| "Timed out sending SOCKS5 greeting".to_string())?
        .map_err(|e| format!("Failed to send SOCKS5 greeting: {}", e))?;

    let mut method_resp = [0u8; 2];
    tokio::time::timeout(CONNECT_TIMEOUT, stream.read_exact(&mut method_resp))
        .await
        .map_err(|_| "Timed out reading SOCKS5 handshake response".to_string())?
        .map_err(|e| format!("Failed to read SOCKS5 handshake response: {}", e))?;

    if method_resp[0] != 0x05 {
        return Err(format!(
            "Incompatible SOCKS protocol version: 0x{:02X}",
            method_resp[0]
        ));
    }

    match method_resp[1] {
        0x00 => {}
        0x02 if has_auth => {
            if let Some((user, pass)) = auth {
                let mut auth_req = Vec::with_capacity(3 + user.len() + pass.len());
                auth_req.push(0x01);
                auth_req.push(user.len() as u8);
                auth_req.extend_from_slice(user.as_bytes());
                auth_req.push(pass.len() as u8);
                auth_req.extend_from_slice(pass.as_bytes());

                tokio::time::timeout(CONNECT_TIMEOUT, stream.write_all(&auth_req))
                    .await
                    .map_err(|_| "Timed out sending SOCKS5 auth credentials".to_string())?
                    .map_err(|e| format!("Failed to send SOCKS5 auth credentials: {}", e))?;

                let mut auth_resp = [0u8; 2];
                tokio::time::timeout(CONNECT_TIMEOUT, stream.read_exact(&mut auth_resp))
                    .await
                    .map_err(|_| "Timed out reading SOCKS5 auth response".to_string())?
                    .map_err(|e| format!("Failed to read SOCKS5 auth response: {}", e))?;

                if auth_resp[1] != 0x00 {
                    return Err(
                        "SOCKS5 authentication failed: invalid username or password".to_string()
                    );
                }
            } else {
                return Err(
                    "SOCKS5 proxy requires authentication, but no credentials provided".to_string(),
                );
            }
        }
        0xFF => {
            return Err("SOCKS5 proxy rejected all supported authentication methods".to_string())
        }
        other => {
            return Err(format!(
                "SOCKS5 proxy selected unsupported authentication method: 0x{:02X}",
                other
            ))
        }
    }

    let mut connect_req = Vec::with_capacity(7 + target_host.len());
    connect_req.push(0x05);
    connect_req.push(0x01);
    connect_req.push(0x00);

    if let Ok(ipv4) = target_host.parse::<std::net::Ipv4Addr>() {
        connect_req.push(0x01);
        connect_req.extend_from_slice(&ipv4.octets());
    } else if let Ok(ipv6) = target_host.parse::<std::net::Ipv6Addr>() {
        connect_req.push(0x04);
        connect_req.extend_from_slice(&ipv6.octets());
    } else {
        connect_req.push(0x03);
        connect_req.push(target_host.len() as u8);
        connect_req.extend_from_slice(target_host.as_bytes());
    }
    connect_req.extend_from_slice(&target_port.to_be_bytes());

    tokio::time::timeout(CONNECT_TIMEOUT, stream.write_all(&connect_req))
        .await
        .map_err(|_| "Timed out sending SOCKS5 connect request".to_string())?
        .map_err(|e| format!("Failed to send SOCKS5 connect request: {}", e))?;

    let mut resp_header = [0u8; 4];
    tokio::time::timeout(CONNECT_TIMEOUT, stream.read_exact(&mut resp_header))
        .await
        .map_err(|_| "Timed out reading SOCKS5 connect response".to_string())?
        .map_err(|e| format!("Failed to read SOCKS5 connect response: {}", e))?;

    if resp_header[0] != 0x05 {
        return Err(format!(
            "Invalid SOCKS5 connect response version: 0x{:02X}",
            resp_header[0]
        ));
    }

    let rep = resp_header[1];
    if rep != 0x00 {
        let msg = match rep {
            0x01 => "general SOCKS server failure",
            0x02 => "connection not allowed by ruleset",
            0x03 => "network unreachable",
            0x04 => "host unreachable",
            0x05 => "connection refused",
            0x06 => "TTL expired",
            0x07 => "command not supported",
            0x08 => "address type not supported",
            _ => "unknown SOCKS error",
        };
        return Err(format!(
            "SOCKS5 connect failed: {} (code 0x{:02X})",
            msg, rep
        ));
    }

    match resp_header[3] {
        0x01 => {
            let mut addr = [0u8; 6];
            stream
                .read_exact(&mut addr)
                .await
                .map_err(|e| e.to_string())?;
        }
        0x03 => {
            let mut len_buf = [0u8; 1];
            stream
                .read_exact(&mut len_buf)
                .await
                .map_err(|e| e.to_string())?;
            let domain_len = len_buf[0] as usize;
            let mut rem = vec![0u8; domain_len + 2];
            stream
                .read_exact(&mut rem)
                .await
                .map_err(|e| e.to_string())?;
        }
        0x04 => {
            let mut addr = [0u8; 18];
            stream
                .read_exact(&mut addr)
                .await
                .map_err(|e| e.to_string())?;
        }
        other => {
            return Err(format!(
                "Unknown address type in SOCKS5 response: 0x{:02X}",
                other
            ))
        }
    }

    Ok(())
}

pub async fn connect_websocket_tunnel(
    url_str: &str,
    proxy_settings: &ProxySettings,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, String> {
    let parsed_url =
        Url::parse(url_str).map_err(|e| format!("Failed to parse WebSocket URL: {}", e))?;

    let host = parsed_url
        .host_str()
        .ok_or_else(|| "WebSocket URL missing valid host".to_string())?;

    let is_secure = match parsed_url.scheme() {
        "wss" => true,
        "ws" => false,
        other => return Err(format!("Unsupported WebSocket scheme: {}", other)),
    };

    let port = parsed_url
        .port_or_known_default()
        .unwrap_or(if is_secure { 443 } else { 80 });

    let resolved = resolve_effective_proxy(proxy_settings);

    let tcp_stream = match resolved {
        ResolvedProxy::Direct => {
            tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
                .await
                .map_err(|_| format!("Direct connection to {}:{} timed out", host, port))?
                .map_err(|e| format!("Direct connection to {}:{} failed: {}", host, port, e))?
        }
        ResolvedProxy::Http {
            host: p_host,
            port: p_port,
            auth,
        } => {
            let mut stream = tokio::time::timeout(
                CONNECT_TIMEOUT,
                TcpStream::connect((p_host.as_str(), p_port)),
            )
            .await
            .map_err(|_| format!("Connection to HTTP proxy {}:{} timed out", p_host, p_port))?
            .map_err(|e| {
                format!(
                    "Connection to HTTP proxy {}:{} failed: {}",
                    p_host, p_port, e
                )
            })?;

            establish_http_connect_tunnel(&mut stream, host, port, auth.as_ref()).await?;
            stream
        }
        ResolvedProxy::Socks5 {
            host: p_host,
            port: p_port,
            auth,
        } => {
            let mut stream = tokio::time::timeout(
                CONNECT_TIMEOUT,
                TcpStream::connect((p_host.as_str(), p_port)),
            )
            .await
            .map_err(|_| format!("Connection to SOCKS5 proxy {}:{} timed out", p_host, p_port))?
            .map_err(|e| {
                format!(
                    "Connection to SOCKS5 proxy {}:{} failed: {}",
                    p_host, p_port, e
                )
            })?;

            establish_socks5_tunnel(&mut stream, host, port, auth.as_ref()).await?;
            stream
        }
    };

    let tunnel_stream = if is_secure {
        let native_connector = native_tls::TlsConnector::builder()
            .build()
            .map_err(|e| format!("Failed to initialize TLS connector: {}", e))?;
        let async_connector = tokio_native_tls::TlsConnector::from(native_connector);

        let tls_stream =
            tokio::time::timeout(CONNECT_TIMEOUT, async_connector.connect(host, tcp_stream))
                .await
                .map_err(|_| format!("TLS handshake with {} timed out", host))?
                .map_err(|e| format!("TLS handshake with {} failed: {}", host, e))?;

        MaybeTlsStream::NativeTls(tls_stream)
    } else {
        MaybeTlsStream::Plain(tcp_stream)
    };

    let (ws_stream, _response) = tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::client_async(url_str, tunnel_stream),
    )
    .await
    .map_err(|_| "WebSocket client handshake timed out".to_string())?
    .map_err(|e| format!("WebSocket client handshake failed: {}", e))?;

    Ok(ws_stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn test_resolve_effective_proxy_direct() {
        let settings = ProxySettings {
            mode: ProxyMode::Direct,
            protocol: ProxyProtocol::Http,
            host: "127.0.0.1".to_string(),
            port: 8080,
            auth_enabled: false,
            username: None,
            password: None,
        };
        assert_eq!(resolve_effective_proxy(&settings), ResolvedProxy::Direct);
    }

    #[test]
    fn test_resolve_effective_proxy_manual_http() {
        let settings = ProxySettings {
            mode: ProxyMode::Manual,
            protocol: ProxyProtocol::Http,
            host: "10.0.0.1".to_string(),
            port: 7890,
            auth_enabled: true,
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
        };
        assert_eq!(
            resolve_effective_proxy(&settings),
            ResolvedProxy::Http {
                host: "10.0.0.1".to_string(),
                port: 7890,
                auth: Some(("user".to_string(), "pass".to_string())),
            }
        );
    }

    #[test]
    fn test_resolve_effective_proxy_manual_socks5() {
        let settings = ProxySettings {
            mode: ProxyMode::Manual,
            protocol: ProxyProtocol::Socks5,
            host: "127.0.0.1".to_string(),
            port: 1080,
            auth_enabled: false,
            username: None,
            password: None,
        };
        assert_eq!(
            resolve_effective_proxy(&settings),
            ResolvedProxy::Socks5 {
                host: "127.0.0.1".to_string(),
                port: 1080,
                auth: None,
            }
        );
    }

    #[tokio::test]
    async fn test_http_connect_tunnel_handshake_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut req_buf = [0u8; 1024];
            let n = socket.read(&mut req_buf).await.unwrap();
            let req_str = String::from_utf8_lossy(&req_buf[..n]);
            assert!(req_str.starts_with("CONNECT example.com:443 HTTP/1.1"));
            assert!(req_str.contains("Proxy-Authorization: Basic dXNlcjpwYXNz"));

            socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
        });

        let mut client_stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        let auth = ("user".to_string(), "pass".to_string());
        let res =
            establish_http_connect_tunnel(&mut client_stream, "example.com", 443, Some(&auth))
                .await;

        assert!(res.is_ok(), "HTTP CONNECT should succeed: {:?}", res);
    }

    #[tokio::test]
    async fn test_http_connect_tunnel_handshake_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut req_buf = [0u8; 512];
            let _ = socket.read(&mut req_buf).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                .await
                .unwrap();
        });

        let mut client_stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        let res = establish_http_connect_tunnel(&mut client_stream, "example.com", 443, None).await;

        assert!(res.is_err());
        assert!(res.unwrap_err().contains("407"));
    }

    #[tokio::test]
    async fn test_socks5_tunnel_handshake_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();

            let mut greeting = [0u8; 4];
            socket.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting[0], 0x05); // VER 5

            socket.write_all(&[0x05, 0x02]).await.unwrap();

            let mut auth_head = [0u8; 2];
            socket.read_exact(&mut auth_head).await.unwrap();
            let ulen = auth_head[1] as usize;
            let mut uname = vec![0u8; ulen];
            socket.read_exact(&mut uname).await.unwrap();
            let mut plen_buf = [0u8; 1];
            socket.read_exact(&mut plen_buf).await.unwrap();
            let plen = plen_buf[0] as usize;
            let mut pass = vec![0u8; plen];
            socket.read_exact(&mut pass).await.unwrap();

            assert_eq!(String::from_utf8_lossy(&uname), "admin");
            assert_eq!(String::from_utf8_lossy(&pass), "secret");

            socket.write_all(&[0x01, 0x00]).await.unwrap();

            let mut conn_head = [0u8; 4];
            socket.read_exact(&mut conn_head).await.unwrap();
            assert_eq!(conn_head[0], 0x05); // VER
            assert_eq!(conn_head[1], 0x01); // CMD CONNECT
            assert_eq!(conn_head[3], 0x03); // ATYP Domain

            let mut dlen = [0u8; 1];
            socket.read_exact(&mut dlen).await.unwrap();
            let mut domain = vec![0u8; dlen[0] as usize];
            socket.read_exact(&mut domain).await.unwrap();
            let mut port_bytes = [0u8; 2];
            socket.read_exact(&mut port_bytes).await.unwrap();
            assert_eq!(String::from_utf8_lossy(&domain), "gemini.test");
            assert_eq!(u16::from_be_bytes(port_bytes), 443);

            socket
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x01, 0xBB])
                .await
                .unwrap();
        });

        let mut client_stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        let auth = ("admin".to_string(), "secret".to_string());
        let res =
            establish_socks5_tunnel(&mut client_stream, "gemini.test", 443, Some(&auth)).await;

        assert!(res.is_ok(), "SOCKS5 handshake should succeed: {:?}", res);
    }
}
