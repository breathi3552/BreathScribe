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
    use tokio_tungstenite::{accept_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

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

    fn system_settings(protocol: ProxyProtocol, auth: AuthCase) -> ProxySettings {
        ProxySettings {
            mode: ProxyMode::System,
            protocol: match protocol {
                ProxyProtocol::Http => ProxyProtocol::Socks5,
                ProxyProtocol::Socks5 => ProxyProtocol::Http,
            },
            host: "ignored.system.proxy".to_string(),
            port: 1,
            auth_enabled: auth.enabled,
            username: auth.username.map(str::to_string),
            password: auth.password.map(str::to_string),
        }
    }

    fn detected_system_proxy(
        protocol: ProxyProtocol,
        proxy_addr: SocketAddr,
    ) -> system_proxy::DetectedProxy {
        system_proxy::DetectedProxy {
            host: proxy_addr.ip().to_string(),
            port: proxy_addr.port(),
            protocol,
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

    async fn http_round_trip(client: &reqwest::Client, url: String) -> Result<String, String> {
        let response = tokio::time::timeout(Duration::from_secs(3), client.get(url).send())
            .await
            .map_err(|_| "HTTP request timed out".to_string())?
            .map_err(|e| format!("HTTP request failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("HTTP target returned {}", response.status()));
        }
        tokio::time::timeout(Duration::from_secs(3), response.text())
            .await
            .map_err(|_| "HTTP response timed out".to_string())?
            .map_err(|e| format!("HTTP response failed: {e}"))
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

    async fn spawn_websocket_target(
        message_count: usize,
    ) -> (SocketAddr, JoinHandle<Result<String, String>>) {
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
            let mut last_text = None;
            for _ in 0..message_count {
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
                last_text = Some(text);
            }
            last_text.ok_or_else(|| "WebSocket target expected a message".to_string())
        });
        (address, task)
    }

    async fn websocket_round_trip(
        websocket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        text: &str,
    ) -> Result<(), String> {
        tokio::time::timeout(
            Duration::from_secs(3),
            websocket.send(Message::Text(text.into())),
        )
        .await
        .map_err(|_| "WebSocket request timed out".to_string())?
        .map_err(|e| format!("WebSocket request failed: {e}"))?;
        let message = tokio::time::timeout(Duration::from_secs(3), websocket.next())
            .await
            .map_err(|_| "WebSocket response timed out".to_string())?
            .ok_or_else(|| "WebSocket target closed without a response".to_string())?
            .map_err(|e| format!("WebSocket response failed: {e}"))?;
        match message {
            Message::Text(response) if response == "ws-target" => Ok(()),
            _ => Err("WebSocket request reached an unexpected target".to_string()),
        }
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

    async fn exercise_http_request(
        protocol: ProxyProtocol,
        auth: AuthCase,
        system_mode: bool,
    ) -> Result<(), String> {
        let (target, target_task) = spawn_http_target().await;
        let proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy listener failed: {e}"))?;
        let proxy_addr = proxy_listener
            .local_addr()
            .map_err(|e| format!("proxy address failed: {e}"))?;
        let expected_http_auth = if system_mode {
            None
        } else {
            expected_http_auth(auth)
        };
        let _system_proxy = system_mode
            .then(|| detected_system_proxy(protocol, proxy_addr))
            .map(|proxy| system_proxy::test_system_proxy(Some(proxy)));
        let proxy_task = match protocol {
            ProxyProtocol::Http => tokio::spawn(run_http_forward_proxy(
                proxy_listener,
                target,
                expected_http_auth,
            )),
            ProxyProtocol::Socks5 => tokio::spawn(run_socks5_proxy(
                proxy_listener,
                target,
                if system_mode {
                    None
                } else {
                    expected_socks5_auth(auth)
                },
            )),
        };

        let settings = if system_mode {
            system_settings(protocol, auth)
        } else {
            manual_settings(protocol, proxy_addr, auth)
        };
        let manager = NetworkManager::new(settings)?;
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
        system_mode: bool,
    ) -> Result<(), String> {
        let (target, target_task) = spawn_websocket_target(1).await;
        let proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy listener failed: {e}"))?;
        let proxy_addr = proxy_listener
            .local_addr()
            .map_err(|e| format!("proxy address failed: {e}"))?;
        let expected_http_auth = if system_mode {
            None
        } else {
            expected_http_auth(auth)
        };
        let _system_proxy = system_mode
            .then(|| detected_system_proxy(protocol, proxy_addr))
            .map(|proxy| system_proxy::test_system_proxy(Some(proxy)));
        let proxy_task = match protocol {
            ProxyProtocol::Http => tokio::spawn(run_http_connect_proxy(
                proxy_listener,
                target,
                expected_http_auth,
            )),
            ProxyProtocol::Socks5 => tokio::spawn(run_socks5_proxy(
                proxy_listener,
                target,
                if system_mode {
                    None
                } else {
                    expected_socks5_auth(auth)
                },
            )),
        };

        let settings = if system_mode {
            system_settings(protocol, auth)
        } else {
            manual_settings(protocol, proxy_addr, auth)
        };
        let manager = NetworkManager::new(settings)?;
        let url = format!("ws://127.0.0.1:{}/live", target.port());
        let mut websocket =
            tokio::time::timeout(Duration::from_secs(3), manager.connect_websocket(&url))
                .await
                .map_err(|_| "WebSocket connection timed out".to_string())?
                .map_err(|e| format!("WebSocket connection failed: {e}"))?;
        websocket_round_trip(&mut websocket, "ws-probe").await?;
        drop(websocket);

        await_test_task(proxy_task).await?;
        let received = await_test_task(target_task).await?;
        if received != "ws-probe" {
            return Err("WebSocket target received an unexpected message".to_string());
        }
        Ok(())
    }

    #[tokio::test]
    async fn network_manager_system_proxy_uses_detected_protocol_and_ignores_saved_auth() {
        let stale_auth = auth_cases()[2];
        for protocol in [ProxyProtocol::Http, ProxyProtocol::Socks5] {
            exercise_http_request(protocol, stale_auth, true)
                .await
                .unwrap_or_else(|error| panic!("System HTTP proxy case {protocol:?}: {error}"));
            exercise_websocket_request(protocol, stale_auth, true)
                .await
                .unwrap_or_else(|error| {
                    panic!("System WebSocket proxy case {protocol:?}: {error}")
                });
        }
    }

    #[tokio::test]
    async fn system_proxy_change_keeps_old_http_client_and_routes_new_websocket(
    ) -> Result<(), String> {
        let (http_target, http_task) = spawn_http_target().await;
        let proxy_a_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy A listener failed: {e}"))?;
        let proxy_a_addr = proxy_a_listener
            .local_addr()
            .map_err(|e| format!("proxy A address failed: {e}"))?;
        let system_proxy_guard = system_proxy::test_system_proxy(Some(detected_system_proxy(
            ProxyProtocol::Http,
            proxy_a_addr,
        )));
        let manager = NetworkManager::new(system_settings(ProxyProtocol::Socks5, auth_cases()[2]))?;
        let old_client = manager.client().await;
        let proxy_a_task =
            tokio::spawn(run_http_forward_proxy(proxy_a_listener, http_target, None));

        let (websocket_target, websocket_task) = spawn_websocket_target(1).await;
        let proxy_b_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy B listener failed: {e}"))?;
        let proxy_b_addr = proxy_b_listener
            .local_addr()
            .map_err(|e| format!("proxy B address failed: {e}"))?;
        system_proxy_guard.set(Some(detected_system_proxy(
            ProxyProtocol::Http,
            proxy_b_addr,
        )));
        let proxy_b_task = tokio::spawn(run_http_connect_proxy(
            proxy_b_listener,
            websocket_target,
            None,
        ));

        let body = http_round_trip(
            &old_client,
            format!("http://127.0.0.1:{}/route-a", http_target.port()),
        )
        .await?;
        if body != HTTP_BODY {
            return Err("old HTTP client did not reach proxy A".to_string());
        }
        let request = await_test_task(http_task).await?;
        if !String::from_utf8_lossy(&request).starts_with("GET ") {
            return Err("proxy A target did not receive a GET request".to_string());
        }
        await_test_task(proxy_a_task).await?;

        let mut websocket = tokio::time::timeout(
            Duration::from_secs(3),
            manager.connect_websocket(&format!(
                "ws://127.0.0.1:{}/route-b",
                websocket_target.port()
            )),
        )
        .await
        .map_err(|_| "new WebSocket connection timed out".to_string())?
        .map_err(|e| format!("new WebSocket connection failed: {e}"))?;
        websocket_round_trip(&mut websocket, "ws-probe").await?;
        drop(websocket);
        await_test_task(proxy_b_task).await?;
        if await_test_task(websocket_task).await? != "ws-probe" {
            return Err("proxy B target received an unexpected message".to_string());
        }
        Ok(())
    }

    #[tokio::test]
    async fn system_proxy_change_preserves_existing_websocket_session() -> Result<(), String> {
        let (old_target, old_target_task) = spawn_websocket_target(2).await;
        let proxy_a_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy A listener failed: {e}"))?;
        let proxy_a_addr = proxy_a_listener
            .local_addr()
            .map_err(|e| format!("proxy A address failed: {e}"))?;
        let system_proxy_guard = system_proxy::test_system_proxy(Some(detected_system_proxy(
            ProxyProtocol::Http,
            proxy_a_addr,
        )));
        let manager = NetworkManager::new(system_settings(ProxyProtocol::Socks5, auth_cases()[2]))?;
        let proxy_a_task = tokio::spawn(run_http_connect_proxy(proxy_a_listener, old_target, None));
        let mut old_websocket = tokio::time::timeout(
            Duration::from_secs(3),
            manager.connect_websocket(&format!("ws://127.0.0.1:{}/old", old_target.port())),
        )
        .await
        .map_err(|_| "old WebSocket connection timed out".to_string())?
        .map_err(|e| format!("old WebSocket connection failed: {e}"))?;
        websocket_round_trip(&mut old_websocket, "old-before").await?;

        let (new_target, new_target_task) = spawn_websocket_target(1).await;
        let proxy_b_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy B listener failed: {e}"))?;
        let proxy_b_addr = proxy_b_listener
            .local_addr()
            .map_err(|e| format!("proxy B address failed: {e}"))?;
        system_proxy_guard.set(Some(detected_system_proxy(
            ProxyProtocol::Http,
            proxy_b_addr,
        )));
        let proxy_b_task = tokio::spawn(run_http_connect_proxy(proxy_b_listener, new_target, None));
        let mut new_websocket = tokio::time::timeout(
            Duration::from_secs(3),
            manager.connect_websocket(&format!("ws://127.0.0.1:{}/new", new_target.port())),
        )
        .await
        .map_err(|_| "new WebSocket connection timed out".to_string())?
        .map_err(|e| format!("new WebSocket connection failed: {e}"))?;
        websocket_round_trip(&mut new_websocket, "new-probe").await?;
        drop(new_websocket);
        await_test_task(proxy_b_task).await?;
        if await_test_task(new_target_task).await? != "new-probe" {
            return Err("proxy B target received an unexpected message".to_string());
        }

        websocket_round_trip(&mut old_websocket, "old-after").await?;
        drop(old_websocket);
        await_test_task(proxy_a_task).await?;
        if await_test_task(old_target_task).await? != "old-after" {
            return Err("existing WebSocket session did not stay on proxy A".to_string());
        }
        Ok(())
    }

    #[tokio::test]
    async fn explicit_system_proxy_update_rebuilds_http_client_without_moving_old_handle(
    ) -> Result<(), String> {
        let proxy_a_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy A listener failed: {e}"))?;
        let proxy_a_addr = proxy_a_listener
            .local_addr()
            .map_err(|e| format!("proxy A address failed: {e}"))?;
        let proxy_b_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy B listener failed: {e}"))?;
        let proxy_b_addr = proxy_b_listener
            .local_addr()
            .map_err(|e| format!("proxy B address failed: {e}"))?;
        let system_proxy_guard = system_proxy::test_system_proxy(Some(detected_system_proxy(
            ProxyProtocol::Http,
            proxy_a_addr,
        )));
        let settings = system_settings(ProxyProtocol::Socks5, auth_cases()[2]);
        let manager = NetworkManager::new(settings.clone())?;
        let old_client = manager.client().await;
        system_proxy_guard.set(Some(detected_system_proxy(
            ProxyProtocol::Http,
            proxy_b_addr,
        )));
        manager.update_proxy_settings(settings).await?;

        let (target_a, target_a_task) = spawn_http_target().await;
        let proxy_a_task = tokio::spawn(run_http_forward_proxy(proxy_a_listener, target_a, None));
        let (target_b, target_b_task) = spawn_http_target().await;
        let proxy_b_task = tokio::spawn(run_http_forward_proxy(proxy_b_listener, target_b, None));

        if http_round_trip(
            &old_client,
            format!("http://127.0.0.1:{}/old", target_a.port()),
        )
        .await?
            != HTTP_BODY
        {
            return Err("old HTTP client failed after proxy update".to_string());
        }
        let new_client = manager.client().await;
        if http_round_trip(
            &new_client,
            format!("http://127.0.0.1:{}/new", target_b.port()),
        )
        .await?
            != HTTP_BODY
        {
            return Err("new HTTP client did not use proxy B".to_string());
        }

        let request_a = await_test_task(target_a_task).await?;
        let request_b = await_test_task(target_b_task).await?;
        if !String::from_utf8_lossy(&request_a).contains(&format!("/old"))
            || !String::from_utf8_lossy(&request_b).contains(&format!("/new"))
        {
            return Err("HTTP requests did not reach their expected local exits".to_string());
        }
        await_test_task(proxy_a_task).await?;
        await_test_task(proxy_b_task).await?;
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

    async fn exercise_direct_connections(settings: ProxySettings) -> Result<(), String> {
        let manager = NetworkManager::new(settings)?;

        let (http_target, http_task) = spawn_http_target().await;
        let client = manager.client().await;
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            client
                .get(format!("http://127.0.0.1:{}/direct", http_target.port()))
                .send(),
        )
        .await
        .map_err(|_| "direct HTTP request timed out".to_string())?
        .map_err(|e| format!("direct HTTP request failed: {e}"))?;
        let body = tokio::time::timeout(Duration::from_secs(3), response.text())
            .await
            .map_err(|_| "direct HTTP response timed out".to_string())?
            .map_err(|e| format!("direct HTTP response failed: {e}"))?;
        if body != HTTP_BODY {
            return Err("direct HTTP request reached an unexpected target".to_string());
        }
        if !String::from_utf8_lossy(&await_test_task(http_task).await?).starts_with("GET ") {
            return Err("direct HTTP target did not receive a GET request".to_string());
        }

        let (ws_target, ws_task) = spawn_websocket_target(1).await;
        let mut websocket = tokio::time::timeout(
            Duration::from_secs(3),
            manager.connect_websocket(&format!("ws://127.0.0.1:{}/direct", ws_target.port())),
        )
        .await
        .map_err(|_| "direct WebSocket connection timed out".to_string())?
        .map_err(|e| format!("direct WebSocket connection failed: {e}"))?;
        websocket_round_trip(&mut websocket, "ws-probe").await?;
        drop(websocket);
        if await_test_task(ws_task).await? != "ws-probe" {
            return Err("direct WebSocket target received an unexpected message".to_string());
        }
        Ok(())
    }

    #[tokio::test]
    async fn network_manager_direct_uses_direct_http_and_websocket_exit() {
        exercise_direct_connections(ProxySettings {
            mode: ProxyMode::Direct,
            ..ProxySettings::default()
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn network_manager_system_without_proxy_uses_direct_http_and_websocket_exit() {
        let _system_proxy = system_proxy::test_system_proxy(None);
        exercise_direct_connections(system_settings(ProxyProtocol::Http, auth_cases()[2]))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn network_manager_manual_http_proxy_matches_http_and_websocket_auth_rules() {
        for (case, auth) in auth_cases().into_iter().enumerate() {
            exercise_http_request(ProxyProtocol::Http, auth, false)
                .await
                .unwrap_or_else(|error| panic!("HTTP proxy case {case}: {error}"));
            exercise_websocket_request(ProxyProtocol::Http, auth, false)
                .await
                .unwrap_or_else(|error| panic!("WebSocket proxy case {case}: {error}"));
        }
    }

    #[tokio::test]
    async fn network_manager_manual_socks5_proxy_matches_http_and_websocket_auth_rules() {
        for (case, auth) in auth_cases().into_iter().enumerate() {
            exercise_http_request(ProxyProtocol::Socks5, auth, false)
                .await
                .unwrap_or_else(|error| panic!("HTTP SOCKS5 case {case}: {error}"));
            exercise_websocket_request(ProxyProtocol::Socks5, auth, false)
                .await
                .unwrap_or_else(|error| panic!("WebSocket SOCKS5 case {case}: {error}"));
        }
    }
}
