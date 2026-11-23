//! Analyse réseau commune à Windows et Linux : services à risque, ports ouverts, sécurité du Wi-Fi,
//! DNS connus, usurpation de la box, pisteurs, genre d'appareil.
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// (port, service, risque 0-3, explication)
pub const PORTS: &[(u16, &str, u8, &str)] = &[
    (21, "FTP", 2, "transfert de fichiers : les mots de passe passent en clair"),
    (22, "SSH", 1, "accès à distance chiffré : bien si c'est voulu, avec un vrai mot de passe"),
    (23, "Telnet", 3, "accès à distance NON chiffré, cible n°1 des virus d'objets connectés : à couper"),
    (53, "DNS", 0, "annuaire internet (normal sur la box)"),
    (80, "Web (HTTP)", 1, "page web non chiffrée (souvent la page d'administration)"),
    (139, "Partage Windows (NetBIOS)", 2, "partage de fichiers ancien"),
    (443, "Web (HTTPS)", 0, "page web chiffrée"),
    (445, "Partage Windows (SMB)", 2, "partage de fichiers : à ne jamais ouvrir vers internet"),
    (554, "Caméra (RTSP)", 2, "flux vidéo : vérifie qu'il demande un mot de passe"),
    (1883, "MQTT", 2, "objets connectés, souvent sans mot de passe"),
    (3389, "Bureau à distance", 2, "prise en main du PC : à fermer si tu ne t'en sers pas"),
    (5000, "NAS / UPnP", 1, "page d'un NAS ou service UPnP"),
    (5900, "VNC", 3, "prise en main à distance souvent mal protégée"),
    (7547, "TR-069", 2, "gestion de la box par l'opérateur : ne devrait pas répondre depuis la maison"),
    (8008, "Chromecast", 0, "diffusion Google Cast"),
    (8080, "Web (autre)", 1, "page web non chiffrée"),
    (8443, "Web chiffré (autre)", 0, "page web chiffrée"),
    (9100, "Imprimante", 1, "impression directe : n'importe qui du réseau peut imprimer"),
    (32400, "Plex", 0, "serveur multimédia"),
    (49152, "UPnP", 1, "service UPnP"),
    (62078, "iPhone / iPad", 0, "synchronisation Apple"),
];


pub fn infos_port(p: u16) -> (&'static str, u8, &'static str) {
    PORTS.iter().find(|x| x.0 == p).map(|x| (x.1, x.2, x.3)).unwrap_or(("inconnu", 1, "service non identifié"))
}


/// Ports TCP ouverts sur une adresse (connexion simple, comme un navigateur).
pub fn ports_ouverts(ip: Ipv4Addr, ports: &[u16], attente: Duration) -> Vec<u16> {
    let ouverts = Arc::new(Mutex::new(vec![]));
    std::thread::scope(|s| {
        for morceau in ports.chunks(4) {
            let o = ouverts.clone();
            s.spawn(move || {
                for p in morceau {
                    if TcpStream::connect_timeout(&SocketAddr::from((ip, *p)), attente).is_ok() {
                        o.lock().unwrap().push(*p);
                    }
                }
            });
        }
    });
    let mut v = ouverts.lock().unwrap().clone();
    v.sort();
    v
}


pub fn liste_ports() -> Vec<u16> {
    PORTS.iter().map(|p| p.0).collect()
}


/// Note de la sécurité Wi-Fi : (points sur 30, explication).
pub fn note_wifi(auth: &str, chiffre: &str) -> (u32, String) {
    let a = auth.to_uppercase();
    if a.contains("WPA3") {
        (30, format!("Wi-Fi en {auth} : le meilleur niveau actuel."))
    } else if a.contains("WPA2") && !chiffre.to_uppercase().contains("TKIP") {
        (24, format!("Wi-Fi en {auth} ({chiffre}) : bien. Passe en WPA3 (ou WPA2/WPA3) dans la box si tous tes appareils le supportent."))
    } else if a.contains("WPA") {
        (10, format!("Wi-Fi en {auth} {chiffre} : ancien, cassable. Choisis WPA2-AES ou WPA3 dans la box."))
    } else if a.contains("WEP") {
        (0, "Wi-Fi en WEP : cassable en quelques minutes ! Passe en WPA2 ou WPA3 tout de suite.".into())
    } else {
        (0, "Wi-Fi ouvert, sans mot de passe : tout le monde peut s'y connecter et voir ton trafic.".into())
    }
}


