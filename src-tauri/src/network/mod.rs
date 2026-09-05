use crate::settings::{ProxyMode, ProxyProtocol, ProxySettings};
use reqwest::{Client, Proxy};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

pub mod system_proxy;

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

    /// 获取共享连接池 HTTP Client 的克隆（reqwest::Client 内部自带 Arc）
    pub async fn client(&self) -> Client {
        self.client.read().await.clone()
    }

    /// 更新代理配置并即时原子替换全局 Client
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

pub fn build_reqwest_client(settings: &ProxySettings) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));

    match settings.mode {
        ProxyMode::Direct => {
            builder = builder.no_proxy();
        }
        ProxyMode::System => {
            if let Some(detected) = system_proxy::get_system_proxy() {
                let proxy_url = format!(
                    "{}://{}:{}",
                    match detected.protocol {
                        ProxyProtocol::Http => "http",
                        ProxyProtocol::Socks5 => "socks5h",
                    },
                    detected.host,
                    detected.port
                );
                let proxy = Proxy::all(&proxy_url)
                    .map_err(|e| format!("Invalid system proxy {}: {}", proxy_url, e))?;
                builder = builder.proxy(proxy);
            } else {
                builder = builder.no_proxy();
            }
        }
        ProxyMode::Manual => {
            let scheme = match settings.protocol {
                ProxyProtocol::Http => "http",
                ProxyProtocol::Socks5 => "socks5h",
            };
            let proxy_url = if settings.auth_enabled {
                let user = settings.username.as_deref().unwrap_or_default();
                let pass = settings.password.as_deref().unwrap_or_default();
                format!(
                    "{}://{}:{}@{}:{}",
                    scheme, user, pass, settings.host, settings.port
                )
            } else {
                format!("{}://{}:{}", scheme, settings.host, settings.port)
            };
            let proxy = Proxy::all(&proxy_url)
                .map_err(|e| format!("Invalid manual proxy: {}", e))?;
            builder = builder.proxy(proxy);
        }
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build reqwest client: {}", e))
}

/// 发起网络连通性探测并返回往返延迟 RTT（毫秒）
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
        .map(|e| format!("网络探测失败: {}", e))
        .unwrap_or_else(|| "网络探测失败: 未知错误".to_string()))
}
