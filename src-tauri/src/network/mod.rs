use crate::settings::{ProxyMode, ProxyProtocol, ProxySettings};
use reqwest::{Client, Proxy};
use std::fmt;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub mod proxy_tunnel;
pub mod system_proxy;

/// The single proxy policy consumed by both reqwest and WebSocket transports.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ResolvedProxy {
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

impl fmt::Debug for ResolvedProxy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted_auth = |auth: &Option<(String, String)>| auth.as_ref().map(|_| "[REDACTED]");
        match self {
            Self::Direct => formatter.write_str("Direct"),
            Self::Http { host, port, auth } => formatter
                .debug_struct("Http")
                .field("host", host)
                .field("port", port)
                .field("auth", &redacted_auth(auth))
                .finish(),
            Self::Socks5 { host, port, auth } => formatter
                .debug_struct("Socks5")
                .field("host", host)
                .field("port", port)
                .field("auth", &redacted_auth(auth))
                .finish(),
        }
    }
}

/// Resolves saved proxy settings once for a transport operation.
///
/// HTTP client construction calls this while building or reloading the shared
/// client. WebSocket callers invoke it immediately before each connection, so
/// System mode keeps its existing detection timing.
pub(crate) fn resolve_effective_proxy(settings: &ProxySettings) -> ResolvedProxy {
    let (protocol, host, port, auth) = match settings.mode {
        ProxyMode::Direct => return ResolvedProxy::Direct,
        ProxyMode::System => match system_proxy::get_system_proxy() {
            Some(detected) => (detected.protocol, detected.host, detected.port, None),
            None => return ResolvedProxy::Direct,
        },
        ProxyMode::Manual => (
            settings.protocol,
            settings.host.clone(),
            settings.port,
            settings
                .auth_enabled
                .then(|| {
                    (
                        settings.username.clone().unwrap_or_default(),
                        settings.password.clone().unwrap_or_default(),
                    )
                })
                .filter(|(username, _)| !username.is_empty()),
        ),
    };

    match protocol {
        ProxyProtocol::Http => ResolvedProxy::Http { host, port, auth },
        ProxyProtocol::Socks5 => ResolvedProxy::Socks5 { host, port, auth },
    }
}

pub struct NetworkManager {
    client: Arc<RwLock<Client>>,
    current_settings: Arc<RwLock<ProxySettings>>,
}

impl NetworkManager {
    pub fn new(initial_settings: ProxySettings) -> Result<Self, String> {
        let client = build_reqwest_client(&initial_settings)?;
        Ok(Self {
            client: Arc::new(RwLock::new(client)),
            current_settings: Arc::new(RwLock::new(initial_settings)),
        })
    }

    pub async fn client(&self) -> Client {
        self.client.read().await.clone()
    }

    pub async fn connect_websocket(
        &self,
        url: &str,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, String> {
        let settings = self.current_settings.read().await.clone();
        let proxy = resolve_effective_proxy(&settings);
        proxy_tunnel::connect_websocket_tunnel(url, proxy).await
    }

    pub async fn update_proxy_settings(&self, new_settings: ProxySettings) -> Result<(), String> {
        let new_client = build_reqwest_client(&new_settings)?;
        let mut client_lock = self.client.write().await;
        let mut settings_lock = self.current_settings.write().await;
        *client_lock = new_client;
        *settings_lock = new_settings;
        log::info!("NetworkManager: proxy client successfully reloaded");
        Ok(())
    }
}

fn build_reqwest_proxy(
    scheme: &str,
    host: &str,
    port: u16,
    auth: Option<&(String, String)>,
) -> Result<Proxy, String> {
    let formatted_host = if host.starts_with('[') {
        host.to_string()
    } else if host.parse::<Ipv6Addr>().is_ok() {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let proxy_url = url::Url::parse(&format!("{scheme}://{formatted_host}:{port}"))
        .map_err(|e| format!("Invalid proxy endpoint: {}", e))?;
    let mut proxy = Proxy::all(proxy_url).map_err(|e| format!("Invalid proxy endpoint: {}", e))?;
    if let Some((username, password)) = auth {
        proxy = proxy.basic_auth(username, password);
    }
    Ok(proxy)
}

pub fn build_reqwest_client(settings: &ProxySettings) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));

    match resolve_effective_proxy(settings) {
        ResolvedProxy::Direct => {
            builder = builder.no_proxy();
        }
        ResolvedProxy::Http { host, port, auth } => {
            builder = builder.proxy(build_reqwest_proxy("http", &host, port, auth.as_ref())?);
        }
        ResolvedProxy::Socks5 { host, port, auth } => {
            builder = builder.proxy(build_reqwest_proxy("socks5h", &host, port, auth.as_ref())?);
        }
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build reqwest client: {}", e))
}

