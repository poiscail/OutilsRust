//! Adresse du téléphone sur le Wi-Fi.
use std::net::UdpSocket;

pub fn ip_locale() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.168.1.1:80").ok()?; // aucun paquet envoyé : sert à choisir l'interface
    Some(s.local_addr().ok()?.ip().to_string())
}
