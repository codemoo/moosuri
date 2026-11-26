//! Deterministic unit, lifecycle-profile and nginx rendering. Templates are
//! compiled in from `deployment/kiri/setup`; no value can add a directive.
use super::host::Result;
use serde_json::json;
use std::path::PathBuf;

pub const GATEWAY_ADAPTER: &str = "127.0.0.1:3211";
pub const VAULT_BIND: &str = "127.0.0.1:3220";
pub const CONTROL: &str = "/var/lib/moosuri-control";
pub const STORAGE: &str = "/srv/moosuri-storage";
pub const ETC: &str = "/etc/moosuri";
pub const ACME_ROOT: &str = "/var/lib/moosuri-acme";
pub const NGINX_CONF: &str = "/etc/nginx/conf.d/moosuri-kiri.conf";
pub const SYSTEMD: &str = "/etc/systemd/system";

pub const UNITS: [(&str, &str); 9] = [
    (
        "moosuri-kiri-gateway.service",
        include_str!("../../../../../deployment/kiri/setup/moosuri-kiri-gateway.service.in"),
    ),
    (
        "moosuri-kiri-vault.service",
        include_str!("../../../../../deployment/kiri/setup/moosuri-kiri-vault.service.in"),
    ),
    (
        "moosuri-kiri-lifecycle.service",
        include_str!("../../../../../deployment/kiri/setup/moosuri-kiri-lifecycle.service.in"),
    ),
    (
        "moosuri-kiri-onboard.service",
        include_str!("../../../../../deployment/kiri/setup/moosuri-kiri-onboard.service.in"),
    ),
    (
        "moosuri-kiri-onboard.timer",
        include_str!("../../../../../deployment/kiri/setup/moosuri-kiri-onboard.timer.in"),
    ),
    (
        "moosuri-kiri-gws.service",
        include_str!("../../../../../deployment/kiri/setup/moosuri-kiri-gws.service.in"),
    ),
    (
        "moosuri-kiri-browser.service",
        include_str!("../../../../../deployment/kiri/setup/moosuri-kiri-browser.service.in"),
    ),
    (
        "moosuri-google-browser.socket",
        include_str!("../../../../../deployment/kiri/setup/moosuri-google-browser.socket.in"),
    ),
    (
        "moosuri-google-browser@.service",
        include_str!("../../../../../deployment/kiri/setup/moosuri-google-browser@.service.in"),
    ),
];

/// A public HTTPS origin with a DNS name and the default port only.
pub fn host_of(origin: &str) -> Result<String> {
    let url = url::Url::parse(origin).map_err(|_| "kiri_install_origin_invalid")?;
    let host = url.host_str().unwrap_or("").to_owned();
    if url.scheme() != "https"
        || url.port().is_some()
        || origin.len() > 253
        || url.origin().ascii_serialization() != origin
        || !matches!(url.host(), Some(url::Host::Domain(_)))
        || !host.contains('.')
        || host.starts_with(['.', '-'])
        || host.ends_with(['.', '-'])
        || host.ends_with(".local")
        || !host
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return Err("kiri_install_origin_invalid".into());
    }
    Ok(host)
}
/// Assistant and vault origins must be distinct browser origins on distinct hosts.
pub fn origins(public: &str, vault: &str) -> Result<(String, String)> {
    let (a, b) = (host_of(public)?, host_of(vault)?);
    if a == b {
        return Err("kiri_install_vault_origin_must_differ".into());
    }
    Ok((a, b))
}

pub struct Values<'a> {
    pub public_origin: &'a str,
    pub vault_origin: &'a str,
    pub installation_id: &'a str,
    pub proxy_gid: u32,
}
pub fn unit(template: &str, v: &Values) -> Result<String> {
    for s in [v.public_origin, v.vault_origin] {
        host_of(s)?;
    }
    let text = template
        .replace("@PUBLIC_ORIGIN@", v.public_origin)
        .replace("@VAULT_ORIGIN@", v.vault_origin)
        .replace("@INSTALLATION_ID@", v.installation_id)
        .replace("@PROXY_GID@", &v.proxy_gid.to_string())
        .replace("@GATEWAY_ADAPTER@", GATEWAY_ADAPTER)
        .replace("@VAULT_BIND@", VAULT_BIND);
    // Any remaining @UPPERCASE@ token is an unresolved template value.
    let bytes = text.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'@' {
            let rest = &bytes[i + 1..];
            let n = rest
                .iter()
                .take_while(|c| c.is_ascii_uppercase() || **c == b'_')
                .count();
            if n > 0 && rest.get(n) == Some(&b'@') {
                return Err("kiri_install_template_unresolved".into());
            }
        }
    }
    Ok(text)
}

