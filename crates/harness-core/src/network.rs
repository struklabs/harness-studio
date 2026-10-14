//! Network exposure rules for llama-server instances: bind address, API key, CORS origins,
//! plus LAN helpers (interface addresses, strong key generator).
//!
//! Every launch path (Dashboard, saved profiles, session restore, agent tuning, router mode)
//! goes through [`prepare_launch`] in `orchestrator::launch`, and `ProcessManager::build_args`
//! re-checks the result with [`check_exposure`], so there is exactly one rule set:
//!
//! * loopback bind (`127.0.0.0/8`, `::1`, `localhost`): always allowed;
//! * network bind (`0.0.0.0`, `::`, a LAN address) with an API key: allowed;
//! * network bind without a key while built-in tools / MCP are on: always rejected;
//! * network bind without a key otherwise: rejected unless the user explicitly confirmed
//!   "open anyway" for that server (persisted in `settings.network.exposure_acks`).
//!
//! Errors are i18n keys (`network.*`) so every front end can localize them.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::config::AppSettings;
use crate::process::LaunchConfig;

/// The bind address is not an IP address (or `localhost`).
pub const ERR_INVALID_HOST: &str = "network.invalid_host";
/// Network bind without an API key and without the user's "open anyway" confirmation.
pub const ERR_LAN_WITHOUT_KEY: &str = "network.lan_without_key";
/// Network bind without an API key while built-in tools / MCP are enabled (never allowed).
pub const ERR_TOOLS_WITHOUT_KEY: &str = "network.tools_without_key";
/// A CORS origin is not `scheme://host[:port]`, `localhost` or `*`.
pub const ERR_INVALID_ORIGIN: &str = "network.invalid_origin";
/// Generating or storing a key failed (the OS random source or the keychain).
pub const ERR_KEY_GENERATION: &str = "network.key_generation_failed";

pub const DEFAULT_BIND_HOST: &str = "127.0.0.1";
pub const LAN_BIND_HOST: &str = "0.0.0.0";
/// `--cors-origins` value for a network bind without user origins. It can never equal a
/// browser `Origin` (those always carry a scheme), so no web page gets CORS access: builds that
/// match origins find no match, builds that echo the value send an origin no browser accepts.
pub const CORS_DENY_ALL: &str = "none";
/// Origins of the Harness window (Tauri on Windows / macOS / Linux, and the dev server).
pub const APP_ORIGINS: [&str; 4] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost:1420",
];

/// `settings.json` → `network`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkSettings {
    /// Browser origins allowed to call the servers (`--cors-origins`). Empty = the default
    /// described in [`cors_arg`].
    pub cors_origins: Vec<String>,
    /// Server id → the user's "open anyway" confirmation for a network bind without a key.
    pub exposure_acks: BTreeMap<String, ExposureAck>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExposureAck {
    /// Unix seconds.
    pub acknowledged_at: u64,
    /// Bind address the user saw when confirming (informational).
    pub host: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum BindScope {
    /// Reachable from this computer only.
    Loopback,
    /// Reachable from other devices (all interfaces or a specific non-loopback address).
    Lan,
}

/// Outcome of an allowed launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exposure {
    Loopback,
    LanWithKey,
    /// Network bind without a key, confirmed by the user ("open anyway").
    LanAcknowledged,
}

/// Canonical bind address: an IPv4/IPv6 literal (brackets removed) or `localhost` mapped to
/// `127.0.0.1`. Host names, comma lists (llama-server binds every listed address) and UNIX
/// socket paths are rejected, so a second address can't slip past the checks.
pub fn normalize_bind_host(raw: &str) -> Result<String, String> {
    let host = raw.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(DEFAULT_BIND_HOST.to_string());
    }
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse::<IpAddr>()
        .map(|ip| ip.to_string())
        .map_err(|_| ERR_INVALID_HOST.to_string())
}

fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

/// `true` only for addresses that are reachable from this computer alone. Anything that
/// doesn't parse counts as exposed.
pub fn is_loopback_host(host: &str) -> bool {
    normalize_bind_host(host)
        .ok()
        .and_then(|h| h.parse::<IpAddr>().ok())
        .is_some_and(is_loopback_ip)
}

