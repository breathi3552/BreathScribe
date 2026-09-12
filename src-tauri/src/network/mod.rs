use crate::settings::{ProxyMode, ProxyProtocol, ProxySettings};
use reqwest::{Client, Proxy};
use std::fmt;
use std::future::Future;
use std::net::Ipv6Addr;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{Mutex as TokioMutex, RwLock};
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

/// Applies the same host and port checks used by the manual proxy form before
/// a candidate or saved setting can reach either transport.
pub(crate) fn normalize_proxy_settings(
    mut settings: ProxySettings,
) -> Result<ProxySettings, String> {
    if settings.mode == ProxyMode::Manual {
        settings.host = settings.host.trim().to_string();
        if settings.host.is_empty() {
            return Err("Proxy server host cannot be empty".to_string());
        }
        if settings.port == 0 {
            return Err("Proxy port must be between 1 and 65535".to_string());
        }
    }
    Ok(settings)
}

#[cfg(test)]
static TEST_CONNECTIVITY_URLS: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
#[cfg(test)]
static TEST_CONNECTIVITY_URLS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
pub(crate) struct TestConnectivityUrlsGuard {
    _serial: MutexGuard<'static, ()>,
}

#[cfg(test)]
impl TestConnectivityUrlsGuard {
    pub(crate) fn set(&self, urls: Vec<String>) {
        *TEST_CONNECTIVITY_URLS
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(urls);
    }
}

#[cfg(test)]
impl Drop for TestConnectivityUrlsGuard {
    fn drop(&mut self) {
        if let Some(urls) = TEST_CONNECTIVITY_URLS.get() {
            *urls.lock().unwrap_or_else(|error| error.into_inner()) = None;
        }
    }
}

#[cfg(test)]
pub(crate) fn test_connectivity_urls(urls: Vec<String>) -> TestConnectivityUrlsGuard {
    let guard = TestConnectivityUrlsGuard {
        _serial: TEST_CONNECTIVITY_URLS_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner()),
    };
    guard.set(urls);
    guard
}

fn connectivity_test_urls() -> Vec<String> {
    #[cfg(test)]
    if let Some(urls) = TEST_CONNECTIVITY_URLS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
    {
        return urls;
    }

    vec![
        "https://www.google.com/generate_204".to_string(),
        "https://generativelanguage.googleapis.com".to_string(),
    ]
}

struct NetworkState {
    client: Client,
    settings: ProxySettings,
}

pub struct NetworkManager {
    state: RwLock<NetworkState>,
    proxy_update_lock: TokioMutex<()>,
}

impl NetworkManager {
    pub fn new(initial_settings: ProxySettings) -> Result<Self, String> {
        let initial_settings = normalize_proxy_settings(initial_settings)?;
        let client = build_reqwest_client(&initial_settings)?;
        Ok(Self {
            state: RwLock::new(NetworkState {
                client,
                settings: initial_settings,
            }),
            proxy_update_lock: TokioMutex::new(()),
        })
    }

    pub async fn client(&self) -> Client {
        self.state.read().await.client.clone()
    }

    pub async fn connect_websocket(
        &self,
        url: &str,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, String> {
        let settings = self.state.read().await.settings.clone();
        let proxy = resolve_effective_proxy(&settings);
        proxy_tunnel::connect_websocket_tunnel(url, proxy).await
    }

    async fn install_proxy_settings(&self, new_settings: ProxySettings, new_client: Client) {
        let mut state = self.state.write().await;
        state.client = new_client;
        state.settings = new_settings;
    }

    pub(crate) async fn update_proxy_settings_with_persistence<F, Fut>(
        &self,
        new_settings: ProxySettings,
        persist: F,
    ) -> Result<(), String>
    where
        F: FnOnce(ProxySettings) -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        let _update_guard = self.proxy_update_lock.lock().await;
        let new_settings = normalize_proxy_settings(new_settings)?;
        let new_client = build_reqwest_client(&new_settings)?;
        persist(new_settings.clone()).await?;
        self.install_proxy_settings(new_settings, new_client).await;
        log::info!("NetworkManager: proxy client successfully reloaded");
        Ok(())
    }

