//! Micro : changeur de voix (65) et réducteur de bruit (66).
//! Le micro est lu, traité (anti-bruit RNNoise, effets), puis envoyé dans un câble audio virtuel
//! (VB-CABLE) que Discord, OBS ou un jeu choisissent comme micro.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FREQ: f32 = 48_000.0;
const TRAME: usize = nnnoiseless::FRAME_SIZE; // 480 échantillons = 10 ms

// ---------------------------------------------------------------------------------------------------
// Traitement du son (pur, testable)
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Voix {
    #[default]
    Normale,
    Grave,
    Aigue,
    Robot,
    Radio,
    Monstre,
    Echo,
    Grotte,
}

impl Voix {
    pub const TOUTES: [Voix; 8] = [Voix::Normale, Voix::Grave, Voix::Aigue, Voix::Robot, Voix::Radio, Voix::Monstre, Voix::Echo, Voix::Grotte];
    pub fn nom(self) -> &'static str {
        match self {
            Voix::Normale => "🙂 Normale",
            Voix::Grave => "🐻 Grave",
            Voix::Aigue => "🐿 Aiguë",
            Voix::Robot => "🤖 Robot",
            Voix::Radio => "📻 Talkie-walkie",
            Voix::Monstre => "👹 Monstre",
            Voix::Echo => "🏔 Écho",
            Voix::Grotte => "🕳 Grotte",
        }
    }
}

/// Changement de hauteur par ligne à retard (deux lectures décalées, fondu entre elles).
pub struct Hauteur {
    tampon: Vec<f32>,
    ecrit: usize,
    phase: f32,
}

impl Hauteur {
    const TAILLE: usize = 2048;
    pub fn new() -> Self {
        Hauteur { tampon: vec![0.0; Self::TAILLE], ecrit: 0, phase: 0.0 }
    }

    pub fn traiter(&mut self, x: f32, rapport: f32) -> f32 {
        let n = Self::TAILLE as f32;
        let fenetre = n / 2.0;
        self.tampon[self.ecrit] = x;
        // le retard varie de 0 à « fenetre » ; deux têtes décalées d'une demi-fenêtre
        self.phase = (self.phase + (1.0 - rapport)).rem_euclid(fenetre);
        let lire = |retard: f32, t: &[f32], ecrit: usize| {
            let p = (ecrit as f32 - retard).rem_euclid(n);
            let i = p as usize;
            let f = p - i as f32;
            t[i] * (1.0 - f) + t[(i + 1) % Self::TAILLE] * f
        };
        let d1 = self.phase;
        let d2 = (self.phase + fenetre / 2.0).rem_euclid(fenetre);
        let g1 = (std::f32::consts::PI * d1 / fenetre).sin();
        let g2 = (std::f32::consts::PI * d2 / fenetre).sin();
        let y = lire(d1, &self.tampon, self.ecrit) * g1 + lire(d2, &self.tampon, self.ecrit) * g2;
        self.ecrit = (self.ecrit + 1) % Self::TAILLE;
        y / (g1 + g2).max(0.7)
    }
}