pub fn bind_scope(host: &str) -> Result<BindScope, String> {
    let ip: IpAddr = normalize_bind_host(host)?
        .parse()
        .map_err(|_| ERR_INVALID_HOST.to_string())?;
    Ok(if is_loopback_ip(ip) {
        BindScope::Loopback
    } else {
        BindScope::Lan
    })
}

/// The single exposure rule (see the module docs).
pub fn check_exposure(
    host: &str,
    api_key_set: bool,
    uses_tools: bool,
    acknowledged: bool,
) -> Result<Exposure, String> {
    match bind_scope(host)? {
        BindScope::Loopback => Ok(Exposure::Loopback),
        BindScope::Lan if api_key_set => Ok(Exposure::LanWithKey),
        BindScope::Lan if uses_tools => Err(ERR_TOOLS_WITHOUT_KEY.to_string()),
        BindScope::Lan if acknowledged => Ok(Exposure::LanAcknowledged),
        BindScope::Lan => Err(ERR_LAN_WITHOUT_KEY.to_string()),
    }
}

/// Checks a launch config as `ProcessManager` will run it (`api_key` = the resolved key).
pub fn check_launch(config: &LaunchConfig) -> Result<Exposure, String> {
    check_exposure(
        &config.host,
        config
            .api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty()),
        config.mcp_enabled,
        config.lan_exposure_ack,
    )
}

/// One CORS origin: `*`, the llama-server special value `localhost`, or
/// `http(s)://host[:port]` (a trailing slash is dropped; paths, commas and spaces are not
/// allowed because the list is passed comma-joined on the command line).
pub fn normalize_origin(raw: &str) -> Result<String, String> {
    let origin = raw.trim().trim_end_matches('/');
    if origin == "*" || origin.eq_ignore_ascii_case("localhost") {
        return Ok(origin.to_ascii_lowercase());
    }
    let invalid = || ERR_INVALID_ORIGIN.to_string();
    let lower = origin.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
        .ok_or_else(invalid)?;
    let ok_chars = rest
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
    if rest.is_empty() || !ok_chars || rest.starts_with(':') {
        return Err(invalid());
    }
    Ok(lower)
}

/// Normalized, de-duplicated origin list (blank lines dropped).
pub fn normalize_origins(list: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for raw in list.iter().filter(|s| !s.trim().is_empty()) {
        let origin = normalize_origin(raw)?;
        if !out.contains(&origin) {
            out.push(origin);
        }
    }
    Ok(out)
}

/// `--cors-origins` value for a launch (`None` = don't pass the flag).
///
/// * user origins set: exactly those;
/// * loopback bind without user origins: llama-server's own default (`*`; it narrows to
///   `localhost` by itself when tools/MCP are on);
/// * network bind without user origins: [`CORS_DENY_ALL`], i.e. no browser page may call it.
pub fn cors_arg(origins: &[String], scope: BindScope) -> Option<String> {
    let origins: Vec<&str> = origins
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !origins.is_empty() {
        return Some(origins.join(","));
    }
    match scope {
        BindScope::Loopback => None,
        BindScope::Lan => Some(CORS_DENY_ALL.to_string()),
    }
}

/// Browser origins a Python worker answers (comma-joined, for its environment): the Harness
/// window and the origins of `cors` (a launch's resolved `--cors-origins` value), in the form a
/// browser sends (`scheme://host[:port]`, lowercase, no trailing slash), since the worker
/// compares them exactly. `*`, `localhost`, [`CORS_DENY_ALL`], wildcards and entries with a
/// path add nothing: a worker never opens to every page.
pub fn worker_origins(cors: Option<&str>) -> String {
    let mut out: Vec<String> = APP_ORIGINS.iter().map(|o| o.to_string()).collect();
    for raw in cors.unwrap_or("").split(',') {
        let origin = raw.trim().trim_end_matches('/').to_ascii_lowercase();
        let Some(rest) = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
        else {
            continue;
        };
        let host_ok = !rest.is_empty()
            && !rest.starts_with(':')
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
        if host_ok && !out.contains(&origin) {
            out.push(origin);
        }
    }
    out.join(",")
}

