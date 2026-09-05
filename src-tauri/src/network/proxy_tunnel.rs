use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use url::Url;

use crate::network::system_proxy;
use crate::settings::{ProxyMode, ProxyProtocol, ProxySettings};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 统一的底层传输流抽象（支持明文 TCP 与 TLS 加密隧道）
pub enum TunnelStream {
    Plain(TcpStream),
    Tls(tokio_native_tls::TlsStream<TcpStream>),
}

impl AsyncRead for TunnelStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TunnelStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            TunnelStream::Tls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TunnelStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            TunnelStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            TunnelStream::Tls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TunnelStream::Plain(s) => Pin::new(s).poll_flush(cx),
            TunnelStream::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TunnelStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            TunnelStream::Tls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// 解析生效的代理配置
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

/// 发起 HTTP CONNECT 隧道请求
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
        .map_err(|_| "发送 HTTP CONNECT 隧道指令超时".to_string())?
        .map_err(|e| format!("发送 HTTP CONNECT 隧道指令失败: {}", e))?;

    // 读取响应头直到 \r\n\r\n
    let mut header_buf = Vec::with_capacity(1024);
    let mut byte_buf = [0u8; 1];
    loop {
        let n = tokio::time::timeout(CONNECT_TIMEOUT, stream.read(&mut byte_buf))
            .await
            .map_err(|_| "读取 HTTP CONNECT 响应超时".to_string())?
            .map_err(|e| format!("读取 HTTP CONNECT 响应失败: {}", e))?;
        if n == 0 {
            return Err("HTTP 代理服务器在完成 CONNECT 握手前关闭了连接".to_string());
        }
        header_buf.push(byte_buf[0]);
        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if header_buf.len() > 8192 {
            return Err("HTTP 代理响应头超出 8KB 上限".to_string());
        }
    }

    let header_str = String::from_utf8_lossy(&header_buf);
    let first_line = header_str.lines().next().unwrap_or_default().trim();

    // 格式如 "HTTP/1.1 200 Connection established"
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(format!("HTTP 代理返回了非法的响应行: {}", first_line));
    }
    let status_code: u16 = parts[1]
        .parse()
        .map_err(|_| format!("解析 HTTP 状态码失败: {}", parts[1]))?;

    if !(200..=299).contains(&status_code) {
        return Err(format!(
            "HTTP CONNECT 代理握手失败，状态码: {}，详情: {}",
            status_code, first_line
        ));
    }

    Ok(())
}