/// Filtre biquad (RBJ).
#[derive(Clone, Copy)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn nouveau(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Biquad { b: [b0 / a0, b1 / a0, b2 / a0], a: [a1 / a0, a2 / a0], z: [0.0; 2] }
    }
    pub fn passe_haut(f: f32, q: f32) -> Self {
        let w = 2.0 * std::f32::consts::PI * f / FREQ;
        let (c, al) = (w.cos(), w.sin() / (2.0 * q));
        Self::nouveau((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    pub fn passe_bas(f: f32, q: f32) -> Self {
        let w = 2.0 * std::f32::consts::PI * f / FREQ;
        let (c, al) = (w.cos(), w.sin() / (2.0 * q));
        Self::nouveau((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    pub fn traiter(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

pub struct Effets {
    pub voix: Voix,
    hauteur: Hauteur,
    hp: Biquad,
    pb: Biquad,
    retard: Vec<f32>,
    pos: usize,
    t: f32,
}

impl Effets {
    pub fn new() -> Self {
        Effets { voix: Voix::Normale, hauteur: Hauteur::new(), hp: Biquad::passe_haut(400.0, 0.7), pb: Biquad::passe_bas(3000.0, 0.7), retard: vec![0.0; 48_000], pos: 0, t: 0.0 }
    }

    fn ligne(&mut self, x: f32, ms: f32, retour: f32, melange: f32) -> f32 {
        let n = ((ms / 1000.0 * FREQ) as usize).clamp(1, self.retard.len() - 1);
        let i = (self.pos + self.retard.len() - n) % self.retard.len();
        let r = self.retard[i];
        self.retard[self.pos] = x + r * retour;
        self.pos = (self.pos + 1) % self.retard.len();
        x + r * melange
    }

    /// Un échantillon (valeurs -1..1).
    pub fn traiter(&mut self, x: f32) -> f32 {
        self.t += 1.0 / FREQ;
        let y = match self.voix {
            Voix::Normale => x,
            Voix::Grave => self.hauteur.traiter(x, 0.75),
            Voix::Aigue => self.hauteur.traiter(x, 1.5),
            Voix::Robot => {
                let m = (2.0 * std::f32::consts::PI * 60.0 * self.t).sin();
                let y = self.ligne(x * m, 9.0, 0.55, 0.6);
                y * 1.4
            }
            Voix::Radio => {
                let y = self.pb.traiter(self.hp.traiter(x));
                (y * 4.0).tanh() * 0.6
            }
            Voix::Monstre => {
                let y = self.hauteur.traiter(x, 0.6);
                (y * 2.5).tanh() * 0.8
            }
            Voix::Echo => self.ligne(x, 320.0, 0.45, 0.6),
            Voix::Grotte => self.ligne(x, 90.0, 0.7, 0.5),
        };
        if self.t > 3600.0 {
            self.t = 0.0;
        }
        y.clamp(-1.0, 1.0)
    }
}

/// Rééchantillonnage linéaire en continu (d'une fréquence à une autre).
pub struct Reech {
    pas: f64,
    pos: f64,
    prec: f32,
}

impl Reech {
    pub fn new(de: f32, vers: f32) -> Self {
        Reech { pas: de as f64 / vers as f64, pos: 0.0, prec: 0.0 }
    }

    pub fn traiter(&mut self, entree: &[f32], sortie: &mut Vec<f32>) {
        if entree.is_empty() {
            return;
        }
        if (self.pas - 1.0).abs() < 1e-9 {
            sortie.extend_from_slice(entree);
            return;
        }
        // pos est relatif au bloc : l'indice -1 est le dernier échantillon du bloc précédent (prec)
        let n = entree.len() as isize;
        let lire = |k: isize, prec: f32| if k < 0 { prec } else { entree[k as usize] };
        loop {
            let i = self.pos.floor() as isize;
            if i + 1 >= n {
                break;
            }
            let f = (self.pos - i as f64) as f32;
            let (a, b) = (lire(i, self.prec), lire(i + 1, self.prec));
            sortie.push(a + (b - a) * f);
            self.pos += self.pas;
        }
        self.pos -= entree.len() as f64;
        self.prec = *entree.last().unwrap();
    }
}

/// Porte de bruit simple (quand l'anti-bruit IA est coupé) : coupe en dessous d'un seuil.
pub fn porte(niveau: f32, seuil: f32, gain: &mut f32) -> f32 {
    let cible = if niveau > seuil { 1.0 } else { 0.0 };
    *gain += (cible - *gain) * if cible > *gain { 0.5 } else { 0.05 };
    *gain
}

// ---------------------------------------------------------------------------------------------------
// Audio Windows (WASAPI)
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub voix: Voix,
    pub anti_bruit: bool,
    pub porte: bool,
    pub seuil: f32,
    pub volume: f32,
    pub ecoute: bool,
    pub sortie: String,
    pub auto: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { voix: Voix::Normale, anti_bruit: true, porte: false, seuil: 0.02, volume: 1.0, ecoute: false, sortie: String::new(), auto: false }
    }
}

#[derive(Default)]
struct Partage {
    marche: bool,
    erreur: Option<String>,
    niveau_entree: f32,
    niveau_sortie: f32,
    voix_detectee: f32,
}

mod wasapi {
    use super::*;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;

    fn e(x: windows::core::Error) -> String {
        x.message()
    }

    pub fn nom(d: &IMMDevice) -> String {
        unsafe { d.OpenPropertyStore(STGM_READ).and_then(|p| p.GetValue(&PKEY_Device_FriendlyName)).map(|v| v.to_string()).unwrap_or_default() }
    }

    pub fn sorties() -> Vec<String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return vec![] };
            let Ok(col) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return vec![] };
            (0..col.GetCount().unwrap_or(0)).filter_map(|i| col.Item(i).ok()).map(|d| nom(&d)).collect()
        }
    }

    struct Sortie {
        client: IAudioClient,
        rendu: IAudioRenderClient,
        taille: u32,
        canaux: usize,
        flottant: bool,
        reech: Reech,
        file: VecDeque<f32>,
        freq: f32,
    }

    impl Sortie {
        unsafe fn ouvrir(d: &IMMDevice) -> Result<Sortie, String> {
            let client: IAudioClient = d.Activate(CLSCTX_ALL, None).map_err(e)?;
            let fmt = client.GetMixFormat().map_err(e)?;
            let (canaux, freq, bits) = ((*fmt).nChannels as usize, (*fmt).nSamplesPerSec as f32, (*fmt).wBitsPerSample);
            let flottant = bits == 32 && ((*fmt).wFormatTag == 3 || (*fmt).wFormatTag == 0xFFFE);
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 400_000, 0, fmt, None).map_err(e)?; // 40 ms
            CoTaskMemFree(Some(fmt as *const _));
            let rendu: IAudioRenderClient = client.GetService().map_err(e)?;
            let taille = client.GetBufferSize().map_err(e)?;
            client.Start().map_err(e)?;
            Ok(Sortie { client, rendu, taille, canaux, flottant, reech: Reech::new(FREQ, freq), file: VecDeque::new(), freq })
        }

        unsafe fn pousser(&mut self, trame: &[f32]) {
            let mut v = Vec::with_capacity(trame.len() * 2);
            self.reech.traiter(trame, &mut v);
            self.file.extend(v);
            // latence bornée : on jette le surplus au-delà de 80 ms
            let max = (self.freq * 0.08) as usize;
            if self.file.len() > max {
                let n = self.file.len() - max / 2;
                self.file.drain(..n);
            }
        }

        unsafe fn servir(&mut self) -> Result<(), String> {
            let libre = self.taille - self.client.GetCurrentPadding().map_err(e)?;
            let n = (libre as usize).min(self.file.len());
            if n == 0 {
                return Ok(());
            }
            let p = self.rendu.GetBuffer(n as u32).map_err(e)?;
            for i in 0..n {
                let s = self.file.pop_front().unwrap_or(0.0);
                for c in 0..self.canaux {
                    let k = i * self.canaux + c;
                    if self.flottant {
                        *(p as *mut f32).add(k) = s;
                    } else {
                        *(p as *mut i16).add(k) = crate::son::f32_vers_i16(s);
                    }
                }
            }
            self.rendu.ReleaseBuffer(n as u32, 0).map_err(e)?;
            Ok(())
        }
    }

    impl Drop for Sortie {
        fn drop(&mut self) {
            unsafe {
                let _ = self.client.Stop();
            }
        }
    }

    pub fn boucle(r: Arc<Mutex<Reglages>>, p: Arc<Mutex<Partage>>, stop: Arc<AtomicBool>) -> Result<(), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e)?;
            let micro = en.GetDefaultAudioEndpoint(eCapture, eCommunications).or_else(|_| en.GetDefaultAudioEndpoint(eCapture, eConsole)).map_err(e)?;
            let reg = r.lock().unwrap().clone();
            // sortie : le câble virtuel choisi
            let col = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).map_err(e)?;
            let cable = (0..col.GetCount().map_err(e)?).filter_map(|i| col.Item(i).ok()).find(|d| {
                let n = nom(d);
                if reg.sortie.is_empty() { n.to_lowercase().contains("cable input") } else { n == reg.sortie }
            }).ok_or("câble virtuel introuvable : installe VB-CABLE (bouton ci-dessous) ou choisis une sortie")?;
            let mut sorties = vec![Sortie::ouvrir(&cable)?];
            let mut ecoute: Option<Sortie> = None;

            let client: IAudioClient = micro.Activate(CLSCTX_ALL, None).map_err(e)?;
            let fmt = client.GetMixFormat().map_err(e)?;
            let (canaux, freq, bits, bloc) = ((*fmt).nChannels as usize, (*fmt).nSamplesPerSec as f32, (*fmt).wBitsPerSample, (*fmt).nBlockAlign as usize);
            let flottant = bits == 32 && ((*fmt).wFormatTag == 3 || (*fmt).wFormatTag == 0xFFFE);
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 200_000, 0, fmt, None).map_err(e)?;
            CoTaskMemFree(Some(fmt as *const _));
            let capture: IAudioCaptureClient = client.GetService().map_err(e)?;
            client.Start().map_err(e)?;
            p.lock().unwrap().marche = true;

            let mut vers48 = Reech::new(freq, FREQ);
            let mut entree: VecDeque<f32> = VecDeque::new();
            let mut debruit = nnnoiseless::DenoiseState::new();
            let mut effets = Effets::new();
            let (mut gain, mut mono, mut r48) = (1.0f32, Vec::new(), Vec::new());
            let (mut tin, mut tout) = ([0f32; TRAME], [0f32; TRAME]);
            while !stop.load(Relaxed) {
                std::thread::sleep(Duration::from_millis(4));
                let reg = r.lock().unwrap().clone();
                effets.voix = reg.voix;
                match (reg.ecoute, ecoute.is_some()) {
                    (true, false) => ecoute = en.GetDefaultAudioEndpoint(eRender, eConsole).ok().and_then(|d| Sortie::ouvrir(&d).ok()),
                    (false, true) => ecoute = None,
                    _ => {}
                }
                let mut paquet = capture.GetNextPacketSize().map_err(e)?;
                while paquet > 0 {
                    let (mut data, mut n, mut drapeaux) = (std::ptr::null_mut(), 0u32, 0u32);
                    capture.GetBuffer(&mut data, &mut n, &mut drapeaux, None, None).map_err(e)?;
                    let octets = std::slice::from_raw_parts(data, n as usize * bloc);
                    mono.clear();
                    if drapeaux & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        mono.resize(n as usize, 0.0);
                    } else if flottant {
                        mono.extend(octets.chunks_exact(4 * canaux).map(|c| (0..canaux).map(|k| f32::from_le_bytes([c[4 * k], c[4 * k + 1], c[4 * k + 2], c[4 * k + 3]])).sum::<f32>() / canaux as f32));
                    } else if bits == 16 {
                        mono.extend(octets.chunks_exact(2 * canaux).map(|c| (0..canaux).map(|k| i16::from_le_bytes([c[2 * k], c[2 * k + 1]]) as f32 / 32768.0).sum::<f32>() / canaux as f32));
                    }
                    capture.ReleaseBuffer(n).map_err(e)?;
                    r48.clear();
                    vers48.traiter(&mono, &mut r48);
                    entree.extend(r48.iter());
                    paquet = capture.GetNextPacketSize().map_err(e)?;
                }
                while entree.len() >= TRAME {
                    for (i, x) in entree.drain(..TRAME).enumerate() {
                        tin[i] = x;
                    }
                    let crete_in = tin.iter().fold(0f32, |m, x| m.max(x.abs()));
                    let mut proba = 1.0;
                    if reg.anti_bruit {
                        let echelle: Vec<f32> = tin.iter().map(|x| x * 32768.0).collect();
                        proba = debruit.process_frame(&mut tout, &echelle);
                        for x in tout.iter_mut() {
                            *x /= 32768.0;
                        }
                    } else {
                        tout = tin;
                    }
                    if reg.porte {
                        let g = porte(crete_in, reg.seuil, &mut gain);
                        for x in tout.iter_mut() {
                            *x *= g;
                        }
                    }
                    for x in tout.iter_mut() {
                        *x = effets.traiter(*x * reg.volume);
                    }
                    for s in sorties.iter_mut().chain(ecoute.iter_mut()) {
                        s.pousser(&tout);
                    }
                    let mut pp = p.lock().unwrap();
                    pp.niveau_entree = crete_in;
                    pp.niveau_sortie = tout.iter().fold(0f32, |m, x| m.max(x.abs()));
                    pp.voix_detectee = proba;
                }
                for s in sorties.iter_mut().chain(ecoute.iter_mut()) {
                    s.servir()?;
                }
            }
            let _ = client.Stop();
            Ok(())
        }
    }
}

