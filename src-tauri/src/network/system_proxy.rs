use crate::settings::ProxyProtocol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedProxy {
    pub host: String,
    pub port: u16,
    pub protocol: ProxyProtocol,
}

/// 解析 Windows 注册表中的 ProxyServer 字符串
/// 支持单代理格式如 "127.0.0.1:7890" 或多协议分流格式如 "http=127.0.0.1:7890;https=127.0.0.1:7890;socks=127.0.0.1:7891"
pub fn parse_windows_proxy_string(proxy_str: &str) -> Option<DetectedProxy> {
    let trimmed = proxy_str.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains(';') || trimmed.contains('=') {
        let parts = trimmed.split(';');
        let mut https_proxy = None;
        let mut http_proxy = None;
        let mut socks_proxy = None;

        for part in parts {
            let part = part.trim();
            if let Some((scheme, addr)) = part.split_once('=') {
                let scheme = scheme.trim().to_lowercase();
                let addr = addr.trim();
                match scheme.as_str() {
                    "https" => https_proxy = Some(addr),
                    "http" => http_proxy = Some(addr),
                    "socks" => socks_proxy = Some(addr),
                    _ => {}
                }
            }
        }

        // 优先次序：https -> http -> socks
        if let Some(addr) = https_proxy {
            if let Some(detected) = parse_host_port(addr, ProxyProtocol::Http) {
                return Some(detected);
            }
        }
        if let Some(addr) = http_proxy {
            if let Some(detected) = parse_host_port(addr, ProxyProtocol::Http) {
                return Some(detected);
            }
        }
        if let Some(addr) = socks_proxy {
            if let Some(detected) = parse_host_port(addr, ProxyProtocol::Socks5) {
                return Some(detected);
            }
        }

        return None;
    }

    parse_single_proxy_string(trimmed)
}

fn extract_protocol_and_addr(s: &str) -> (ProxyProtocol, &str) {
    let s = s.trim();
    if let Some(rest) = s
        .strip_prefix("socks5h://")
        .or_else(|| s.strip_prefix("socks5://"))
        .or_else(|| s.strip_prefix("socks://"))
    {
        (ProxyProtocol::Socks5, rest)
    } else if let Some(rest) = s
        .strip_prefix("http://")
        .or_else(|| s.strip_prefix("https://"))
    {
        (ProxyProtocol::Http, rest)
    } else {
        (ProxyProtocol::Http, s)
    }
}

fn parse_single_proxy_string(s: &str) -> Option<DetectedProxy> {
    let (protocol, rest) = extract_protocol_and_addr(s);
    parse_host_port(rest, protocol)
}

fn parse_host_port(addr: &str, protocol: ProxyProtocol) -> Option<DetectedProxy> {
    let (_, addr) = extract_protocol_and_addr(addr);
    let addr = addr.trim_end_matches('/');
    if let Some((host, port_str)) = addr.rsplit_once(':') {
        let port: u16 = port_str.parse().ok()?;
        let host = host.trim_matches(|c| c == '[' || c == ']').to_string();
        if host.is_empty() {
            return None;
        }
        Some(DetectedProxy {
            host,
            port,
            protocol,
        })
    } else {
        None
    }
}

/// 解析环境变量中的代理 URL，例如 "http://127.0.0.1:7890" 或 "socks5://user:pass@127.0.0.1:1080"
pub fn parse_url_proxy(url_str: &str) -> Option<DetectedProxy> {
    let trimmed = url_str.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (protocol, rest) = extract_protocol_and_addr(trimmed);
    let without_auth = if let Some((_auth, host_port)) = rest.rsplit_once('@') {
        host_port
    } else {
        rest
    };

    parse_host_port(without_auth, protocol)
}

#[cfg(target_os = "windows")]
pub fn get_system_proxy() -> Option<DetectedProxy> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let internet_settings = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;

    let proxy_enable: u32 = internet_settings.get_value("ProxyEnable").ok()?;
    if proxy_enable != 1 {
        return None;
    }

    let proxy_server: String = internet_settings.get_value("ProxyServer").ok()?;
    parse_windows_proxy_string(&proxy_server)
}