/// 执行 SOCKS5 握手并建立隧道 (RFC 1928 / 1929)
pub async fn establish_socks5_tunnel(
    stream: &mut TcpStream,
    target_host: &str,
    target_port: u16,
    auth: Option<&(String, String)>,
) -> Result<(), String> {
    // 1. 发送 Greeting 报文
    let (greeting, has_auth) = if let Some((user, _pass)) = auth {
        if !user.is_empty() {
            // Version 5, 2 Methods: 0x00 (No Auth), 0x02 (User/Pass)
            (vec![0x05, 0x02, 0x00, 0x02], true)
        } else {
            (vec![0x05, 0x01, 0x00], false)
        }
    } else {
        (vec![0x05, 0x01, 0x00], false)
    };

    tokio::time::timeout(CONNECT_TIMEOUT, stream.write_all(&greeting))
        .await
        .map_err(|_| "发送 SOCKS5 握手报文超时".to_string())?
        .map_err(|e| format!("发送 SOCKS5 握手失败: {}", e))?;

    let mut method_resp = [0u8; 2];
    tokio::time::timeout(CONNECT_TIMEOUT, stream.read_exact(&mut method_resp))
        .await
        .map_err(|_| "读取 SOCKS5 握手响应超时".to_string())?
        .map_err(|e| format!("读取 SOCKS5 握手响应失败: {}", e))?;

    if method_resp[0] != 0x05 {
        return Err(format!("不兼容的 SOCKS 协议版本: 0x{:02X}", method_resp[0]));
    }

    match method_resp[1] {
        0x00 => {
            // 无需密码认证
        }
        0x02 if has_auth => {
            if let Some((user, pass)) = auth {
                let mut auth_req = Vec::with_capacity(3 + user.len() + pass.len());
                auth_req.push(0x01); // 认证协议版本 1
                auth_req.push(user.len() as u8);
                auth_req.extend_from_slice(user.as_bytes());
                auth_req.push(pass.len() as u8);
                auth_req.extend_from_slice(pass.as_bytes());

                tokio::time::timeout(CONNECT_TIMEOUT, stream.write_all(&auth_req))
                    .await
                    .map_err(|_| "发送 SOCKS5 认证凭据超时".to_string())?
                    .map_err(|e| format!("发送 SOCKS5 认证凭据失败: {}", e))?;

                let mut auth_resp = [0u8; 2];
                tokio::time::timeout(CONNECT_TIMEOUT, stream.read_exact(&mut auth_resp))
                    .await
                    .map_err(|_| "读取 SOCKS5 认证响应超时".to_string())?
                    .map_err(|e| format!("读取 SOCKS5 认证响应失败: {}", e))?;

                if auth_resp[1] != 0x00 {
                    return Err("SOCKS5 认证失败: 账号或密码错误".to_string());
                }
            } else {
                return Err("SOCKS5 代理要求认证，但当前未配置用户名与密码".to_string());
            }
        }
        0xFF => return Err("SOCKS5 代理拒绝了所有支持的认证方式".to_string()),
        other => {
            return Err(format!(
                "SOCKS5 代理选定了未支持的认证方式: 0x{:02X}",
                other
            ))
        }
    }

    // 2. 发送 CONNECT 请求
    let mut connect_req = Vec::with_capacity(7 + target_host.len());
    connect_req.push(0x05); // VER
    connect_req.push(0x01); // CMD: 0x01 = CONNECT
    connect_req.push(0x00); // RSV

    if let Ok(ipv4) = target_host.parse::<std::net::Ipv4Addr>() {
        connect_req.push(0x01); // ATYP: IPv4
        connect_req.extend_from_slice(&ipv4.octets());
    } else if let Ok(ipv6) = target_host.parse::<std::net::Ipv6Addr>() {
        connect_req.push(0x04); // ATYP: IPv6
        connect_req.extend_from_slice(&ipv6.octets());
    } else {
        // ATYP: Domain name
        connect_req.push(0x03);
        connect_req.push(target_host.len() as u8);
        connect_req.extend_from_slice(target_host.as_bytes());
    }
    connect_req.extend_from_slice(&target_port.to_be_bytes());

    tokio::time::timeout(CONNECT_TIMEOUT, stream.write_all(&connect_req))
        .await
        .map_err(|_| "发送 SOCKS5 连接请求超时".to_string())?
        .map_err(|e| format!("发送 SOCKS5 连接请求失败: {}", e))?;

    // 3. 读取 CONNECT 响应
    let mut resp_header = [0u8; 4];
    tokio::time::timeout(CONNECT_TIMEOUT, stream.read_exact(&mut resp_header))
        .await
        .map_err(|_| "读取 SOCKS5 连接响应超时".to_string())?
        .map_err(|e| format!("读取 SOCKS5 连接响应失败: {}", e))?;

    if resp_header[0] != 0x05 {
        return Err(format!(
            "SOCKS5 连接响应协议版本非法: 0x{:02X}",
            resp_header[0]
        ));
    }

    let rep = resp_header[1];
    if rep != 0x00 {
        let msg = match rep {
            0x01 => "常规 SOCKS 服务器故障",
            0x02 => "规则集不允许该连接",
            0x03 => "网络不可达",
            0x04 => "主机不可达",
            0x05 => "目标连接被拒绝",
            0x06 => "TTL 已过期",
            0x07 => "不支持的命令",
            0x08 => "不支持的地址类型",
            _ => "未知 SOCKS 错误",
        };
        return Err(format!("SOCKS5 连接目标失败: {} (代码 0x{:02X})", msg, rep));
    }

    // 消耗响应中的绑定地址（BND.ADDR 与 BND.PORT）
    match resp_header[3] {
        0x01 => {
            // IPv4: 4 bytes IP + 2 bytes port
            let mut addr = [0u8; 6];
            stream
                .read_exact(&mut addr)
                .await
                .map_err(|e| e.to_string())?;
        }
        0x03 => {
            // Domain: 1 byte len + len bytes domain + 2 bytes port
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
            // IPv6: 16 bytes IP + 2 bytes port
            let mut addr = [0u8; 18];
            stream
                .read_exact(&mut addr)
                .await
                .map_err(|e| e.to_string())?;
        }
        other => return Err(format!("SOCKS5 响应中未知的地址类型: 0x{:02X}", other)),
    }

    Ok(())
}

