//! Musique dans toute la maison (16) : un PC joue une liste de musiques, les autres PC de la maison
//! (avec Outils) la jouent en même temps. Les horloges sont comparées sur le réseau, et chaque morceau
//! de son porte l'heure exacte à laquelle il doit sonner.
//! Ports : UDP 8652 (trouver les PC), TCP 8653 (son et synchronisation).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering::Relaxed};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const PORT_DECOUVERTE: u16 = 8652;
pub const PORT_SON: u16 = 8653;
pub const FREQ: u32 = 44100;
const BLOC: usize = 4096; // trames stéréo par paquet (~93 ms)
const AVANCE_US: i64 = 1_200_000; // le son part avec 1,2 s d'avance (le temps d'arriver partout)

/// Heure (µs) de l'horloge de ce PC.
pub fn horloge() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as i64).unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// demande l'heure (émetteur → récepteur), avec l'heure de l'émetteur
    Ping(i64),
    /// réponse : heure du récepteur
    Pong(i64),
    /// son à jouer à l'heure donnée (horloge du récepteur), échantillons stéréo 16 bits
    Son(i64, Vec<i16>),
    Stop,
    /// titre en cours (pour l'affichage)
    Titre(String),
}

pub fn ecrire_message(w: &mut impl Write, m: &Message) -> std::io::Result<()> {
    let (t, corps): (u8, Vec<u8>) = match m {
        Message::Ping(x) => (1, x.to_le_bytes().to_vec()),
        Message::Pong(x) => (2, x.to_le_bytes().to_vec()),
        Message::Son(t, e) => {
            let mut v = t.to_le_bytes().to_vec();
            v.extend(e.iter().flat_map(|s| s.to_le_bytes()));
            (3, v)
        }
        Message::Stop => (4, vec![]),
        Message::Titre(s) => (5, s.as_bytes().to_vec()),
    };
    w.write_all(&[t])?;
    w.write_all(&(corps.len() as u32).to_le_bytes())?;
    w.write_all(&corps)
}

