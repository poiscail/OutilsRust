//! UPnP (ouvrir un port sur la box) et STUN (adresse publique) : communs à Windows et Linux.
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).user_agent("Outils").build()
}

/// Valeur de l'en-tête LOCATION d'une réponse SSDP.
pub fn location_ssdp(r: &str) -> Option<String> {
    r.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("location").then(|| v.trim().to_string())
    })
}

/// (type de service, URL de contrôle) de la connexion WAN dans la description de la box.
pub fn service_wan(xml: &str, base: &str) -> Option<(String, String)> {
    let re = regex::Regex::new(r"(?s)<service>(.*?)</service>").unwrap();
    let re_type = regex::Regex::new(r"<serviceType>(.*?)</serviceType>").unwrap();
    let re_ctrl = regex::Regex::new(r"<controlURL>(.*?)</controlURL>").unwrap();
    for c in re.captures_iter(xml) {
        let s = &c[1];
        let typ = re_type.captures(s)?[1].trim().to_string();
        if typ.contains("WANIPConnection") || typ.contains("WANPPPConnection") {
            let ctrl = re_ctrl.captures(s)?[1].trim().to_string();
            let url = if ctrl.starts_with("http") { ctrl } else { format!("{}/{}", base.trim_end_matches('/'), ctrl.trim_start_matches('/')) };
            return Some((typ, url));
        }
    }
    None
}

pub fn trouver_box() -> Result<(String, String), String> {
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok();
    let req = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
    s.send_to(req.as_bytes(), "239.255.255.250:1900").map_err(|e| e.to_string())?;
    let mut b = [0u8; 2048];
    let (n, _) = s.recv_from(&mut b).map_err(|_| "aucune box ne répond en UPnP (option désactivée dans la box ?)".to_string())?;
    let loc = location_ssdp(&String::from_utf8_lossy(&b[..n])).ok_or("réponse UPnP sans adresse")?;
    let xml = agent().get(&loc).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let base = loc.split('/').take(3).collect::<Vec<_>>().join("/");
    service_wan(&xml, &base).ok_or_else(|| "la box ne propose pas l'ouverture de ports".into())
}

pub fn soap(service: &(String, String), action: &str, args: &[(&str, String)]) -> Result<String, String> {
    let corps: String = args.iter().map(|(k, v)| format!("<{k}>{v}</{k}>")).collect();
    let xml = format!(r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{}">{corps}</u:{action}></s:Body></s:Envelope>"#, service.0);
    agent().post(&service.1).set("Content-Type", "text/xml; charset=\"utf-8\"").set("SOAPAction", &format!("\"{}#{action}\"", service.0)).send_string(&xml)
        .map_err(|e| match e {
            ureq::Error::Status(c, r) => format!("la box a refusé ({c}) : {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>()),
            x => x.to_string(),
        })?.into_string().map_err(|e| e.to_string())
}

/// Requête STUN « binding » (RFC 5389).
pub fn requete_stun(id: [u8; 12]) -> Vec<u8> {
    let mut v = vec![0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42];
    v.extend(id);
    v
}

/// Adresse publique vue par le serveur STUN (XOR-MAPPED-ADDRESS, ou MAPPED-ADDRESS).
pub fn reponse_stun(b: &[u8]) -> Option<SocketAddr> {
    if b.len() < 20 || b[0..2] != [0x01, 0x01] {
        return None;
    }
    let mut i = 20;
    while i + 4 <= b.len() {
        let t = u16::from_be_bytes([b[i], b[i + 1]]);
        let l = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let v = b.get(i + 4..i + 4 + l)?;
        if (t == 0x0020 || t == 0x0001) && l >= 8 && v[1] == 0x01 {
            let mut port = u16::from_be_bytes([v[2], v[3]]);
            let mut ip = [v[4], v[5], v[6], v[7]];
            if t == 0x0020 {
                port ^= 0x2112;
                for (k, x) in ip.iter_mut().enumerate() {
                    *x ^= [0x21, 0x12, 0xA4, 0x42][k];
                }
            }
            return Some(SocketAddr::from((Ipv4Addr::from(ip), port)));
        }
        i += 4 + l.div_ceil(4) * 4;
    }
    None
}


/// Ouvre un port sur la box (UPnP) vers ce PC.
pub fn ouvrir_port(port: u16, proto: &str, description: &str) -> Result<(), String> {
    let s = trouver_box()?;
    let ip = crate::telephone::ip_locale().ok_or("adresse locale introuvable")?;
    soap(&s, "AddPortMapping", &[("NewRemoteHost", String::new()), ("NewExternalPort", port.to_string()), ("NewProtocol", proto.into()), ("NewInternalPort", port.to_string()), ("NewInternalClient", ip), ("NewEnabled", "1".into()), ("NewPortMappingDescription", description.into()), ("NewLeaseDuration", "0".into())]).map(|_| ())
}

/// Adresse IP publique de la maison (par un serveur STUN, sans site web).
pub fn ip_publique() -> Option<String> {
    use std::net::ToSocketAddrs;
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok();
    for serveur in ["stun.l.google.com:19302", "stun.cloudflare.com:3478"] {
        let Some(a) = serveur.to_socket_addrs().ok().and_then(|mut x| x.find(|a| a.is_ipv4())) else { continue };
        let mut id = [0u8; 12];
        let _ = getrandom::getrandom(&mut id);
        let _ = s.send_to(&requete_stun(id), a);
        let mut b = [0u8; 512];
        if let Ok((n, _)) = s.recv_from(&mut b) {
            if let Some(x) = reponse_stun(&b[..n]) {
                return Some(x.ip().to_string());
            }
        }
    }
    None
}