    pub async fn update_proxy_settings(&self, new_settings: ProxySettings) -> Result<(), String> {
        self.update_proxy_settings_with_persistence(new_settings, |_| async { Ok(()) })
            .await
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
    let settings = normalize_proxy_settings(settings.clone())?;
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));

    match resolve_effective_proxy(&settings) {
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

#[cfg(not(test))]
const CONNECTIVITY_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
const TEST_CONNECTIVITY_TIMEOUT: Duration = Duration::from_millis(250);

fn connectivity_timeout() -> Duration {
    #[cfg(test)]
    {
        TEST_CONNECTIVITY_TIMEOUT
    }

    #[cfg(not(test))]
    {
        CONNECTIVITY_TIMEOUT
    }
}

/// Probe network connectivity and return round-trip latency in ms.
pub async fn test_connectivity(client: &Client) -> Result<u64, String> {
    let mut last_err = None;

    for url in connectivity_test_urls() {
        let start = std::time::Instant::now();
        let response =
            match tokio::time::timeout(connectivity_timeout(), client.get(&url).send()).await {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    last_err = Some(crate::llm_client::report_reqwest_error(
                        "Connectivity probe request failed",
                        &error,
                    ));
                    continue;
                }
                Err(_) => {
                    let safe_url = crate::llm_client::sanitized_url_for_log(&url);
                    log::warn!("Connectivity test probe timed out for {safe_url}");
                    last_err = Some(format!("Connectivity probe to {safe_url} timed out"));
                    continue;
                }
            };

        let safe_url = crate::llm_client::sanitized_url_for_log(&url);
        if response.status().is_success() {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            log::info!(
                "Connectivity test succeeded via {} in {} ms, status: {}",
                safe_url,
                elapsed_ms,
                response.status()
            );
            return Ok(elapsed_ms);
        }

        log::warn!(
            "Connectivity test probe returned HTTP {} from {}",
            response.status(),
            safe_url
        );
        last_err = Some(format!(
            "Connectivity probe returned HTTP {} from {}",
            response.status(),
            safe_url
        ));
    }

    Err(last_err.unwrap_or_else(|| "Connectivity probe failed: unknown error".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    use futures_util::{SinkExt, StreamExt};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::path::Path;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::oneshot;
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

    async fn spawn_http_only_target() -> (SocketAddr, JoinHandle<Result<Vec<Vec<u8>>, String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .map_err(|e| format!("HTTP-only target accept failed: {e}"))?;
                requests.push(read_headers(&mut stream).await?);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    HTTP_BODY.len(),
                    HTTP_BODY
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .map_err(|e| format!("HTTP-only target response failed: {e}"))?;
                stream
                    .shutdown()
                    .await
                    .map_err(|e| format!("HTTP-only target shutdown failed: {e}"))?;
            }
            Ok(requests)
        });
        (address, task)
    }

    async fn spawn_delayed_http_target() -> (
        SocketAddr,
        JoinHandle<Result<Vec<u8>, String>>,
        oneshot::Receiver<()>,
        oneshot::Sender<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_seen_tx, request_seen_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("delayed HTTP target accept failed: {e}"))?;
            let request = read_headers(&mut stream).await?;
            let _ = request_seen_tx.send(());
            release_rx
                .await
                .map_err(|_| "delayed HTTP target release was dropped".to_string())?;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                HTTP_BODY.len(),
                HTTP_BODY
            );
            stream
                .write_all(response.as_bytes())
                .await
                .map_err(|e| format!("delayed HTTP target response failed: {e}"))?;
            stream
                .shutdown()
                .await
                .map_err(|e| format!("delayed HTTP target shutdown failed: {e}"))?;
            Ok(request)
        });
        (address, task, request_seen_rx, release_tx)
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

    async fn run_http_proxy_for_http_and_connect(
        listener: TcpListener,
        http_target: SocketAddr,
        websocket_target: SocketAddr,
    ) -> Result<(), String> {
        for _ in 0..2 {
            let (mut client, _) = listener
                .accept()
                .await
                .map_err(|e| format!("HTTP proxy accept failed: {e}"))?;
            let request = read_headers(&mut client).await?;
            if String::from_utf8_lossy(&request).starts_with("GET http://127.0.0.1:") {
                let mut target_stream = TcpStream::connect(http_target)
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
            } else {
                let expected_target =
                    format!("CONNECT 127.0.0.1:{} HTTP/1.1", websocket_target.port());
                if !String::from_utf8_lossy(&request).starts_with(&expected_target) {
                    return Err("HTTP proxy received an unexpected request".to_string());
                }
                let mut target_stream = TcpStream::connect(websocket_target)
                    .await
                    .map_err(|e| format!("HTTP CONNECT target connection failed: {e}"))?;
                client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .map_err(|e| format!("HTTP CONNECT response failed: {e}"))?;
                tokio::io::copy_bidirectional(&mut client, &mut target_stream)
                    .await
                    .map_err(|e| format!("HTTP CONNECT forwarding failed: {e}"))?;
            }
        }
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

    #[derive(Clone, Copy)]
    enum ProxyFailure {
        Rejected,
        Authentication,
        ConnectionError,
        Silent,
    }

    async fn run_http_failure_proxy(
        listener: TcpListener,
        status: u16,
        require_authentication: bool,
        expected_prefix: &str,
    ) -> Result<(), String> {
        let (mut client, _) = listener
            .accept()
            .await
            .map_err(|e| format!("HTTP failure proxy accept failed: {e}"))?;
        let request = read_headers(&mut client).await?;
        if !String::from_utf8_lossy(&request).starts_with(expected_prefix) {
            return Err("HTTP failure proxy received an unexpected request".to_string());
        }
        if require_authentication && header_value(&request, "Proxy-Authorization").is_none() {
            return Err("HTTP failure proxy did not receive proxy credentials".to_string());
        }
        let reason = if status == 407 {
            "Proxy Authentication Required"
        } else {
            "Forbidden"
        };
        let response =
            format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        client
            .write_all(response.as_bytes())
            .await
            .map_err(|e| format!("HTTP failure proxy response failed: {e}"))?;
        client
            .shutdown()
            .await
            .map_err(|e| format!("HTTP failure proxy shutdown failed: {e}"))?;
        Ok(())
    }

    async fn run_silent_proxy(listener: TcpListener) -> Result<(), String> {
        let _ = listener
            .accept()
            .await
            .map_err(|e| format!("silent proxy accept failed: {e}"))?;
        std::future::pending::<Result<(), String>>().await
    }

    async fn run_socks5_failure_proxy(
        listener: TcpListener,
        failure: ProxyFailure,
    ) -> Result<(), String> {
        let (mut client, _) = listener
            .accept()
            .await
            .map_err(|e| format!("SOCKS5 failure proxy accept failed: {e}"))?;
        let mut greeting_header = [0u8; 2];
        client
            .read_exact(&mut greeting_header)
            .await
            .map_err(|e| format!("SOCKS5 failure greeting failed: {e}"))?;
        let mut methods = vec![0u8; greeting_header[1] as usize];
        client
            .read_exact(&mut methods)
            .await
            .map_err(|e| format!("SOCKS5 failure methods failed: {e}"))?;

        match failure {
            ProxyFailure::Rejected => {
                client
                    .write_all(&[0x05, 0x00])
                    .await
                    .map_err(|e| format!("SOCKS5 rejection method failed: {e}"))?;
                read_socks5_destination(&mut client).await?;
                client
                    .write_all(&[0x05, 0x05, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
                    .await
                    .map_err(|e| format!("SOCKS5 rejection response failed: {e}"))?;
            }
            ProxyFailure::Authentication => {
                if !methods.contains(&0x02) {
                    return Err("SOCKS5 client did not offer username/password auth".to_string());
                }
                client
                    .write_all(&[0x05, 0x02])
                    .await
                    .map_err(|e| format!("SOCKS5 auth method failed: {e}"))?;
                let mut auth_header = [0u8; 2];
                client
                    .read_exact(&mut auth_header)
                    .await
                    .map_err(|e| format!("SOCKS5 auth header failed: {e}"))?;
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
                client
                    .write_all(&[0x01, 0x01])
                    .await
                    .map_err(|e| format!("SOCKS5 auth rejection failed: {e}"))?;
            }
            ProxyFailure::ConnectionError => unreachable!(),
            ProxyFailure::Silent => {
                client
                    .write_all(&[0x05, 0x00])
                    .await
                    .map_err(|e| format!("SOCKS5 silent method failed: {e}"))?;
                read_socks5_destination(&mut client).await?;
                client
                    .write_all(&[0x05, 0x00, 0x00, 0x01])
                    .await
                    .map_err(|e| format!("SOCKS5 partial response failed: {e}"))?;
                std::future::pending::<()>().await
            }
        }
        Ok(())
    }

    async fn assert_no_direct_target_access(listener: &TcpListener) -> Result<(), String> {
        match tokio::time::timeout(Duration::from_millis(100), listener.accept()).await {
            Ok(Ok((stream, _))) => {
                drop(stream);
                Err("request bypassed the failed proxy and reached the target".to_string())
            }
            Ok(Err(error)) => Err(format!("target access check failed: {error}")),
            Err(_) => Ok(()),
        }
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

    fn read_persisted_proxy(path: &Path) -> Result<ProxySettings, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("read persisted proxy: {e}"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("decode persisted proxy: {e}"))
    }

    fn read_persisted_settings(path: &Path) -> crate::settings::AppSettings {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
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
    async fn system_proxy_invalid_environment_candidate_falls_back_for_http_and_websocket(
    ) -> Result<(), String> {
        let (http_target, http_task) = spawn_http_target().await;
        let (websocket_target, websocket_task) = spawn_websocket_target(1).await;
        let proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy listener failed: {e}"))?;
        let proxy_addr = proxy_listener
            .local_addr()
            .map_err(|e| format!("proxy address failed: {e}"))?;
        let proxy_url = format!("http://127.0.0.1:{}", proxy_addr.port());
        let _system_proxy = system_proxy::test_system_proxy_environment([
            None,
            None,
            None,
            Some("not a proxy URL"),
            Some(proxy_url.as_str()),
            None,
        ]);
        let manager = NetworkManager::new(system_settings(ProxyProtocol::Socks5, auth_cases()[2]))?;
        let proxy_task = tokio::spawn(run_http_proxy_for_http_and_connect(
            proxy_listener,
            http_target,
            websocket_target,
        ));

        let client = manager.client().await;
        if http_round_trip(
            &client,
            format!("http://127.0.0.1:{}/fallback-http", http_target.port()),
        )
        .await?
            != HTTP_BODY
        {
            return Err("HTTP request did not use the lower-priority valid proxy".to_string());
        }

        let mut websocket = manager
            .connect_websocket(&format!(
                "ws://127.0.0.1:{}/fallback-websocket",
                websocket_target.port()
            ))
            .await?;
        websocket_round_trip(&mut websocket, "fallback-probe").await?;
        drop(websocket);

        await_test_task(proxy_task).await?;
        await_test_task(http_task).await?;
        if await_test_task(websocket_task).await? != "fallback-probe" {
            return Err("WebSocket request did not use the lower-priority valid proxy".to_string());
        }
        Ok(())
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
    async fn manual_proxy_update_routes_new_websocket_and_preserves_existing_session(
    ) -> Result<(), String> {
        let no_auth = auth_cases()[0];
        let (old_target, old_target_task) = spawn_websocket_target(2).await;
        let proxy_a_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy A listener failed: {e}"))?;
        let proxy_a_addr = proxy_a_listener
            .local_addr()
            .map_err(|e| format!("proxy A address failed: {e}"))?;
        let initial = manual_settings(ProxyProtocol::Http, proxy_a_addr, no_auth);
        let manager = Arc::new(NetworkManager::new(initial.clone())?);
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
        crate::commands::network::update_proxy_settings_with_persistence(
            manager.as_ref(),
            manual_settings(ProxyProtocol::Http, proxy_b_addr, no_auth),
            |_| async { Ok(()) },
        )
        .await?;
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
    async fn update_proxy_settings_persists_then_reloads_http_client() -> Result<(), String> {
        let temp_dir = tempfile::tempdir().map_err(|e| format!("temp store failed: {e}"))?;
        let no_auth = auth_cases()[0];

        let (target_a, target_a_task, request_seen, release_request) =
            spawn_delayed_http_target().await;
        let proxy_a_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy A listener failed: {e}"))?;
        let proxy_a_addr = proxy_a_listener
            .local_addr()
            .map_err(|e| format!("proxy A address failed: {e}"))?;
        let (target_b, target_b_task) = spawn_http_target().await;
        let proxy_b_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy B listener failed: {e}"))?;
        let proxy_b_addr = proxy_b_listener
            .local_addr()
            .map_err(|e| format!("proxy B address failed: {e}"))?;
        let initial = manual_settings(ProxyProtocol::Http, proxy_a_addr, no_auth);
        let updated = manual_settings(ProxyProtocol::Http, proxy_b_addr, no_auth);
        let manager = Arc::new(NetworkManager::new(initial.clone())?);
        let old_client = manager.client().await;
        let settings_path = temp_dir.path().join("settings.json");
        let path_for_persist = settings_path.clone();
        let proxy_a_task = tokio::spawn(run_http_forward_proxy(proxy_a_listener, target_a, None));
        let old_request = tokio::spawn({
            let old_client = old_client.clone();
            let url = format!("http://127.0.0.1:{}/old", target_a.port());
            async move { http_round_trip(&old_client, url).await }
        });
        tokio::time::timeout(Duration::from_secs(3), request_seen)
            .await
            .map_err(|_| "old HTTP request did not reach proxy A".to_string())?
            .map_err(|_| "old HTTP request signal was dropped".to_string())?;

        crate::commands::network::update_proxy_settings_with_persistence(
            manager.as_ref(),
            updated.clone(),
            move |settings| async move {
                std::fs::write(
                    &path_for_persist,
                    serde_json::to_vec(&settings)
                        .map_err(|e| format!("serialize proxy failed: {e}"))?,
                )
                .map_err(|e| format!("persist proxy failed: {e}"))
            },
        )
        .await?;

        let proxy_b_task = tokio::spawn(run_http_forward_proxy(proxy_b_listener, target_b, None));
        release_request
            .send(())
            .map_err(|_| "old HTTP target was dropped before release".to_string())?;
        if await_test_task(old_request).await? != HTTP_BODY {
            return Err("old HTTP request did not complete after the update".to_string());
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
        await_test_task(proxy_a_task).await?;
        await_test_task(proxy_b_task).await?;
        let request_a = await_test_task(target_a_task).await?;
        let request_b = await_test_task(target_b_task).await?;
        if !String::from_utf8_lossy(&request_a).contains("/old")
            || !String::from_utf8_lossy(&request_b).contains("/new")
        {
            return Err("HTTP requests did not reach their expected exits".to_string());
        }
        assert_eq!(read_persisted_proxy(&settings_path)?, updated);
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_proxy_updates_keep_disk_memory_and_new_transports_aligned(
    ) -> Result<(), String> {
        let temp_dir = tempfile::tempdir().map_err(|e| format!("temp store failed: {e}"))?;
        let settings_path = temp_dir.path().join("settings.json");
        let no_auth = auth_cases()[0];
        let initial_proxy = ProxySettings {
            mode: ProxyMode::Direct,
            ..ProxySettings::default()
        };
        let mut initial = crate::settings::get_default_settings();
        initial.proxy = initial_proxy.clone();
        initial
            .cloud_stt_api_keys
            .insert("gemini".to_string(), "old-api-key".to_string());
        std::fs::write(
            &settings_path,
            serde_json::to_vec(&initial)
                .map_err(|e| format!("serialize initial settings failed: {e}"))?,
        )
        .map_err(|e| format!("write initial settings failed: {e}"))?;

        let proxy_b_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy B listener failed: {e}"))?;
        let proxy_b_addr = proxy_b_listener
            .local_addr()
            .map_err(|e| format!("proxy B address failed: {e}"))?;
        let unavailable_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy A address allocation failed: {e}"))?;
        let proxy_a_addr = unavailable_listener
            .local_addr()
            .map_err(|e| format!("proxy A address failed: {e}"))?;
        drop(unavailable_listener);

        let proxy_a = manual_settings(ProxyProtocol::Http, proxy_a_addr, no_auth);
        let proxy_b = manual_settings(ProxyProtocol::Http, proxy_b_addr, no_auth);
        let manager = Arc::new(NetworkManager::new(initial_proxy)?);

        let path_for_a = settings_path.clone();
        let update_a = crate::commands::network::update_proxy_settings_with_persistence(
            manager.as_ref(),
            proxy_a.clone(),
            move |settings| async move {
                crate::settings::with_settings_update(
                    || read_persisted_settings(&path_for_a),
                    |current| current.proxy = settings,
                    |persisted| {
                        std::fs::write(
                            &path_for_a,
                            serde_json::to_vec(&persisted)
                                .map_err(|e| format!("serialize proxy A failed: {e}"))?,
                        )
                        .map_err(|e| format!("persist proxy A failed: {e}"))
                    },
                )?;
                // Explicitly yield after the production settings transaction.
                // `tokio::join!` polls update A first, so update B can persist
                // and publish before A publishes without relying on timing.
                tokio::task::yield_now().await;
                Ok(())
            },
        );
        let path_for_b = settings_path.clone();
        let update_b = crate::commands::network::update_proxy_settings_with_persistence(
            manager.as_ref(),
            proxy_b.clone(),
            move |settings| async move {
                crate::settings::with_settings_update(
                    || read_persisted_settings(&path_for_b),
                    |current| current.proxy = settings,
                    |persisted| {
                        std::fs::write(
                            &path_for_b,
                            serde_json::to_vec(&persisted)
                                .map_err(|e| format!("serialize proxy B failed: {e}"))?,
                        )
                        .map_err(|e| format!("persist proxy B failed: {e}"))
                    },
                )
            },
        );
        let path_for_api_key = settings_path.clone();
        let update_api_key = async move {
            crate::settings::with_settings_update(
                || read_persisted_settings(&path_for_api_key),
                |settings| {
                    settings
                        .cloud_stt_api_keys
                        .insert("gemini".to_string(), "new-api-key".to_string());
                },
                |persisted| {
                    std::fs::write(
                        &path_for_api_key,
                        serde_json::to_vec(&persisted)
                            .map_err(|e| format!("serialize API key update failed: {e}"))?,
                    )
                    .map_err(|e| format!("persist API key update failed: {e}"))
                },
            )
        };
        let (result_a, result_b, result_api_key) = tokio::join!(update_a, update_b, update_api_key);
        result_a?;
        result_b?;
        result_api_key?;

        let persisted = read_persisted_settings(&settings_path);
        assert_eq!(persisted.proxy, proxy_b);
        assert_eq!(
            persisted.cloud_stt_api_keys.get("gemini"),
            Some(&"new-api-key".to_string())
        );
        assert_eq!(manager.state.read().await.settings, proxy_b);

        let (http_target, http_task) = spawn_http_target().await;
        let (websocket_target, websocket_task) = spawn_websocket_target(1).await;
        let proxy_b_task = tokio::spawn(run_http_proxy_for_http_and_connect(
            proxy_b_listener,
            http_target,
            websocket_target,
        ));
        let client = manager.client().await;
        let http_result = http_round_trip(
            &client,
            format!("http://127.0.0.1:{}/final-http", http_target.port()),
        )
        .await;
        if let Err(error) = http_result {
            proxy_b_task.abort();
            return Err(format!("final HTTP request did not use proxy B: {error}"));
        }

        let mut websocket = manager
            .connect_websocket(&format!(
                "ws://127.0.0.1:{}/final-websocket",
                websocket_target.port()
            ))
            .await?;
        websocket_round_trip(&mut websocket, "final-probe").await?;
        drop(websocket);

        await_test_task(proxy_b_task).await?;
        await_test_task(http_task).await?;
        if await_test_task(websocket_task).await? != "final-probe" {
            return Err("final WebSocket request reached an unexpected target".to_string());
        }
        Ok(())
    }

    #[tokio::test]
    async fn candidate_proxy_tests_use_independent_clients_and_keep_global_settings(
    ) -> Result<(), String> {
        let no_auth = auth_cases()[0];
        let proxy_a_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy A listener failed: {e}"))?;
        let proxy_a_addr = proxy_a_listener
            .local_addr()
            .map_err(|e| format!("proxy A address failed: {e}"))?;
        let initial = manual_settings(ProxyProtocol::Http, proxy_a_addr, no_auth);
        let manager = Arc::new(NetworkManager::new(initial)?);

        let (candidate_target, candidate_target_task) = spawn_http_target().await;
        let candidate_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("candidate proxy listener failed: {e}"))?;
        let candidate_addr = candidate_listener
            .local_addr()
            .map_err(|e| format!("candidate proxy address failed: {e}"))?;
        let candidate = manual_settings(ProxyProtocol::Http, candidate_addr, no_auth);
        let urls = test_connectivity_urls(vec![format!(
            "http://127.0.0.1:{}/candidate",
            candidate_target.port()
        )]);
        let candidate_task = tokio::spawn(run_http_forward_proxy(
            candidate_listener,
            candidate_target,
            None,
        ));
        crate::commands::network::test_candidate_proxy_connectivity(candidate).await?;
        await_test_task(candidate_task).await?;
        await_test_task(candidate_target_task).await?;

        let unused_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("failure port allocation failed: {e}"))?;
        let unavailable = unused_listener
            .local_addr()
            .map_err(|e| format!("failure port lookup failed: {e}"))?;
        drop(unused_listener);
        urls.set(vec![format!(
            "http://127.0.0.1:{}/failure",
            unavailable.port()
        )]);
        let failure = manual_settings(ProxyProtocol::Http, unavailable, no_auth);
        assert!(
            crate::commands::network::test_candidate_proxy_connectivity(failure)
                .await
                .is_err()
        );

        urls.set(vec!["http://127.0.0.1:1/invalid".to_string()]);
        let invalid = ProxySettings {
            mode: ProxyMode::Manual,
            host: "   ".to_string(),
            port: 0,
            ..Default::default()
        };
        assert!(
            crate::commands::network::test_candidate_proxy_connectivity(invalid)
                .await
                .is_err()
        );

        let (global_http_target, global_http_target_task) = spawn_http_target().await;
        let (global_websocket_target, global_websocket_target_task) =
            spawn_websocket_target(1).await;
        let global_proxy_task = tokio::spawn(run_http_proxy_for_http_and_connect(
            proxy_a_listener,
            global_http_target,
            global_websocket_target,
        ));
        let client = manager.client().await;
        if http_round_trip(
            &client,
            format!("http://127.0.0.1:{}/global", global_http_target.port()),
        )
        .await?
            != HTTP_BODY
        {
            return Err("global HTTP client did not stay on proxy A".to_string());
        }
        let mut websocket = tokio::time::timeout(
            Duration::from_secs(3),
            manager.connect_websocket(&format!(
                "ws://127.0.0.1:{}/global",
                global_websocket_target.port()
            )),
        )
        .await
        .map_err(|_| "global WebSocket connection timed out".to_string())?
        .map_err(|e| format!("global WebSocket connection failed: {e}"))?;
        websocket_round_trip(&mut websocket, "global-probe").await?;
        drop(websocket);
        await_test_task(global_proxy_task).await?;
        await_test_task(global_http_target_task).await?;
        await_test_task(global_websocket_target_task).await?;
        Ok(())
    }

    #[tokio::test]
    async fn failed_proxy_persistence_keeps_client_and_disk_unchanged() -> Result<(), String> {
        let temp_dir = tempfile::tempdir().map_err(|e| format!("temp store failed: {e}"))?;
        let settings_path = temp_dir.path().join("settings.json");
        let no_auth = auth_cases()[0];
        let proxy_a_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("proxy A listener failed: {e}"))?;
        let proxy_a_addr = proxy_a_listener
            .local_addr()
            .map_err(|e| format!("proxy A address failed: {e}"))?;
        let initial = manual_settings(ProxyProtocol::Http, proxy_a_addr, no_auth);
        std::fs::write(
            &settings_path,
            serde_json::to_vec(&initial).map_err(|e| format!("serialize proxy failed: {e}"))?,
        )
        .map_err(|e| format!("write proxy failed: {e}"))?;
        let manager = Arc::new(NetworkManager::new(initial.clone())?);
        let old_client = manager.client().await;
        let error = crate::commands::network::update_proxy_settings_with_persistence(
            manager.as_ref(),
            manual_settings(ProxyProtocol::Http, "127.0.0.1:1".parse().unwrap(), no_auth),
            |_| async { Err("persist failed".to_string()) },
        )
        .await
        .expect_err("failed persistence must reject the update");
        assert_eq!(error, "persist failed");
        assert_eq!(read_persisted_proxy(&settings_path)?, initial);
        assert_eq!(manager.state.read().await.settings, initial);

        let (target, target_task) = spawn_http_target().await;
        let proxy_task = tokio::spawn(run_http_forward_proxy(proxy_a_listener, target, None));
        if http_round_trip(
            &old_client,
            format!("http://127.0.0.1:{}/after-failure", target.port()),
        )
        .await?
            != HTTP_BODY
        {
            return Err("old HTTP client stopped working after persistence failure".to_string());
        }
        await_test_task(proxy_task).await?;
        await_test_task(target_task).await?;
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
        if !String::from_utf8_lossy(&request_a).contains("/old")
            || !String::from_utf8_lossy(&request_b).contains("/new")
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

    #[tokio::test]
    async fn network_manager_reports_http_success_but_real_websocket_upgrade_failure(
    ) -> Result<(), String> {
        let (target, target_task) = spawn_http_only_target().await;
        let _probe_urls = test_connectivity_urls(vec![format!(
            "http://127.0.0.1:{}/probe?key=local-test-key",
            target.port()
        )]);
        let manager = NetworkManager::new(ProxySettings {
            mode: ProxyMode::Direct,
            ..ProxySettings::default()
        })?;
        let client = manager.client().await;
        test_connectivity(&client).await?;

        let websocket_result = manager
            .connect_websocket(&format!(
                "ws://127.0.0.1:{}/live?key=local-test-key",
                target.port()
            ))
            .await;
        assert!(
            websocket_result.is_err(),
            "an HTTP 200 response must not be accepted as a WebSocket upgrade"
        );

        let requests = await_test_task(target_task).await?;
        assert_eq!(requests.len(), 2);
        let http_request = String::from_utf8_lossy(&requests[0]).to_ascii_lowercase();
        assert!(http_request.starts_with("get /probe"));
        assert!(!http_request.contains("\r\nupgrade: websocket"));
        let websocket_request = String::from_utf8_lossy(&requests[1]).to_ascii_lowercase();
        assert!(websocket_request.starts_with("get /live"));
        assert!(websocket_request.contains("\r\nupgrade: websocket"));
        Ok(())
    }

    #[tokio::test]
    async fn network_manager_reports_proxy_failures_without_direct_fallback() -> Result<(), String>
    {
        const SENSITIVE: &str = "proxy-test-api-key";
        let failures = [
            ProxyFailure::Rejected,
            ProxyFailure::Authentication,
            ProxyFailure::ConnectionError,
            ProxyFailure::Silent,
        ];

        for protocol in [ProxyProtocol::Http, ProxyProtocol::Socks5] {
            for websocket in [false, true] {
                for failure in failures {
                    let target_listener = TcpListener::bind("127.0.0.1:0")
                        .await
                        .map_err(|e| format!("target listener failed: {e}"))?;
                    let target_addr = target_listener
                        .local_addr()
                        .map_err(|e| format!("target address failed: {e}"))?;
                    let expected_prefix = if websocket { "CONNECT " } else { "GET http://" };
                    let auth = if matches!(failure, ProxyFailure::Authentication) {
                        AuthCase {
                            enabled: true,
                            username: Some("failure-user"),
                            password: Some("failure-password"),
                        }
                    } else {
                        auth_cases()[0]
                    };

                    let (proxy_addr, proxy_task) =
                        if matches!(failure, ProxyFailure::ConnectionError) {
                            let listener = TcpListener::bind("127.0.0.1:0")
                                .await
                                .map_err(|e| format!("proxy listener failed: {e}"))?;
                            let address = listener
                                .local_addr()
                                .map_err(|e| format!("proxy address failed: {e}"))?;
                            drop(listener);
                            (address, None)
                        } else {
                            let listener = TcpListener::bind("127.0.0.1:0")
                                .await
                                .map_err(|e| format!("proxy listener failed: {e}"))?;
                            let address = listener
                                .local_addr()
                                .map_err(|e| format!("proxy address failed: {e}"))?;
                            let task = match protocol {
                                ProxyProtocol::Http => match failure {
                                    ProxyFailure::Rejected => tokio::spawn(run_http_failure_proxy(
                                        listener,
                                        403,
                                        false,
                                        expected_prefix,
                                    )),
                                    ProxyFailure::Authentication => {
                                        tokio::spawn(run_http_failure_proxy(
                                            listener,
                                            407,
                                            true,
                                            expected_prefix,
                                        ))
                                    }
                                    ProxyFailure::Silent => {
                                        tokio::spawn(run_silent_proxy(listener))
                                    }
                                    ProxyFailure::ConnectionError => unreachable!(),
                                },
                                ProxyProtocol::Socks5 => {
                                    tokio::spawn(run_socks5_failure_proxy(listener, failure))
                                }
                            };
                            (address, Some(task))
                        };

                    let manager = NetworkManager::new(manual_settings(protocol, proxy_addr, auth))?;
                    let started = std::time::Instant::now();
                    let error = if websocket {
                        manager
                            .connect_websocket(&format!(
                                "ws://{}:{}/live?key={SENSITIVE}",
                                target_addr.ip(),
                                target_addr.port()
                            ))
                            .await
                            .expect_err("failed WebSocket proxy operation must return an error")
                    } else {
                        let _probe_urls = test_connectivity_urls(vec![format!(
                            "http://{}:{}/probe?key={SENSITIVE}",
                            target_addr.ip(),
                            target_addr.port()
                        )]);
                        test_connectivity(&manager.client().await)
                            .await
                            .expect_err("failed HTTP proxy operation must return an error")
                    };
                    assert!(!error.is_empty());
                    assert!(
                        !error.contains(SENSITIVE),
                        "proxy failure leaked the API key: {error}"
                    );
                    if matches!(failure, ProxyFailure::Silent) {
                        assert!(
                            started.elapsed() < Duration::from_secs(2),
                            "silent proxy failure exceeded the bounded test deadline: {:?}",
                            started.elapsed()
                        );
                    }

                    assert_no_direct_target_access(&target_listener).await?;
                    if let Some(task) = proxy_task {
                        if matches!(failure, ProxyFailure::Silent) {
                            task.abort();
                            let _ = task.await;
                        } else {
                            await_test_task(task).await?;
                        }
                    }
                }
            }
        }
        Ok(())
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