/// DNS « connus » (box, grands DNS publics, réseau local).
pub fn dns_connu(ip: &str, box_ip: &str) -> bool {
    const PUBLICS: &[&str] = &["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4", "9.9.9.9", "149.112.112.112", "208.67.222.222", "208.67.220.220", "94.140.14.14", "94.140.15.15", "80.67.169.12", "80.67.169.40", "212.27.40.240", "212.27.40.241", "80.10.246.2", "80.10.246.129", "194.158.122.10", "194.158.122.15"];
    if ip == box_ip || PUBLICS.contains(&ip) || ip.starts_with("fe80") {
        return true;
    }
    ip.parse::<Ipv4Addr>().map(|a| a.is_private() || a.is_loopback()).unwrap_or(false)
}


/// Plusieurs adresses IP partagent-elles la MAC de la box ? (signe d'usurpation ARP)
pub fn usurpation_arp(arp: &BTreeMap<Ipv4Addr, String>, box_ip: Ipv4Addr) -> Option<Vec<Ipv4Addr>> {
    let mac = arp.get(&box_ip)?;
    let autres: Vec<Ipv4Addr> = arp.iter().filter(|(ip, m)| *m == mac && **ip != box_ip).map(|(ip, _)| *ip).collect();
    (!autres.is_empty()).then_some(autres)
}


/// Domaines de pub, de pistage ou de télémétrie (dans le nom inverse du serveur).
pub fn pisteur(nom: &str) -> Option<&'static str> {
    let n = nom.to_lowercase();
    const L: &[(&str, &str)] = &[
        ("doubleclick", "pub Google"), ("googlesyndication", "pub Google"), ("google-analytics", "statistiques Google"), ("googleadservices", "pub Google"),
        ("facebook", "Facebook / Meta"), ("fbcdn", "Facebook / Meta"), ("scorecardresearch", "mesure d'audience"), ("adnxs", "pub"), ("criteo", "pub ciblée"),
        ("taboola", "pub"), ("outbrain", "pub"), ("hotjar", "enregistrement de navigation"), ("mixpanel", "statistiques"), ("segment.io", "statistiques"),
        ("appsflyer", "pistage d'applis"), ("adjust.com", "pistage d'applis"), ("telemetry", "télémétrie"), ("vortex", "télémétrie Microsoft"), ("watson.microsoft", "rapports d'erreurs Microsoft"),
        ("data.microsoft", "télémétrie Microsoft"), ("app-measurement", "statistiques Google"), ("amazon-adsystem", "pub Amazon"), ("tiktok", "TikTok"), ("bytedance", "TikTok"),
    ];
    L.iter().find(|(k, _)| n.contains(k)).map(|x| x.1)
}


/// Genre d'appareil probable d'après son fabricant, son nom et ses ports.
pub fn genre(fab: &str, nom: &str, ports: &[u16]) -> &'static str {
    let t = format!("{fab} {nom}").to_lowercase();
    let a = |l: &[&str]| l.iter().any(|x| t.contains(x));
    if ports.contains(&554) || a(&["hikvision", "dahua", "reolink", "ezviz", "wyze", "camera", "ring", "arlo", "foscam"]) {
        "📹 caméra"
    } else if ports.contains(&9100) || a(&["printer", "imprim", "epson", "canon", "brother", "hp inc", "hewlett"]) {
        "🖨 imprimante"
    } else if a(&["freebox", "sagem", "livebox", "bbox", "technicolor", "sercomm", "arcadyan", "box", "router", "tp-link", "netgear", "asus"]) {
        "📡 box / routeur"
    } else if a(&["sony interactive", "playstation", "nintendo", "xbox"]) {
        "🎮 console"
    } else if ports.contains(&8008) || a(&["chromecast", "roku", "lg electronics", "samsung electronics", "tcl", "hisense", "tv", "vizio"]) {
        "📺 TV / multimédia"
    } else if a(&["espressif", "tuya", "shelly", "sonoff", "signify", "philips lighting", "xiaomi", "amazon", "google", "nest", "ecobee"]) {
        "💡 objet connecté"
    } else if ports.contains(&62078) || a(&["apple", "iphone", "ipad", "privée", "android", "oneplus", "oppo", "huawei"]) {
        "📱 téléphone / tablette"
    } else if ports.contains(&445) || ports.contains(&3389) || a(&["intel", "realtek", "micro-star", "gigabyte", "dell", "lenovo", "-pc", "desktop", "laptop"]) {
        "💻 ordinateur"
    } else if a(&["raspberry", "synology", "qnap", "nas"]) {
        "🗄 serveur / NAS"
    } else {
        "❔ appareil"
    }
}