/// Fills in the network part of a launch and applies the exposure rule. Called by
/// `orchestrator::launch` for every start path, after the API key was resolved.
///
/// * an empty host takes the default bind address (`settings.server.host`, which Settings >
///   Network & security edits); a profile's own host always wins;
/// * the host is canonicalized (see [`normalize_bind_host`]);
/// * `cors_origins` and `lan_exposure_ack` are always set here, never taken from the caller.
pub fn prepare_launch(
    config: &mut LaunchConfig,
    settings: &AppSettings,
    server_id: &str,
) -> Result<Exposure, String> {
    if config.host.trim().is_empty() {
        config.host = settings.server.host.clone();
    }
    if config.host.trim().is_empty() {
        config.host = DEFAULT_BIND_HOST.to_string();
    }
    config.host = normalize_bind_host(&config.host)?;
    let scope = bind_scope(&config.host)?;
    config.lan_exposure_ack = settings.network.exposure_acks.contains_key(server_id);
    config.cors_origins = cors_arg(&settings.network.cors_origins, scope);
    check_launch(config)
}

/// Validates the network part of settings coming from the frontend. The acknowledgements
/// are only changed through [`acknowledge`] / [`revoke`], so the stored ones are kept.
pub fn sanitize_settings(incoming: &mut AppSettings, stored: &AppSettings) -> Result<(), String> {
    incoming.server.host = normalize_bind_host(&incoming.server.host)?;
    incoming.network.cors_origins = normalize_origins(&incoming.network.cors_origins)?;
    incoming.network.exposure_acks = stored.network.exposure_acks.clone();
    // The Control API block changes only through its own validated command.
    incoming.control_api = stored.control_api.clone();
    // So does the gateway block.
    incoming.gateway = stored.gateway.clone();
    // Experimental engine consent changes only through its dedicated desktop command.
    incoming.audio_cpp_enabled = stored.audio_cpp_enabled;
    // And the OpenTelemetry block (its header values live in the keychain).
    incoming.otel = stored.otel.clone();
    Ok(())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Records "open anyway" for `server_id` (host = the bind address the user saw).
pub fn acknowledge(server_id: &str, host: &str) -> Result<AppSettings, String> {
    crate::servers::validate_id(server_id)?;
    let mut settings = AppSettings::load();
    settings.network.exposure_acks.insert(
        server_id.to_string(),
        ExposureAck {
            acknowledged_at: now_secs(),
            host: normalize_bind_host(host).unwrap_or_else(|_| host.trim().to_string()),
        },
    );
    settings.save()?;
    Ok(settings)
}

pub fn revoke(server_id: &str) -> Result<AppSettings, String> {
    let mut settings = AppSettings::load();
    if settings.network.exposure_acks.remove(server_id).is_some() {
        settings.save()?;
    }
    Ok(settings)
}

// --- Strong API keys ---

/// Number of random bytes in a generated key (256 bits).
pub const API_KEY_BYTES: usize = 32;

/// Maximum redirects followed by user-controlled network tools.
pub const PUBLIC_HTTP_REDIRECT_LIMIT: usize = 5;

/// True for addresses a user-controlled URL must never reach. This includes loopback,
/// RFC1918/ULA, link-local (including cloud metadata), unspecified and multicast ranges.
pub fn is_private_network_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                // 0.0.0.0/8 ("this network"), CGNAT 100.64/10, benchmarking 198.18/15.
                || ip.octets()[0] == 0
                || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xC0) == 64)
                || (ip.octets()[0] == 198 && (ip.octets()[1] & 0xFE) == 18)
        }
        IpAddr::V6(ip) => {
            if let Some(ipv4) = ip.to_ipv4_mapped() {
                return is_private_network_ip(IpAddr::V4(ipv4));
            }
            let seg = ip.segments();
            // IPv4-compatible (::a.b.c.d) and NAT64 (64:ff9b::/96) embed an IPv4 address.
            if seg[..6] == [0, 0, 0, 0, 0, 0] || seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                let v4 = Ipv4Addr::new(
                    (seg[6] >> 8) as u8,
                    seg[6] as u8,
                    (seg[7] >> 8) as u8,
                    seg[7] as u8,
                );
                return is_private_network_ip(IpAddr::V4(v4));
            }
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_multicast()
        }
    }
}