#[cfg(not(target_os = "windows"))]
pub fn get_system_proxy() -> Option<DetectedProxy> {
    // macOS / Linux 编译存根：读取 http_proxy / all_proxy 环境变量
    std::env::var("all_proxy")
        .or_else(|_| std::env::var("ALL_PROXY"))
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTPS_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .ok()
        .and_then(|url| parse_url_proxy(&url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_proxy_string() {
        let result = parse_windows_proxy_string("127.0.0.1:7890").expect("Should parse");
        assert_eq!(
            result,
            DetectedProxy {
                host: "127.0.0.1".to_string(),
                port: 7890,
                protocol: ProxyProtocol::Http,
            }
        );

        let result_url =
            parse_windows_proxy_string("http://192.168.1.100:8080/").expect("Should parse URL");
        assert_eq!(
            result_url,
            DetectedProxy {
                host: "192.168.1.100".to_string(),
                port: 8080,
                protocol: ProxyProtocol::Http,
            }
        );

        let result_socks =
            parse_windows_proxy_string("socks5://127.0.0.1:1080").expect("Should parse socks5");
        assert_eq!(
            result_socks,
            DetectedProxy {
                host: "127.0.0.1".to_string(),
                port: 1080,
                protocol: ProxyProtocol::Socks5,
            }
        );
    }

    #[test]
    fn test_parse_multi_protocol_proxy_string() {
        let multi = "http=127.0.0.1:7890;https=127.0.0.1:7890;socks=127.0.0.1:7891";
        let result = parse_windows_proxy_string(multi).expect("Should parse multi protocol");
        // Prioritizes https
        assert_eq!(
            result,
            DetectedProxy {
                host: "127.0.0.1".to_string(),
                port: 7890,
                protocol: ProxyProtocol::Http,
            }
        );

        let only_socks = "socks=127.0.0.1:1080;ftp=127.0.0.1:21";
        let result_socks =
            parse_windows_proxy_string(only_socks).expect("Should parse socks fallback");
        assert_eq!(
            result_socks,
            DetectedProxy {
                host: "127.0.0.1".to_string(),
                port: 1080,
                protocol: ProxyProtocol::Socks5,
            }
        );

        let multi_with_schemes = "http=http://127.0.0.1:7890;https=https://127.0.0.1:7890";
        let result_schemes = parse_windows_proxy_string(multi_with_schemes)
            .expect("Should parse multi with schemes");
        assert_eq!(
            result_schemes,
            DetectedProxy {
                host: "127.0.0.1".to_string(),
                port: 7890,
                protocol: ProxyProtocol::Http,
            }
        );
    }

    #[test]
    fn test_parse_url_proxy() {
        let env_http = "http://proxy.corp.net:8888";
        let res1 = parse_url_proxy(env_http).expect("Should parse http");
        assert_eq!(
            res1,
            DetectedProxy {
                host: "proxy.corp.net".to_string(),
                port: 8888,
                protocol: ProxyProtocol::Http,
            }
        );

        let env_auth = "socks5://alice:secret@10.0.0.1:1080";
        let res2 = parse_url_proxy(env_auth).expect("Should parse socks with auth");
        assert_eq!(
            res2,
            DetectedProxy {
                host: "10.0.0.1".to_string(),
                port: 1080,
                protocol: ProxyProtocol::Socks5,
            }
        );

        assert!(parse_url_proxy("").is_none());
        assert!(parse_url_proxy("invalid_no_port").is_none());
    }

    #[test]
    fn test_invalid_proxy_string() {
        assert!(parse_windows_proxy_string("").is_none());
        assert!(parse_windows_proxy_string("   ").is_none());
        assert!(parse_windows_proxy_string("no_port_here").is_none());
        assert!(parse_windows_proxy_string("127.0.0.1:99999").is_none());
    }
}