pub struct Profile<'a> {
    pub public_origin: &'a str,
    pub vault_origin: &'a str,
    pub member_image: &'a str,
    pub volume_bytes: u64,
    pub uid_pool: &'a [u32],
    pub capacity: u32,
}
/// The existing lifecycle profile consumed by recovery, onboarding and both relays.
pub fn lifecycle(p: &Profile) -> Vec<u8> {
    let mut text = serde_json::to_vec_pretty(&json!({
        "version": 1,
        "registry_directory": format!("{CONTROL}/registry"),
        "mounts_root": format!("{STORAGE}/mounts"),
        "ipc_root": format!("{STORAGE}/ipc"),
        "allocations_root": format!("{STORAGE}/assignments"),
        "google_sockets_root": "/run/moosuri-kiri-google",
        "public_origin": p.public_origin,
        "vault_origin": p.vault_origin,
        "browser_sockets_root": "/var/lib/moosuri-kiri-browser",
        "image": p.member_image,
        "engine_socket": "/run/kiri/docker.sock",
        "max_volume_bytes": p.volume_bytes,
        "enrollment_directory": format!("{CONTROL}/enrolled"),
        "members": [],
    }))
    .unwrap_or_default();
    text.push(b'\n');
    text
}
pub fn policy(p: &Profile) -> Vec<u8> {
    let mut text = serde_json::to_vec_pretty(&json!({
        "uid_pool": p.uid_pool,
        "max_total_volume_bytes": p.volume_bytes.saturating_mul(u64::from(p.capacity)),
    }))
    .unwrap_or_default();
    text.push(b'\n');
    text
}
pub fn origin_env(public_origin: &str) -> Vec<u8> {
    format!("MOOSURI_PUBLIC_ORIGIN={public_origin}\n").into_bytes()
}

#[derive(Clone)]
pub enum Tls {
    /// Operator-selected paired certificate and key (tests or externally managed).
    Supplied { certificate: PathBuf, key: PathBuf },
    /// Certbot webroot certificate for exactly the two selected names.
    Certbot { name: String },
}
impl Tls {
    pub fn paths(&self) -> (String, String) {
        match self {
            Tls::Supplied { certificate, key } => (
                certificate.display().to_string(),
                key.display().to_string(),
            ),
            Tls::Certbot { name } => (
                format!("/etc/letsencrypt/live/{name}/fullchain.pem"),
                format!("/etc/letsencrypt/live/{name}/privkey.pem"),
            ),
        }
    }
}
const PROXY: &str = "        proxy_set_header Host $http_host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For \"\";
        proxy_set_header Connection \"\";
        proxy_http_version 1.1;
        proxy_buffering off;
        proxy_request_buffering off;
        client_body_buffer_size 128k;
        proxy_read_timeout 100s;
        proxy_cache off;
        access_log off;