/// Parses a model/user supplied HTTP URL. Credentials and non-HTTP schemes are refused.
pub fn parse_public_http_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only http and https URLs are allowed".into());
    }
    if url.host_str().is_none() {
        return Err("the URL has no host".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("credentials in URLs are not allowed".into());
    }
    Ok(url)
}

/// The socket addresses of a URL's host: an IP literal as is, a domain through DNS.
/// `host_str()` keeps the brackets of an IPv6 literal (`[::1]`), which the system resolver on
/// Linux rejects, so literals must not go to the resolver.
pub async fn lookup_url_host(url: &reqwest::Url, port: u16) -> std::io::Result<Vec<SocketAddr>> {
    let host = url.host_str().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "the URL has no host")
    })?;
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    Ok(tokio::net::lookup_host((host, port)).await?.collect())
}

/// Resolves every address for a URL and rejects the whole host if any answer is not public.
/// Returning the addresses lets callers pin reqwest to the checked DNS answers, closing the
/// check/use gap that would otherwise permit DNS rebinding.
pub async fn resolve_public_http_url(url: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
    let host = url.host_str().ok_or("the URL has no host")?;
    let port = url
        .port_or_known_default()
        .ok_or("the URL has no known port")?;
    let addresses = lookup_url_host(url, port)
        .await
        .map_err(|e| crate::http_error::dns(&e, url.as_str()))?;
    if addresses.is_empty() {
        return Err(format!("{host} did not resolve to an address"));
    }
    if addresses
        .iter()
        .any(|address| is_private_network_ip(address.ip()))
    {
        return Err(format!("refused private or local network target: {host}"));
    }
    Ok(addresses)
}

/// Resolves a redirect relative to the current URL and applies the same structural rules.
pub fn public_redirect_target(
    current: &reqwest::Url,
    location: &str,
) -> Result<reqwest::Url, String> {
    let target = current
        .join(location)
        .map_err(|e| format!("invalid redirect URL {location:?}: {e}"))?;
    parse_public_http_url(target.as_str())
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
        }
    }
    out
}

/// A fresh key: 32 bytes from the OS random source, base64url without padding (43 chars).
pub fn generate_api_key() -> Result<String, String> {
    let mut buf = [0u8; API_KEY_BYTES];
    getrandom::fill(&mut buf).map_err(|e| format!("{}: {}", ERR_KEY_GENERATION, e))?;
    Ok(base64url(&buf))
}

/// Generates a key for a saved server profile and stores it in the keychain (the profile only
/// keeps the reference). Returns the updated profile list.
pub fn generate_profile_key(
    profile_id: &str,
) -> Result<Vec<crate::servers::ServerProfile>, String> {
    let mut profile = crate::servers::load_profiles()
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| {
            String::from(crate::error_key::err("servers.profile_not_found").with("id", profile_id))
        })?;
    profile.config.api_key = Some(generate_api_key()?);
    crate::servers::save_profile(profile)
}

// --- LAN addresses ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanAddress {
    pub ip: String,
    /// Interface name as the OS shows it ("Wi-Fi", "Ethernet", "en0").
    pub interface: String,
}

/// Addresses another device on the local network could use: not loopback, link-local,
/// unspecified, broadcast, multicast or documentation ranges.
pub fn is_lan_candidate(ip: Ipv4Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation())
}