pub fn lire_message(r: &mut impl Read) -> std::io::Result<Message> {
    let mut e = [0u8; 5];
    r.read_exact(&mut e)?;
    let n = u32::from_le_bytes([e[1], e[2], e[3], e[4]]) as usize;
    if n > 4 << 20 {
        return Err(std::io::Error::other("paquet trop gros"));
    }
    let mut c = vec![0u8; n];
    r.read_exact(&mut c)?;
    let i64_de = |b: &[u8]| -> std::io::Result<i64> { Ok(i64::from_le_bytes(b.get(..8).ok_or_else(|| std::io::Error::other("court"))?.try_into().unwrap())) };
    Ok(match e[0] {
        1 => Message::Ping(i64_de(&c)?),
        2 => Message::Pong(i64_de(&c)?),
        3 => Message::Son(i64_de(&c)?, c[8..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()),
        4 => Message::Stop,
        5 => Message::Titre(String::from_utf8_lossy(&c).to_string()),
        _ => return Err(std::io::Error::other("message inconnu")),
    })
}

/// Écart d'horloge (récepteur − émetteur) d'après des mesures (envoi, réponse, retour) :
/// on garde l'aller-retour le plus court, le plus fiable.
pub fn ecart(mesures: &[(i64, i64, i64)]) -> Option<i64> {
    mesures.iter().min_by_key(|(a, _, c)| c - a).map(|(a, b, c)| b - (a + c) / 2)
}

// ---------------------------------------------------------------------------------------------------
// Lecture (récepteur et émetteur jouent de la même façon)
// ---------------------------------------------------------------------------------------------------

pub enum Commande {
    Son(i64, Vec<i16>),
    Stop,
}

/// Fil qui joue les paquets à leur heure. `decalage_ms` corrige le retard propre à la carte son.
pub fn lecteur(rx: Receiver<Commande>, volume: Arc<AtomicU32>, decalage_ms: Arc<AtomicI32>) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            let Ok((_flux, h)) = rodio::OutputStream::try_default() else { return };
            let mut sink = rodio::Sink::try_new(&h).ok();
            let mut fin_prevue: i64 = 0; // heure où le son déjà en file se termine
            while let Ok(c) = rx.recv() {
                match c {
                    Commande::Stop => {
                        if let Some(s) = sink.take() {
                            s.stop();
                        }
                        sink = rodio::Sink::try_new(&h).ok();
                        fin_prevue = 0;
                    }
                    Commande::Son(t, e) => {
                        let Some(s) = &sink else { continue };
                        s.set_volume(volume.load(Relaxed) as f32 / 100.0);
                        let duree = (e.len() / 2) as i64 * 1_000_000 / FREQ as i64;
                        let cible = t - decalage_ms.load(Relaxed) as i64 * 1000;
                        let maintenant = horloge();
                        if cible + duree < maintenant {
                            continue; // trop tard : on saute ce paquet
                        }
                        // file vide ou décalée de plus de 80 ms : on se recale sur l'heure prévue
                        if s.empty() || (cible - fin_prevue).abs() > 80_000 {
                            if cible > maintenant {
                                std::thread::sleep(Duration::from_micros((cible - maintenant) as u64));
                            }
                            fin_prevue = cible;
                        }
                        s.append(rodio::buffer::SamplesBuffer::new(2, FREQ, e));
                        fin_prevue += duree;
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            // pacat (PulseAudio / PipeWire) reçoit le son brut sur son entrée
            let lancer = || std::process::Command::new("pacat").args(["--format=s16le", "--rate=44100", "--channels=2", "--latency-msec=60"]).stdin(std::process::Stdio::piped()).spawn().ok();
            let mut pacat: Option<std::process::Child> = None;
            let mut fin_prevue: i64 = 0;
            while let Ok(c) = rx.recv() {
                match c {
                    Commande::Stop => {
                        if let Some(mut p) = pacat.take() {
                            let _ = p.kill();
                        }
                        fin_prevue = 0;
                    }
                    Commande::Son(t, mut e) => {
                        let duree = (e.len() / 2) as i64 * 1_000_000 / FREQ as i64;
                        let cible = t - decalage_ms.load(Relaxed) as i64 * 1000;
                        let maintenant = horloge();
                        if cible + duree < maintenant {
                            continue;
                        }
                        if pacat.is_none() || (cible - fin_prevue).abs() > 80_000 {
                            if let Some(mut p) = pacat.take() {
                                let _ = p.kill();
                            }
                            if cible > maintenant {
                                std::thread::sleep(Duration::from_micros((cible - maintenant) as u64));
                            }
                            pacat = lancer();
                            fin_prevue = cible;
                        }
                        let v = volume.load(Relaxed) as f32 / 100.0;
                        for s in &mut e {
                            *s = (*s as f32 * v) as i16;
                        }
                        let octets: Vec<u8> = e.iter().flat_map(|s| s.to_le_bytes()).collect();
                        if let Some(p) = &mut pacat {
                            if p.stdin.as_mut().map(|i| i.write_all(&octets)).is_none_or(|r| r.is_err()) {
                                pacat = None;
                            }
                        }
                        fin_prevue += duree;
                    }
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------------------------------
// Récepteur
// ---------------------------------------------------------------------------------------------------

/// Répond « je suis là » aux PC qui cherchent, et reçoit le son.
pub fn recepteur(actif: Arc<AtomicBool>, vers_lecteur: Sender<Commande>, titre: Arc<Mutex<String>>) {
    // découverte
    {
        let actif = actif.clone();
        std::thread::spawn(move || {
            let Ok(s) = UdpSocket::bind(("0.0.0.0", PORT_DECOUVERTE)) else { return };
            let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
            let nom = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "PC".into());
            let mut b = [0u8; 64];
            while actif.load(Relaxed) {
                if let Ok((n, de)) = s.recv_from(&mut b) {
                    if &b[..n] == b"OUTILS-MUSIQUE?" {
                        let _ = s.send_to(format!("OUTILS-MUSIQUE!|{nom}").as_bytes(), de);
                    }
                }
            }
        });
    }
    // son
    std::thread::spawn(move || {
        let Ok(l) = TcpListener::bind(("0.0.0.0", PORT_SON)) else { return };
        let _ = l.set_nonblocking(true);
        while actif.load(Relaxed) {
            match l.accept() {
                Ok((mut c, _)) => {
                    let _ = c.set_nonblocking(false);
                    let _ = c.set_nodelay(true);
                    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
                    while actif.load(Relaxed) {
                        match lire_message(&mut c) {
                            Ok(Message::Ping(_)) => {
                                if ecrire_message(&mut c, &Message::Pong(horloge())).is_err() {
                                    break;
                                }
                            }
                            Ok(Message::Son(t, e)) => {
                                let _ = vers_lecteur.send(Commande::Son(t, e));
                            }
                            Ok(Message::Stop) => {
                                let _ = vers_lecteur.send(Commande::Stop);
                            }
                            Ok(Message::Titre(t)) => *titre.lock().unwrap() = t,
                            Ok(Message::Pong(_)) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                            Err(_) => break,
                        }
                    }
                    let _ = vers_lecteur.send(Commande::Stop);
                    titre.lock().unwrap().clear();
                }
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    });
}

/// Cherche les PC récepteurs sur le réseau (1 seconde).
pub fn chercher() -> Vec<(String, SocketAddr)> {
    let Ok(s) = UdpSocket::bind("0.0.0.0:0") else { return Vec::new() };
    let _ = s.set_broadcast(true);
    let _ = s.set_read_timeout(Some(Duration::from_millis(250)));
    let _ = s.send_to(b"OUTILS-MUSIQUE?", ("255.255.255.255", PORT_DECOUVERTE));
    if let Some(ip) = crate::telephone::ip_locale() {
        if let Some((debut, _)) = ip.rsplit_once('.') {
            let _ = s.send_to(b"OUTILS-MUSIQUE?", (format!("{debut}.255").as_str(), PORT_DECOUVERTE));
        }
    }
    let mut v: Vec<(String, SocketAddr)> = Vec::new();
    let fin = std::time::Instant::now() + Duration::from_secs(1);
    let mut b = [0u8; 128];
    while std::time::Instant::now() < fin {
        if let Ok((n, de)) = s.recv_from(&mut b) {
            if let Some(nom) = std::str::from_utf8(&b[..n]).ok().and_then(|t| t.strip_prefix("OUTILS-MUSIQUE!|")) {
                if !v.iter().any(|x| x.1.ip() == de.ip()) {
                    v.push((nom.to_string(), SocketAddr::new(de.ip(), PORT_SON)));
                }
            }
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------------
// Émetteur
// ---------------------------------------------------------------------------------------------------

struct Sortie {
    flux: TcpStream,
    ecart: i64,
}

fn connecter(a: SocketAddr) -> Option<Sortie> {
    let mut flux = TcpStream::connect_timeout(&a, Duration::from_secs(2)).ok()?;
    let _ = flux.set_nodelay(true);
    let _ = flux.set_read_timeout(Some(Duration::from_secs(2)));
    let mut mesures = Vec::new();
    for _ in 0..8 {
        let t0 = horloge();
        ecrire_message(&mut flux, &Message::Ping(t0)).ok()?;
        if let Ok(Message::Pong(tr)) = lire_message(&mut flux) {
            mesures.push((t0, tr, horloge()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Some(Sortie { flux, ecart: ecart(&mesures)? })
}

pub struct Emission {
    pub stop: Arc<AtomicBool>,
    pub suivant: Arc<AtomicBool>,
    pub titre: Arc<Mutex<String>>,
}

/// Joue la liste sur ce PC (si `local`) et sur les récepteurs choisis.
pub fn emettre(liste: Vec<PathBuf>, cibles: Vec<SocketAddr>, local: Option<Sender<Commande>>, etat: Arc<Mutex<String>>) -> Emission {
    let e = Emission { stop: Arc::default(), suivant: Arc::default(), titre: Arc::default() };
    let (stop, suivant, titre) = (e.stop.clone(), e.suivant.clone(), e.titre.clone());
    std::thread::spawn(move || {
        *etat.lock().unwrap() = "Connexion aux PC…".into();
        let mut sorties: Vec<Sortie> = cibles.into_iter().filter_map(connecter).collect();
        *etat.lock().unwrap() = format!("🔊 Musique sur {} PC{}", sorties.len(), if local.is_some() { " + celui-ci" } else { "" });
        let mut debut = horloge() + AVANCE_US;
        'liste: for f in liste.iter().cycle().take(liste.len() * 50) {
            if stop.load(Relaxed) {
                break;
            }
            let nom = f.file_stem().unwrap_or_default().to_string_lossy().to_string();
            *titre.lock().unwrap() = nom.clone();
            for s in &mut sorties {
                let _ = ecrire_message(&mut s.flux, &Message::Titre(nom.clone()));
            }
            let Ok(mut c) = crate::createur::ffmpeg() else {
                *etat.lock().unwrap() = "ffmpeg introuvable".into();
                break;
            };
            let Ok(mut p) = c.args(["-v", "error", "-i"]).arg(f).args(["-vn", "-f", "s16le", "-ac", "2", "-ar", "44100", "-"]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn() else { continue };
            let Some(mut sortie) = p.stdout.take() else { continue };
            let mut tampon = vec![0u8; BLOC * 4];
            loop {
                if stop.load(Relaxed) || suivant.swap(false, Relaxed) {
                    let _ = p.kill();
                    // on coupe net partout et on repart un peu plus tard
                    if let Some(l) = &local {
                        let _ = l.send(Commande::Stop);
                    }
                    for s in &mut sorties {
                        let _ = ecrire_message(&mut s.flux, &Message::Stop);
                    }
                    debut = horloge() + AVANCE_US;
                    if stop.load(Relaxed) {
                        break 'liste;
                    }
                    continue 'liste;
                }
                let mut lu = 0;
                while lu < tampon.len() {
                    match sortie.read(&mut tampon[lu..]) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => lu += n,
                    }
                }
                if lu < 4 {
                    break;
                }
                let e: Vec<i16> = tampon[..lu - lu % 4].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
                let duree = (e.len() / 2) as i64 * 1_000_000 / FREQ as i64;
                // pas plus de 2 s d'avance dans les tuyaux
                let attente = debut - horloge() - 2_000_000;
                if attente > 0 {
                    std::thread::sleep(Duration::from_micros(attente as u64));
                }
                if let Some(l) = &local {
                    let _ = l.send(Commande::Son(debut, e.clone()));
                }
                sorties.retain_mut(|s| ecrire_message(&mut s.flux, &Message::Son(debut + s.ecart, e.clone())).is_ok());
                debut += duree;
            }
            let _ = p.wait();
        }
        for s in &mut sorties {
            let _ = ecrire_message(&mut s.flux, &Message::Stop);
        }
        etat.lock().unwrap().clear();
        titre.lock().unwrap().clear();
    });
    e
}

// ---------------------------------------------------------------------------------------------------
// Outil
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Reglages {
    recevoir: bool,
    volume: u32,
    decalage_ms: i32,
    liste: Vec<PathBuf>,
    local: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { recevoir: false, volume: 80, decalage_ms: 0, liste: Vec::new(), local: true }
    }
}

pub struct MusiqueMaison {
    r: Reglages,
    recepteur_actif: Arc<AtomicBool>,
    vers_lecteur: Sender<Commande>,
    volume: Arc<AtomicU32>,
    decalage: Arc<AtomicI32>,
    titre_recu: Arc<Mutex<String>>,
    trouves: Arc<Mutex<Vec<(String, SocketAddr, bool)>>>,
    emission: Option<Emission>,
    etat: Arc<Mutex<String>>,
}

impl MusiqueMaison {
    pub fn new() -> Self {
        let r: Reglages = charger("musique_maison");
        let (tx, rx) = channel();
        let volume = Arc::new(AtomicU32::new(r.volume));
        let decalage = Arc::new(AtomicI32::new(r.decalage_ms));
        lecteur(rx, volume.clone(), decalage.clone());
        let m = MusiqueMaison { recepteur_actif: Arc::default(), vers_lecteur: tx, volume, decalage, titre_recu: Arc::default(), trouves: Arc::default(), emission: None, etat: Arc::default(), r };
        if m.r.recevoir {
            m.demarrer_recepteur();
        }
        m
    }

    fn demarrer_recepteur(&self) {
        if !self.recepteur_actif.swap(true, Relaxed) {
            recepteur(self.recepteur_actif.clone(), self.vers_lecteur.clone(), self.titre_recu.clone());
        }
    }

    fn chercher(&self) {
        let t = self.trouves.clone();
        std::thread::spawn(move || {
            let v = chercher();
            let mut l = t.lock().unwrap();
            for (n, a) in v {
                if !l.iter().any(|x| x.1 == a) {
                    l.push((n, a, true));
                }
            }
        });
    }
}

impl Outil for MusiqueMaison {
    fn nom(&self) -> &'static str {
        "🎶 Musique dans toute la maison"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn resume(&self) -> Option<String> {
        let t = self.titre_recu.lock().unwrap().clone();
        (!t.is_empty()).then(|| format!("🎶 {t}"))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.r.clone();
        ui.heading("Musique dans toute la maison");
        ui.weak("La même musique sur plusieurs PC en même temps. Sur chaque PC qui doit jouer, ouvre cet outil et coche « Ce PC joue la musique des autres ».");
        // récepteur
        egui::Frame::group(ui.style()).show(ui, |ui| {
            if ui.checkbox(&mut self.r.recevoir, RichText::new("📻 Ce PC joue la musique des autres").strong()).changed() {
                if self.r.recevoir {
                    self.demarrer_recepteur();
                } else {
                    self.recepteur_actif.store(false, Relaxed);
                    let _ = self.vers_lecteur.send(Commande::Stop);
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("Volume");
                ui.add(egui::Slider::new(&mut self.r.volume, 0..=100).suffix(" %"));
                ui.label("Décalage");
                ui.add(egui::Slider::new(&mut self.r.decalage_ms, -500..=500).suffix(" ms")).on_hover_text("Si ce PC est un peu en avance ou en retard par rapport aux autres, corrige à l'oreille");
            });
            self.volume.store(self.r.volume, Relaxed);
            self.decalage.store(self.r.decalage_ms, Relaxed);
            let t = self.titre_recu.lock().unwrap().clone();
            if !t.is_empty() {
                ui.colored_label(Color32::LIGHT_GREEN, format!("🎵 En train de jouer : {t}"));
            }
        });
        ui.add_space(6.0);
        // émetteur
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.strong("📡 Envoyer de la musique aux autres PC");
            ui.horizontal_wrapped(|ui| {
                if ui.button("🔎 Chercher les PC").clicked() {
                    self.chercher();
                }
                ui.checkbox(&mut self.r.local, "Ce PC joue aussi");
            });
            let mut l = self.trouves.lock().unwrap();
            if l.is_empty() {
                ui.weak("Aucun PC trouvé pour l'instant (ils doivent avoir coché « Ce PC joue la musique des autres »).");
            }
            for (n, a, choisi) in l.iter_mut() {
                ui.checkbox(choisi, format!("🖥 {n} ({})", a.ip()));
            }
            drop(l);
            ui.horizontal_wrapped(|ui| {
                if ui.button("➕ Ajouter des musiques…").clicked() {
                    self.r.liste.extend(rfd::FileDialog::new().add_filter("Musique", &["mp3", "m4a", "flac", "wav", "ogg", "opus", "aac"]).pick_files().unwrap_or_default());
                }
                if !self.r.liste.is_empty() && ui.small_button("🧹 Vider").clicked() {
                    self.r.liste.clear();
                }
            });
            let mut retirer = None;
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                for (k, f) in self.r.liste.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("🎵 {}", f.file_stem().unwrap_or_default().to_string_lossy()));
                        if ui.small_button("✖").clicked() {
                            retirer = Some(k);
                        }
                    });
                }
            });
            if let Some(k) = retirer {
                self.r.liste.remove(k);
            }
            ui.horizontal(|ui| {
                let en_cours = self.emission.as_ref().is_some_and(|e| !e.stop.load(Relaxed)) && !self.etat.lock().unwrap().is_empty();
                if !en_cours {
                    if ui.add_enabled(!self.r.liste.is_empty(), egui::Button::new(RichText::new("▶ Jouer partout").strong())).clicked() {
                        let cibles: Vec<SocketAddr> = self.trouves.lock().unwrap().iter().filter(|x| x.2).map(|x| x.1).collect();
                        let local = self.r.local.then(|| self.vers_lecteur.clone());
                        self.emission = Some(emettre(self.r.liste.clone(), cibles, local, self.etat.clone()));
                    }
                } else {
                    if ui.button("⏭ Suivante").clicked() {
                        if let Some(e) = &self.emission {
                            e.suivant.store(true, Relaxed);
                        }
                    }
                    if ui.button("⏹ Stop").clicked() {
                        if let Some(e) = &self.emission {
                            e.stop.store(true, Relaxed);
                        }
                    }
                    if let Some(e) = &self.emission {
                        ui.label(format!("🎵 {}", e.titre.lock().unwrap()));
                    }
                }
            });
            ui.label(self.etat.lock().unwrap().clone());
        });
        ui.weak("La première fois, Windows peut demander d'autoriser Outils sur le réseau privé : accepte.");
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        if self.r != avant {
            enregistrer("musique_maison", &self.r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_et_synchro() {
        let mut b = Vec::new();
        let ms = [Message::Ping(123), Message::Pong(-5), Message::Son(1_000_000, vec![1, -2, 3, 4]), Message::Stop, Message::Titre("Été".into())];
        for m in &ms {
            ecrire_message(&mut b, m).unwrap();
        }
        let mut r = std::io::Cursor::new(b);
        for m in &ms {
            assert_eq!(&lire_message(&mut r).unwrap(), m);
        }
        // récepteur en avance de 500 ms ; l'aller-retour le plus court donne la meilleure mesure
        assert_eq!(ecart(&[(0, 500_010, 40), (100, 500_150, 200), (300, 500_302, 304)]), Some(500_000));
        assert_eq!(ecart(&[]), None);
    }

    #[test]
    fn recepteur_local() {
        // un vrai récepteur sur ce PC, sur des ports libres : synchro puis son reçu à la bonne heure
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            loop {
                match lire_message(&mut c) {
                    Ok(Message::Ping(_)) => ecrire_message(&mut c, &Message::Pong(horloge() + 2_000_000)).unwrap(),
                    Ok(Message::Son(t, e)) => {
                        tx.send((t, e)).unwrap();
                        break;
                    }
                    _ => {}
                }
            }
        });
        let mut s = connecter(a).unwrap();
        // l'horloge « du récepteur » a 2 s d'avance : l'écart mesuré est d'environ 2 s
        assert!((s.ecart - 2_000_000).abs() < 50_000, "{}", s.ecart);
        ecrire_message(&mut s.flux, &Message::Son(10 + s.ecart, vec![7, 8])).unwrap();
        let (t, e) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(e, vec![7, 8]);
        assert!((t - 10 - 2_000_000).abs() < 50_000);
    }
}