/// 通过网络代理隧道（或直连）建立 WebSocket 客户端全双工长连接
pub async fn connect_websocket_tunnel(
    url_str: &str,
    proxy_settings: &ProxySettings,
) -> Result<WebSocketStream<TunnelStream>, String> {
    let parsed_url = Url::parse(url_str).map_err(|e| format!("解析 WebSocket URL 失败: {}", e))?;

    let host = parsed_url
        .host_str()
        .ok_or_else(|| "WebSocket URL 缺少有效的主机名".to_string())?;

    let is_secure = match parsed_url.scheme() {
        "wss" => true,
        "ws" => false,
        other => return Err(format!("不支持的 WebSocket 协议方案: {}", other)),
    };

    let port = parsed_url
        .port_or_known_default()
        .unwrap_or(if is_secure { 443 } else { 80 });

    let resolved = resolve_effective_proxy(proxy_settings);

    // 1. 建立 TCP 流（直连或经由代理）
    let tcp_stream = match resolved {
        ResolvedProxy::Direct => {
            tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
                .await
                .map_err(|_| format!("直连目标 {}:{} 超时", host, port))?
                .map_err(|e| format!("直连目标 {}:{} 失败: {}", host, port, e))?
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
            .map_err(|_| format!("连接 HTTP 代理 {}:{} 超时", p_host, p_port))?
            .map_err(|e| format!("连接 HTTP 代理 {}:{} 失败: {}", p_host, p_port, e))?;

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
            .map_err(|_| format!("连接 SOCKS5 代理 {}:{} 超时", p_host, p_port))?
            .map_err(|e| format!("连接 SOCKS5 代理 {}:{} 失败: {}", p_host, p_port, e))?;

            establish_socks5_tunnel(&mut stream, host, port, auth.as_ref()).await?;
            stream
        }
    };

    // 2. 根据协议判断是否需要进行 TLS 封装
    let tunnel_stream = if is_secure {
        let native_connector = native_tls::TlsConnector::builder()
            .build()
            .map_err(|e| format!("初始化原生 TLS 连接器失败: {}", e))?;
        let async_connector = tokio_native_tls::TlsConnector::from(native_connector);

        let tls_stream =
            tokio::time::timeout(CONNECT_TIMEOUT, async_connector.connect(host, tcp_stream))
                .await
                .map_err(|_| format!("与目标主机 {} 进行 TLS 握手超时", host))?
                .map_err(|e| format!("与目标主机 {} 进行 TLS 握手失败: {}", host, e))?;

        TunnelStream::Tls(tls_stream)
    } else {
        TunnelStream::Plain(tcp_stream)
    };

    // 3. 执行 WebSocket 握手
    let (ws_stream, _response) = tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::client_async(url_str, tunnel_stream),
    )
    .await
    .map_err(|_| "WebSocket 客户端协议握手超时".to_string())?
    .map_err(|e| format!("WebSocket 客户端协议握手失败: {}", e))?;

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
        // 启动本地模拟 HTTP CONNECT 代理
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut req_buf = [0u8; 1024];
            let n = socket.read(&mut req_buf).await.unwrap();
            let req_str = String::from_utf8_lossy(&req_buf[..n]);
            assert!(req_str.starts_with("CONNECT example.com:443 HTTP/1.1"));
            assert!(req_str.contains("Proxy-Authorization: Basic dXNlcjpwYXNz"));

            // 返回 200 OK 响应
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

            // 1. 读取 Greeting
            let mut greeting = [0u8; 4];
            socket.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting[0], 0x05); // VER 5

            // 返回 Method 选择: 0x02 (User/Pass)
            socket.write_all(&[0x05, 0x02]).await.unwrap();

            // 2. 读取 User/Pass 认证请求
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

            // 返回认证成功: [0x01, 0x00]
            socket.write_all(&[0x01, 0x00]).await.unwrap();

            // 3. 读取 CONNECT 请求
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

            // 返回成功响应: [0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x01, 0xBB]
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