/// Interfaces of virtual machines / containers / host-only networks (not reachable from
/// other devices). Matched on the interface name or adapter description.
pub fn looks_virtual(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "vethernet",
        "hyper-v",
        "virtualbox",
        "vboxnet",
        "vmware",
        "vmnet",
        "wsl",
        "docker",
        "veth",
        "virbr",
        "loopback",
        "npcap",
        "tap-windows",
        "host-only",
    ];
    MARKERS.iter().any(|m| n.contains(m)) || n.starts_with("br-") || n == "lo" || n == "lo0"
}

/// IPv4 addresses of this computer's active, non-virtual interfaces (best effort; empty when
/// the OS call fails).
pub fn lan_ipv4_addresses() -> Vec<LanAddress> {
    let mut out: Vec<LanAddress> = platform_ipv4_interfaces()
        .into_iter()
        .filter(|(ip, name, desc)| {
            is_lan_candidate(*ip) && !looks_virtual(name) && !looks_virtual(desc)
        })
        .map(|(ip, name, _)| LanAddress {
            ip: ip.to_string(),
            interface: name,
        })
        .collect();
    out.dedup_by(|a, b| a.ip == b.ip);
    out
}

/// `(address, interface name, description)` of every up interface with an IPv4 address.
#[cfg(windows)]
fn platform_ipv4_interfaces() -> Vec<(Ipv4Addr, String, String)> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::AF_INET;

    const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
    const IF_TYPE_TUNNEL: u32 = 131;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buf: Vec<u64> = Vec::new();
    let mut result = ERROR_BUFFER_OVERFLOW;
    for _ in 0..3 {
        // u64 elements keep the buffer 8-byte aligned for the adapter structs.
        buf = vec![0u64; (size as usize).div_ceil(8)];
        result = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            )
        };
        if result != ERROR_BUFFER_OVERFLOW {
            break;
        }
    }
    if result != 0 {
        return Vec::new();
    }

    let wide = |p: windows::core::PWSTR| -> String {
        if p.is_null() {
            String::new()
        } else {
            unsafe { p.to_string() }.unwrap_or_default()
        }
    };
    let mut out = Vec::new();
    let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        let a = unsafe { &*adapter };
        adapter = a.Next;
        if a.OperStatus != IfOperStatusUp
            || a.IfType == IF_TYPE_SOFTWARE_LOOPBACK
            || a.IfType == IF_TYPE_TUNNEL
        {
            continue;
        }
        let name = wide(a.FriendlyName);
        let desc = wide(a.Description);
        let mut unicast = a.FirstUnicastAddress;
        while !unicast.is_null() {
            let u = unsafe { &*unicast };
            unicast = u.Next;
            let sa = u.Address.lpSockaddr;
            if sa.is_null() || u.Address.iSockaddrLength < 8 {
                continue;
            }
            if unsafe { (*sa).sa_family } != AF_INET {
                continue;
            }
            // SOCKADDR_IN: family (2) + port (2) + address (4)
            let bytes = unsafe { std::slice::from_raw_parts(sa as *const u8, 8) };
            let ip = Ipv4Addr::new(bytes[4], bytes[5], bytes[6], bytes[7]);
            out.push((ip, name.clone(), desc.clone()));
        }
    }
    out
}

#[cfg(unix)]
fn platform_ipv4_interfaces() -> Vec<(Ipv4Addr, String, String)> {
    let mut out = Vec::new();
    let mut addrs: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut addrs) } != 0 {
        return out;
    }
    let mut cur = addrs;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        let up = ifa.ifa_flags & (libc::IFF_UP as u32) != 0;
        let loopback = ifa.ifa_flags & (libc::IFF_LOOPBACK as u32) != 0;
        if !up || loopback || ifa.ifa_addr.is_null() {
            continue;
        }
        if unsafe { (*ifa.ifa_addr).sa_family } as i32 != libc::AF_INET {
            continue;
        }
        let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
        let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
        let name = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) }
            .to_string_lossy()
            .to_string();
        out.push((ip, name, String::new()));
    }
    unsafe { libc::freeifaddrs(addrs) };
    out
}

#[cfg(not(any(windows, unix)))]
fn platform_ipv4_interfaces() -> Vec<(Ipv4Addr, String, String)> {
    Vec::new()
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