";
fn challenge(host: &str, origin: &str) -> String {
    format!(
        "server {{
    listen 80;
    listen [::]:80;
    server_name {host};
    access_log off;
    location ^~ /.well-known/acme-challenge/ {{
        root {ACME_ROOT};
        default_type text/plain;
    }}
    location / {{
        return 308 {origin}$request_uri;
    }}
}}
"
    )
}
/// Port 80 only: used before an automatic certificate exists.
pub fn nginx_challenge(public_origin: &str, vault_origin: &str) -> Result<String> {
    let (a, b) = origins(public_origin, vault_origin)?;
    Ok(format!(
        "# Managed by moosuri setup-kiri. Dedicated names only; no URI/access logging.\n{}{}",
        challenge(&a, public_origin),
        challenge(&b, vault_origin)
    ))
}
pub fn nginx_final(public_origin: &str, vault_origin: &str, tls: &Tls) -> Result<String> {
    let (a, b) = origins(public_origin, vault_origin)?;
    let (cert, key) = tls.paths();
    if [&cert, &key]
        .iter()
        .any(|p| p.contains([' ', ';', '{', '}', '"', '\'', '$', '\n', '\\']) || !p.starts_with('/'))
    {
        return Err("kiri_install_tls_path_invalid".into());
    }
    let tls_block = |host: &str, limit: &str, upstream: &str| {
        format!(
            "server {{
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name {host};
    ssl_certificate {cert};
    ssl_certificate_key {key};
    ssl_protocols TLSv1.2 TLSv1.3;
    access_log off;
    error_log /var/log/nginx/moosuri-kiri-error.log crit;
    client_max_body_size {limit};
    location / {{
        proxy_pass {upstream};
{PROXY}    }}
}}
"
        )
    };
    let mut text = String::from(
        "# Managed by moosuri setup-kiri. Dedicated names only; no URI/access logging.\n",
    );
    text.push_str(&challenge(&a, public_origin));
    text.push_str(&challenge(&b, vault_origin));
    text.push_str(&tls_block(
        &a,
        "128k",
        "http://unix:/run/moosuri-member-proxy/gateway.sock",
    ));
    text.push_str(&tls_block(&b, "1m", &format!("http://{VAULT_BIND}")));
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins_are_strict_and_separate() {
        assert!(host_of("https://assistant.example.com").is_ok());
        for bad in [
            "http://assistant.example.com",
            "https://assistant.example.com/",
            "https://assistant.example.com:8443",
            "https://127.0.0.1",
            "https://localhost",
            "https://user@assistant.example.com",
            "https://Assistant.example.com",
            "https://a_b.example.com",
        ] {
            assert!(host_of(bad).is_err(), "{bad}");
        }
        assert!(origins("https://a.example.com", "https://a.example.com").is_err());
        assert!(origins("https://a.example.com", "https://v.example.com").is_ok());
    }
    #[test]
    fn all_templates_render_without_unresolved_values() {
        let id = "11111111-1111-4111-8111-111111111111";
        let v = Values {
            public_origin: "https://a.example.com",
            vault_origin: "https://v.example.com",
            installation_id: id,
            proxy_gid: 33,
        };
        let mut text = String::new();
        for (name, template) in UNITS {
            let t = unit(template, &v).unwrap();
            assert!(!t.contains("@PUBLIC") && !t.contains("@PROXY") && !t.contains("@INSTALL"), "{name}");
            text.push_str(&t);
        }
        for expected in [
            "--vault-all-members",
            "--vault-site-origin https://v.example.com",
            "--vault-adapter-listen 127.0.0.1:3211",
            "--vault-adapter-key-file /etc/moosuri/vault-adapter.key",
            "--google-runtime-socket /run/moosuri-google-browser/runtime.sock",
            "--google-runtime-installation 11111111-1111-4111-8111-111111111111",
        ] {
            assert!(text.contains(expected), "{expected}");
        }
        // Vault is bound to all members through the gateway, never one fixed recipient.
        assert!(!text.contains("--kiri-recipient-file"));
    }
    #[test]
    fn nginx_preserves_host_overwrites_real_ip_and_does_not_log() {
        let tls = Tls::Certbot { name: "moosuri-kiri-test".into() };
        let text = nginx_final("https://a.example.com", "https://v.example.com", &tls).unwrap();
        assert!(text.contains("proxy_set_header Host $http_host;"));
        assert!(text.contains("proxy_set_header X-Real-IP $remote_addr;"));
        assert!(text.contains("access_log off;"));
        assert!(!text.contains("default_server"));
        assert_eq!(text.matches("listen 443").count(), 2);
        let bad = Tls::Supplied { certificate: "/x/a b.pem".into(), key: "/x/k.pem".into() };
        assert!(nginx_final("https://a.example.com", "https://v.example.com", &bad).is_err());
    }
}