/// Probe network connectivity and return round-trip latency in ms
pub async fn test_connectivity(client: &Client) -> Result<u64, String> {
    let test_urls = [
        "https://www.google.com/generate_204",
        "https://generativelanguage.googleapis.com",
    ];

    let mut last_err = None;

    for url in test_urls {
        let start = std::time::Instant::now();
        match client.get(url).send().await {
            Ok(resp) => {
                let elapsed_ms = start.elapsed().as_millis() as u64;
                log::info!(
                    "Connectivity test succeeded via {} in {} ms, status: {}",
                    url,
                    elapsed_ms,
                    resp.status()
                );
                return Ok(elapsed_ms);
            }
            Err(e) => {
                log::warn!("Connectivity test probe failed for {}: {}", url, e);
                last_err = Some(e);
            }
        }
    }

    Err(last_err
        .map(|e| format!("Connectivity probe failed: {}", e))
        .unwrap_or_else(|| "Connectivity probe failed: unknown error".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    use futures_util::{SinkExt, StreamExt};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::task::JoinHandle;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    const HTTP_BODY: &str = "http-target";

    #[derive(Clone, Copy)]
    struct AuthCase {
        enabled: bool,
        username: Option<&'static str>,
        password: Option<&'static str>,
    }

    fn auth_cases() -> [AuthCase; 3] {
        [
            AuthCase {
                enabled: false,
                username: Some("stale-user"),
                password: Some("stale-password"),
            },
            AuthCase {
                enabled: true,
                username: None,
                password: None,
            },
            AuthCase {
                enabled: true,
                username: Some("user:name"),
                password: Some("p@ss/word?%"),
            },
        ]
    }

    fn manual_settings(
        protocol: ProxyProtocol,
        proxy_addr: SocketAddr,
        auth: AuthCase,
    ) -> ProxySettings {
        ProxySettings {
            mode: ProxyMode::Manual,
            protocol,
            host: proxy_addr.ip().to_string(),
            port: proxy_addr.port(),
            auth_enabled: auth.enabled,
            username: auth.username.map(str::to_string),
            password: auth.password.map(str::to_string),
        }
    }

    fn expected_http_auth(auth: AuthCase) -> Option<String> {
        (auth.enabled && !auth.username.unwrap_or_default().is_empty()).then(|| {
            let credentials = format!(
                "{}:{}",
                auth.username.unwrap_or_default(),
                auth.password.unwrap_or_default()
            );
            format!("Basic {}", BASE64.encode(credentials))
        })
    }

    fn expected_socks5_auth(auth: AuthCase) -> Option<(Vec<u8>, Vec<u8>)> {
        (auth.enabled && !auth.username.unwrap_or_default().is_empty()).then(|| {
            (
                auth.username.unwrap_or_default().as_bytes().to_vec(),
                auth.password.unwrap_or_default().as_bytes().to_vec(),
            )
        })
    }

    async fn read_headers(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
        let mut request = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let read = stream
                .read(&mut chunk)
                .await
                .map_err(|e| format!("failed to read local request: {e}"))?;
            if read == 0 {
                return Err("local peer closed before sending headers".to_string());
            }
            request.extend_from_slice(&chunk[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                return Ok(request);
            }
            if request.len() > 16 * 1024 {
                return Err("local request headers exceeded 16KB".to_string());
            }
        }
    }

    fn header_value(request: &[u8], name: &str) -> Option<String> {
        String::from_utf8_lossy(request).lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    }

    fn check_http_proxy_auth(request: &[u8], expected: &Option<String>) -> Result<(), String> {
        let actual = header_value(request, "Proxy-Authorization");
        if actual.as_deref() == expected.as_deref() {
            Ok(())
        } else {
            Err(format!(
                "local HTTP proxy received unexpected proxy credentials (expected_present={}, actual_present={})",
                expected.is_some(),
                actual.is_some()
            ))
        }
    }

    async fn spawn_http_target() -> (SocketAddr, JoinHandle<Result<Vec<u8>, String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("HTTP target accept failed: {e}"))?;
            let request = read_headers(&mut stream).await?;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                HTTP_BODY.len(),
                HTTP_BODY
            );
            stream
                .write_all(response.as_bytes())
                .await
                .map_err(|e| format!("HTTP target response failed: {e}"))?;
            stream
                .shutdown()
                .await
                .map_err(|e| format!("HTTP target shutdown failed: {e}"))?;
            Ok(request)
        });
        (address, task)
    }

    async fn spawn_websocket_target() -> (SocketAddr, JoinHandle<Result<String, String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("WebSocket target accept failed: {e}"))?;
            let mut websocket = accept_async(stream)
                .await
                .map_err(|e| format!("WebSocket target handshake failed: {e}"))?;
            let message = tokio::time::timeout(Duration::from_secs(3), websocket.next())
                .await
                .map_err(|_| "WebSocket target timed out waiting for message".to_string())?
                .ok_or_else(|| "WebSocket target received no message".to_string())?
                .map_err(|e| format!("WebSocket target receive failed: {e}"))?;
            let text = match message {
                Message::Text(text) => text.to_string(),
                _ => return Err("WebSocket target received an unexpected message".to_string()),
            };
            websocket
                .send(Message::Text("ws-target".into()))
                .await
                .map_err(|e| format!("WebSocket target response failed: {e}"))?;
            Ok(text)
        });
        (address, task)
    }

    async fn run_http_forward_proxy(
        listener: TcpListener,
        target: SocketAddr,
        expected_auth: Option<String>,
    ) -> Result<(), String> {
        let (mut client, _) = listener
            .accept()
            .await
            .map_err(|e| format!("HTTP proxy accept failed: {e}"))?;
        let request = read_headers(&mut client).await?;
        check_http_proxy_auth(&request, &expected_auth)?;
        if !String::from_utf8_lossy(&request).starts_with("GET http://127.0.0.1:") {
            return Err("HTTP client did not use absolute-form proxy request".to_string());
        }

        let mut target_stream = TcpStream::connect(target)
            .await
            .map_err(|e| format!("HTTP proxy target connection failed: {e}"))?;
        target_stream
            .write_all(&request)
            .await
            .map_err(|e| format!("HTTP proxy request forwarding failed: {e}"))?;
        tokio::io::copy(&mut target_stream, &mut client)
            .await
            .map_err(|e| format!("HTTP proxy response forwarding failed: {e}"))?;
        client
            .shutdown()
            .await
            .map_err(|e| format!("HTTP proxy shutdown failed: {e}"))?;
        Ok(())
    }

    async fn run_http_connect_proxy(
        listener: TcpListener,
        target: SocketAddr,
        expected_auth: Option<String>,
    ) -> Result<(), String> {
        let (mut client, _) = listener
            .accept()
            .await
            .map_err(|e| format!("HTTP CONNECT proxy accept failed: {e}"))?;
        let request = read_headers(&mut client).await?;
        check_http_proxy_auth(&request, &expected_auth)?;
        let expected_target = format!("CONNECT 127.0.0.1:{} HTTP/1.1", target.port());
        if !String::from_utf8_lossy(&request).starts_with(&expected_target) {
            return Err("WebSocket client sent an unexpected CONNECT target".to_string());
        }

        let mut target_stream = TcpStream::connect(target)
            .await
            .map_err(|e| format!("HTTP CONNECT target connection failed: {e}"))?;
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .map_err(|e| format!("HTTP CONNECT response failed: {e}"))?;
        tokio::io::copy_bidirectional(&mut client, &mut target_stream)
            .await
            .map_err(|e| format!("HTTP CONNECT forwarding failed: {e}"))?;
        Ok(())
    }

    async fn read_socks5_destination(stream: &mut TcpStream) -> Result<SocketAddr, String> {
        let mut header = [0u8; 4];
        stream
            .read_exact(&mut header)
            .await
            .map_err(|e| format!("SOCKS5 CONNECT header failed: {e}"))?;
        if header[0] != 0x05 || header[1] != 0x01 || header[2] != 0x00 {
            return Err("SOCKS5 CONNECT request had an invalid header".to_string());
        }

        let ip = match header[3] {
            0x01 => {
                let mut bytes = [0u8; 4];
                stream
                    .read_exact(&mut bytes)
                    .await
                    .map_err(|e| format!("SOCKS5 IPv4 destination failed: {e}"))?;
                IpAddr::V4(Ipv4Addr::from(bytes))
            }
            0x04 => {
                let mut bytes = [0u8; 16];
                stream
                    .read_exact(&mut bytes)
                    .await
                    .map_err(|e| format!("SOCKS5 IPv6 destination failed: {e}"))?;
                IpAddr::V6(Ipv6Addr::from(bytes))
            }
            0x03 => {
                let mut length = [0u8; 1];
                stream
                    .read_exact(&mut length)
                    .await
                    .map_err(|e| format!("SOCKS5 domain length failed: {e}"))?;
                let mut domain = vec![0u8; length[0] as usize];
                stream
                    .read_exact(&mut domain)
                    .await
                    .map_err(|e| format!("SOCKS5 domain failed: {e}"))?;
                return Err("SOCKS5 test expected an IP destination".to_string());
            }
            _ => return Err("SOCKS5 destination used an unknown address type".to_string()),
        };

        let mut port = [0u8; 2];
        stream
            .read_exact(&mut port)
            .await
            .map_err(|e| format!("SOCKS5 destination port failed: {e}"))?;
        Ok(SocketAddr::new(ip, u16::from_be_bytes(port)))
    }

    async fn run_socks5_proxy(
        listener: TcpListener,
        target: SocketAddr,
        expected_auth: Option<(Vec<u8>, Vec<u8>)>,
    ) -> Result<(), String> {
        let (mut client, _) = listener
            .accept()
            .await
            .map_err(|e| format!("SOCKS5 proxy accept failed: {e}"))?;
        let mut greeting_header = [0u8; 2];
        client
            .read_exact(&mut greeting_header)
            .await
            .map_err(|e| format!("SOCKS5 greeting failed: {e}"))?;
        if greeting_header[0] != 0x05 {
            return Err("SOCKS5 greeting used an invalid version".to_string());
        }
        let mut methods = vec![0u8; greeting_header[1] as usize];
        client
            .read_exact(&mut methods)
            .await
            .map_err(|e| format!("SOCKS5 methods failed: {e}"))?;

        let selected_method = if expected_auth.is_some() {
            if !methods.contains(&0x02) {
                return Err("SOCKS5 client did not offer username/password auth".to_string());
            }
            0x02
        } else {
            if methods != [0x00] {
                return Err("SOCKS5 client offered auth while auth was disabled".to_string());
            }
            0x00
        };
        client
            .write_all(&[0x05, selected_method])
            .await
            .map_err(|e| format!("SOCKS5 method response failed: {e}"))?;

        if let Some((expected_user, expected_password)) = expected_auth {
            let mut auth_header = [0u8; 2];
            client
                .read_exact(&mut auth_header)
                .await
                .map_err(|e| format!("SOCKS5 credentials header failed: {e}"))?;
            if auth_header[0] != 0x01 {
                return Err("SOCKS5 credentials used an invalid version".to_string());
            }
            let mut username = vec![0u8; auth_header[1] as usize];
            client
                .read_exact(&mut username)
                .await
                .map_err(|e| format!("SOCKS5 username failed: {e}"))?;
            let mut password_length = [0u8; 1];
            client
                .read_exact(&mut password_length)
                .await
                .map_err(|e| format!("SOCKS5 password length failed: {e}"))?;
            let mut password = vec![0u8; password_length[0] as usize];
            client
                .read_exact(&mut password)
                .await
                .map_err(|e| format!("SOCKS5 password failed: {e}"))?;
            if username != expected_user || password != expected_password {
                return Err("SOCKS5 proxy received unexpected credentials".to_string());
            }
            client
                .write_all(&[0x01, 0x00])
                .await
                .map_err(|e| format!("SOCKS5 credentials response failed: {e}"))?;
        }

        let destination = read_socks5_destination(&mut client).await?;
        if destination != target {
            return Err("SOCKS5 client requested an unexpected destination".to_string());
        }
        let mut target_stream = TcpStream::connect(target)
            .await
            .map_err(|e| format!("SOCKS5 target connection failed: {e}"))?;
        client
            .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
            .await
            .map_err(|e| format!("SOCKS5 CONNECT response failed: {e}"))?;
        tokio::io::copy_bidirectional(&mut client, &mut target_stream)
            .await
            .map_err(|e| format!("SOCKS5 forwarding failed: {e}"))?;
        Ok(())
    }

    async fn await_test_task<T>(task: JoinHandle<Result<T, String>>) -> Result<T, String> {
        let joined = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .map_err(|_| "local network test task timed out".to_string())?;
        joined.map_err(|e| format!("local network test task failed: {e}"))?
    }

    async fn exercise_http_request(protocol: ProxyProtocol, auth: AuthCase) -> Result<(), String> {
        let (target, target_task) = spawn_http_target().await;
        let proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy listener failed: {e}"))?;
        let proxy_addr = proxy_listener
            .local_addr()
            .map_err(|e| format!("proxy address failed: {e}"))?;
        let proxy_task = match protocol {
            ProxyProtocol::Http => tokio::spawn(run_http_forward_proxy(
                proxy_listener,
                target,
                expected_http_auth(auth),
            )),
            ProxyProtocol::Socks5 => tokio::spawn(run_socks5_proxy(
                proxy_listener,
                target,
                expected_socks5_auth(auth),
            )),
        };

        let manager = NetworkManager::new(manual_settings(protocol, proxy_addr, auth))?;
        let client = manager.client().await;
        let url = format!("http://127.0.0.1:{}/route", target.port());
        let response =
            match tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    let proxy_result = await_test_task(proxy_task).await;
                    let target_result = await_test_task(target_task).await;
                    return Err(format!(
                    "HTTP request failed: {error}; proxy={proxy_result:?}; target={target_result:?}"
                ));
                }
                Err(_) => {
                    let proxy_result = await_test_task(proxy_task).await;
                    let target_result = await_test_task(target_task).await;
                    return Err(format!(
                        "HTTP request timed out; proxy={proxy_result:?}; target={target_result:?}"
                    ));
                }
            };
        if !response.status().is_success() {
            return Err(format!("HTTP target returned {}", response.status()));
        }
        let body = response
            .text()
            .await
            .map_err(|e| format!("HTTP response body failed: {e}"))?;
        if body != HTTP_BODY {
            return Err("HTTP request reached an unexpected target".to_string());
        }

        await_test_task(proxy_task).await?;
        let target_request = await_test_task(target_task).await?;
        if !String::from_utf8_lossy(&target_request).starts_with("GET ") {
            return Err("HTTP target did not receive a GET request".to_string());
        }
        Ok(())
    }

    async fn exercise_websocket_request(
        protocol: ProxyProtocol,
        auth: AuthCase,
    ) -> Result<(), String> {
        let (target, target_task) = spawn_websocket_target().await;
        let proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy listener failed: {e}"))?;
        let proxy_addr = proxy_listener
            .local_addr()
            .map_err(|e| format!("proxy address failed: {e}"))?;
        let proxy_task = match protocol {
            ProxyProtocol::Http => tokio::spawn(run_http_connect_proxy(
                proxy_listener,
                target,
                expected_http_auth(auth),
            )),
            ProxyProtocol::Socks5 => tokio::spawn(run_socks5_proxy(
                proxy_listener,
                target,
                expected_socks5_auth(auth),
            )),
        };

        let manager = NetworkManager::new(manual_settings(protocol, proxy_addr, auth))?;
        let url = format!("ws://127.0.0.1:{}/live", target.port());
        let mut websocket =
            tokio::time::timeout(Duration::from_secs(3), manager.connect_websocket(&url))
                .await
                .map_err(|_| "WebSocket connection timed out".to_string())?
                .map_err(|e| format!("WebSocket connection failed: {e}"))?;
        websocket
            .send(Message::Text("ws-probe".into()))
            .await
            .map_err(|e| format!("WebSocket request failed: {e}"))?;
        let message = tokio::time::timeout(Duration::from_secs(3), websocket.next())
            .await
            .map_err(|_| "WebSocket response timed out".to_string())?
            .ok_or_else(|| "WebSocket target closed without a response".to_string())?
            .map_err(|e| format!("WebSocket response failed: {e}"))?;
        match message {
            Message::Text(text) if text == "ws-target" => {}
            _ => return Err("WebSocket request reached an unexpected target".to_string()),
        }
        drop(websocket);

        await_test_task(proxy_task).await?;
        let received = await_test_task(target_task).await?;
        if received != "ws-probe" {
            return Err("WebSocket target received an unexpected message".to_string());
        }
        Ok(())
    }

    #[test]
    fn resolve_effective_proxy_has_one_auth_policy() {
        for protocol in [ProxyProtocol::Http, ProxyProtocol::Socks5] {
            let settings = manual_settings(
                protocol,
                "127.0.0.1:8080".parse().unwrap(),
                AuthCase {
                    enabled: true,
                    username: None,
                    password: None,
                },
            );
            // Empty saved credentials are not a usable auth identity; both
            // transports must therefore take the same no-auth path.
            let expected = match protocol {
                ProxyProtocol::Http => ResolvedProxy::Http {
                    host: "127.0.0.1".to_string(),
                    port: 8080,
                    auth: None,
                },
                ProxyProtocol::Socks5 => ResolvedProxy::Socks5 {
                    host: "127.0.0.1".to_string(),
                    port: 8080,
                    auth: None,
                },
            };
            assert_eq!(resolve_effective_proxy(&settings), expected);
        }
    }

    #[test]
    fn proxy_debug_output_redacts_credentials() {
        let settings = manual_settings(
            ProxyProtocol::Http,
            "127.0.0.1:8080".parse().unwrap(),
            AuthCase {
                enabled: true,
                username: Some("user:name"),
                password: Some("p@ss/word?%"),
            },
        );
        let settings_debug = format!("{settings:?}");
        let resolved_debug = format!("{:?}", resolve_effective_proxy(&settings));
        for value in ["user:name", "p@ss/word?%"] {
            assert!(!settings_debug.contains(value));
            assert!(!resolved_debug.contains(value));
        }
        assert!(settings_debug.contains("[REDACTED]"));
        assert!(resolved_debug.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn network_manager_direct_uses_direct_http_and_websocket_exit() {
        let settings = ProxySettings {
            mode: ProxyMode::Direct,
            ..ProxySettings::default()
        };
        let manager = NetworkManager::new(settings).unwrap();

        let (http_target, http_task) = spawn_http_target().await;
        let response = manager
            .client()
            .await
            .get(format!("http://127.0.0.1:{}/direct", http_target.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), HTTP_BODY);
        assert!(
            String::from_utf8_lossy(&await_test_task(http_task).await.unwrap()).starts_with("GET ")
        );

        let (ws_target, ws_task) = spawn_websocket_target().await;
        let mut websocket = manager
            .connect_websocket(&format!("ws://127.0.0.1:{}/direct", ws_target.port()))
            .await
            .unwrap();
        websocket
            .send(Message::Text("ws-probe".into()))
            .await
            .unwrap();
        assert!(matches!(
            websocket.next().await.unwrap().unwrap(),
            Message::Text(text) if text == "ws-target"
        ));
        drop(websocket);
        assert_eq!(await_test_task(ws_task).await.unwrap(), "ws-probe");
    }

    #[tokio::test]
    async fn network_manager_manual_http_proxy_matches_http_and_websocket_auth_rules() {
        for (case, auth) in auth_cases().into_iter().enumerate() {
            exercise_http_request(ProxyProtocol::Http, auth)
                .await
                .unwrap_or_else(|error| panic!("HTTP proxy case {case}: {error}"));
            exercise_websocket_request(ProxyProtocol::Http, auth)
                .await
                .unwrap_or_else(|error| panic!("WebSocket proxy case {case}: {error}"));
        }
    }

    #[tokio::test]
    async fn network_manager_manual_socks5_proxy_matches_http_and_websocket_auth_rules() {
        for (case, auth) in auth_cases().into_iter().enumerate() {
            exercise_http_request(ProxyProtocol::Socks5, auth)
                .await
                .unwrap_or_else(|error| panic!("HTTP SOCKS5 case {case}: {error}"));
            exercise_websocket_request(ProxyProtocol::Socks5, auth)
                .await
                .unwrap_or_else(|error| panic!("WebSocket SOCKS5 case {case}: {error}"));
        }
    }
}