fn cable_installe(sorties: &[String]) -> bool {
    sorties.iter().any(|s| s.to_lowercase().contains("cable input"))
}

/// Télécharge VB-CABLE (site officiel) et lance son installateur.
fn installer_cable() -> Result<String, String> {
    let d = crate::dependances::dossier().join("vbcable");
    let _ = std::fs::create_dir_all(&d);
    let zip = d.join("VBCABLE_Driver_Pack.zip");
    let r = ureq::get("https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip").call().map_err(|e| e.to_string())?;
    let mut f = std::fs::File::create(&zip).map_err(|e| e.to_string())?;
    std::io::copy(&mut r.into_reader(), &mut f).map_err(|e| e.to_string())?;
    drop(f);
    let mut a = zip::ZipArchive::new(std::fs::File::open(&zip).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    a.extract(&d).map_err(|e| e.to_string())?;
    let setup = d.join(if cfg!(target_pointer_width = "64") { "VBCABLE_Setup_x64.exe" } else { "VBCABLE_Setup.exe" });
    // l'installateur demande lui-même les droits admin ; l'utilisateur clique « Install Driver »
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let w = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let ok = unsafe { ShellExecuteW(std::ptr::null_mut(), w("runas").as_ptr(), w(&setup.display().to_string()).as_ptr(), std::ptr::null(), w(&d.display().to_string()).as_ptr(), 1) } as isize > 32;
    if ok { Ok("Clique « Install Driver » dans la fenêtre VB-CABLE, puis redémarre le PC.".into()) } else { Err("installateur non lancé".into()) }
}

pub struct Micro {
    r: Arc<Mutex<Reglages>>,
    p: Arc<Mutex<Partage>>,
    stop: Arc<AtomicBool>,
    sorties: Vec<String>,
    install: Arc<Mutex<(bool, String)>>,
}

impl Micro {
    pub fn new() -> Self {
        let r: Reglages = charger("micro");
        let auto = r.auto;
        let mut m = Micro { r: Arc::new(Mutex::new(r)), p: Arc::default(), stop: Arc::new(AtomicBool::new(true)), sorties: vec![], install: Arc::default() };
        if auto {
            m.demarrer();
        }
        m
    }

    fn demarrer(&mut self) {
        self.stop = Arc::new(AtomicBool::new(false));
        let (r, p, s) = (self.r.clone(), self.p.clone(), self.stop.clone());
        *p.lock().unwrap() = Partage::default();
        std::thread::spawn(move || {
            let res = wasapi::boucle(r, p.clone(), s.clone());
            let mut x = p.lock().unwrap();
            x.marche = false;
            x.erreur = res.err();
            s.store(true, Relaxed);
        });
    }
}

impl Outil for Micro {
    fn nom(&self) -> &'static str {
        "🎤 Micro : anti-bruit et voix"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn resume(&self) -> Option<String> {
        let p = self.p.lock().unwrap();
        p.marche.then(|| format!("🎤 Micro traité actif ({})", self.r.lock().unwrap().voix.nom()))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Micro : anti-bruit et changeur de voix");
        ui.weak("Retire le bruit de fond (clavier, ventilateur, rue) grâce à une petite IA, et peut transformer ta voix. Dans Discord / OBS / le jeu, choisis « CABLE Output » comme micro.");
        if self.sorties.is_empty() {
            self.sorties = wasapi::sorties();
        }
        if !cable_installe(&self.sorties) {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Il faut un câble audio virtuel (VB-CABLE, gratuit) : c'est le « faux micro » dans lequel Outils envoie ta voix nettoyée.");
                let (o, m) = self.install.lock().unwrap().clone();
                ui.horizontal(|ui| {
                    if ui.add_enabled(!o, egui::Button::new("📥 Installer VB-CABLE")).clicked() {
                        let i = self.install.clone();
                        *i.lock().unwrap() = (true, "Téléchargement…".into());
                        std::thread::spawn(move || *i.lock().unwrap() = (false, installer_cable().unwrap_or_else(|e| format!("✖ {e}"))));
                    }
                    if ui.small_button("⟳").clicked() {
                        self.sorties.clear();
                    }
                    if o {
                        ui.spinner();
                    }
                    ui.label(m);
                });
            });
        }
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        let marche = !self.stop.load(Relaxed);
        ui.horizontal(|ui| {
            if marche {
                if ui.button(RichText::new("⏹ Arrêter").size(16.0)).clicked() {
                    self.stop.store(true, Relaxed);
                }
            } else if ui.button(RichText::new("▶ Activer").size(16.0)).clicked() {
                self.demarrer();
            }
            ui.checkbox(&mut r.auto, "Activer au lancement d'Outils");
        });
        let p = self.p.lock().unwrap();
        if let Some(e) = &p.erreur {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        if p.marche {
            ui.horizontal(|ui| {
                ui.label("Entrée");
                ui.add(egui::ProgressBar::new(p.niveau_entree.min(1.0)).desired_width(160.0));
                ui.label("Sortie");
                ui.add(egui::ProgressBar::new(p.niveau_sortie.min(1.0)).desired_width(160.0));
                if r.anti_bruit {
                    ui.label(if p.voix_detectee > 0.5 { "🗣 voix" } else { "🤫" });
                }
            });
            ui.ctx().request_repaint_after(Duration::from_millis(60));
        }
        drop(p);
        ui.separator();
        ui.checkbox(&mut r.anti_bruit, "🧹 Anti-bruit IA (RNNoise)");
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.porte, "🚪 Couper sous un seuil");
            if r.porte {
                ui.add(egui::Slider::new(&mut r.seuil, 0.002..=0.2).logarithmic(true).show_value(false));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Volume");
            ui.add(egui::Slider::new(&mut r.volume, 0.2..=3.0).show_value(false));
        });
        ui.checkbox(&mut r.ecoute, "🎧 M'entendre (dans le casque, pour régler)");
        ui.strong("Voix");
        ui.horizontal_wrapped(|ui| {
            for v in Voix::TOUTES {
                ui.selectable_value(&mut r.voix, v, v.nom());
            }
        });
        ui.collapsing("Sortie avancée", |ui| {
            egui::ComboBox::from_id_salt("sortie_micro").selected_text(if r.sortie.is_empty() { "CABLE Input (auto)".to_string() } else { r.sortie.clone() }).show_ui(ui, |ui| {
                ui.selectable_value(&mut r.sortie, String::new(), "CABLE Input (auto)");
                for s in &self.sorties {
                    ui.selectable_value(&mut r.sortie, s.clone(), s);
                }
            });
            ui.weak("Changer la sortie demande d'arrêter puis réactiver.");
        });
        if r != avant {
            enregistrer("micro", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reechantillonnage() {
        let mut r = Reech::new(44_100.0, 48_000.0);
        let mut s = vec![];
        for _ in 0..100 {
            r.traiter(&vec![0.5; 441], &mut s);
        }
        assert!((s.len() as i64 - 48_000).abs() < 5, "{}", s.len());
        assert!(s.iter().skip(1).all(|x| (x - 0.5).abs() < 1e-6));
        let mut r = Reech::new(48_000.0, 48_000.0);
        let mut s = vec![];
        r.traiter(&[1.0, 2.0], &mut s);
        assert_eq!(s, vec![1.0, 2.0]);
    }

    #[test]
    fn effets_bornes() {
        for v in Voix::TOUTES {
            let mut e = Effets::new();
            e.voix = v;
            let mut max = 0f32;
            for i in 0..48_000 {
                let x = (i as f32 * 0.05).sin() * 0.8;
                let y = e.traiter(x);
                assert!(y.is_finite());
                max = max.max(y.abs());
            }
            assert!(max <= 1.0 && max > 0.05, "{v:?} {max}");
        }
        // le changement de hauteur garde à peu près le niveau
        let mut h = Hauteur::new();
        let s: f32 = (0..48_000).map(|i| h.traiter((i as f32 * 0.03).sin(), 1.5).abs()).sum::<f32>() / 48_000.0;
        assert!(s > 0.3 && s < 1.0, "{s}");
        let mut g = 0.0;
        for _ in 0..20 {
            porte(0.5, 0.1, &mut g);
        }
        assert!(g > 0.99);
    }
}
