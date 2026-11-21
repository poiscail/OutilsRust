//! Jeux, suite : minuteurs de cooldown avec voix (9), optimisation au lancement d'un jeu (10), journal de jeu (11),
//! comparateur de configs (12), jeux gratuits qui expirent (13), profils de mods Minecraft côté joueur (14).
use crate::interface2::{texte_raccourci, vk, Raccourci};
use crate::outil::{charger, enregistrer, ouvrir, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SANS_FENETRE: u32 = 0x0800_0000;

type Etat = Arc<Mutex<(bool, String)>>;

fn en_fond(e: &Etat, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
    let e = e.clone();
    *e.lock().unwrap() = (true, "En cours…".into());
    std::thread::spawn(move || {
        let r = f();
        *e.lock().unwrap() = (false, r.unwrap_or_else(|m| format!("✖ {m}")));
    });
}

fn etat_ui(ui: &mut egui::Ui, e: &Etat) -> bool {
    let (o, m) = e.lock().unwrap().clone();
    if o {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(&m);
        });
        ui.ctx().request_repaint_after(Duration::from_millis(300));
    } else if !m.is_empty() {
        ui.label(m);
    }
    o
}

fn maintenant() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn dire(t: &str) {
    let r: serde_json::Value = charger("voix");
    let _ = crate::voix::parler(t, r["voix"].as_str().unwrap_or(""), r["vitesse"].as_i64().unwrap_or(0) as i32);
}

/// Lignes ajoutées à l'overlay en jeu (minuteurs en cours, ping).
pub static OVERLAY: Mutex<Vec<(String, [u8; 3])>> = Mutex::new(Vec::new());

// ---------------------------------------------------------------------------------------------------
// 9 · Minuteurs de cooldown
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Minuteur {
    pub nom: String,
    pub secondes: u32,
    pub avant: u32,
    pub raccourci: Option<Raccourci>,
}

impl Default for Minuteur {
    fn default() -> Self {
        Minuteur { nom: "Dragon".into(), secondes: 300, avant: 30, raccourci: None }
    }
}

/// Annonce à faire pour un minuteur selon le temps restant (déjà faites exclues).
pub fn annonce(nom: &str, reste: i64, avant: u32, deja: &mut u8) -> Option<String> {
    if reste <= 0 && *deja < 2 {
        *deja = 2;
        return Some(format!("{nom} prêt"));
    }
    if avant > 0 && reste <= avant as i64 && reste > 0 && *deja < 1 {
        *deja = 1;
        return Some(format!("{nom} dans {avant} secondes"));
    }
    None
}

pub fn mmss(s: i64) -> String {
    format!("{}:{:02}", s.max(0) / 60, s.max(0) % 60)
}

type EnCours = Arc<Mutex<Vec<Option<(Instant, u8)>>>>;

pub struct Minuteurs {
    liste: Arc<Mutex<Vec<Minuteur>>>,
    en_cours: EnCours,
    capture: Option<usize>,
    recharger: Arc<AtomicBool>,
}

impl Minuteurs {
    pub fn new() -> Self {
        let liste: Arc<Mutex<Vec<Minuteur>>> = Arc::new(Mutex::new(charger("minuteurs_jeu")));
        let en_cours: EnCours = Arc::default();
        let recharger = Arc::new(AtomicBool::new(true));
        let (l, e, rc) = (liste.clone(), en_cours.clone(), recharger.clone());
        std::thread::spawn(move || unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let mut nb = 0usize;
            loop {
                if rc.swap(false, Relaxed) {
                    for k in 0..nb {
                        UnregisterHotKey(std::ptr::null_mut(), 1200 + k as i32);
                    }
                    let v = l.lock().unwrap().clone();
                    nb = v.len();
                    for (k, m) in v.iter().enumerate() {
                        if let Some(r) = &m.raccourci {
                            let md = MOD_NOREPEAT | if r.ctrl { MOD_CONTROL } else { 0 } | if r.alt { MOD_ALT } else { 0 } | if r.maj { MOD_SHIFT } else { 0 };
                            RegisterHotKey(std::ptr::null_mut(), 1200 + k as i32, md, r.vk);
                        }
                    }
                }
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), WM_HOTKEY, WM_HOTKEY, PM_REMOVE) != 0 {
                    let k = (msg.wParam as i32 - 1200) as usize;
                    let mut ec = e.lock().unwrap();
                    if ec.len() <= k {
                        ec.resize(k + 1, None);
                    }
                    ec[k] = Some((Instant::now(), 0));
                }
                // annonces et lignes de l'overlay
                let v = l.lock().unwrap().clone();
                let mut ec = e.lock().unwrap();
                let mut lignes = vec![];
                for (k, m) in v.iter().enumerate() {
                    if let Some(Some((t0, deja))) = ec.get_mut(k) {
                        let reste = m.secondes as i64 - t0.elapsed().as_secs() as i64;
                        if let Some(a) = annonce(&m.nom, reste, m.avant, deja) {
                            dire(&a);
                        }
                        if reste > -5 {
                            lignes.push((format!("{} {}", m.nom, if reste > 0 { mmss(reste) } else { "PRÊT".into() }), if reste > 0 { [255, 200, 120] } else { [120, 255, 160] }));
                        }
                    }
                }
                drop(ec);
                let mut o = OVERLAY.lock().unwrap();
                o.retain(|x| !x.0.starts_with('⏱'));
                o.extend(lignes.into_iter().map(|(t, c)| (format!("⏱ {t}"), c)));
                drop(o);
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        Minuteurs { liste, en_cours, capture: None, recharger }
    }
}

impl Outil for Minuteurs {
    fn nom(&self) -> &'static str {
        "⏱ Minuteurs de cooldown"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Minuteurs de cooldown");
        ui.weak("Pour les boss, objectifs, sorts ou respawns : un raccourci lance le compte à rebours pendant le jeu, une voix prévient avant la fin et quand c'est prêt. Les minuteurs s'affichent aussi dans l'overlay en jeu.");
        let avant = self.liste.lock().unwrap().clone();
        let mut v = avant.clone();
        let mut retirer = None;
        let mut lancer = None;
        {
            let ec = self.en_cours.lock().unwrap();
            for (k, m) in v.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut m.nom).desired_width(120.0));
                        let mut mn = m.secondes / 60;
                        let mut s = m.secondes % 60;
                        ui.add(egui::DragValue::new(&mut mn).range(0..=120).suffix(" min"));
                        ui.add(egui::DragValue::new(&mut s).range(0..=59).suffix(" s"));
                        m.secondes = (mn * 60 + s).max(1);
                        ui.label("prévenir");
                        ui.add(egui::DragValue::new(&mut m.avant).range(0..=300).suffix(" s avant"));
                        let t = m.raccourci.as_ref().map(texte_raccourci).unwrap_or("aucun raccourci".into());
                        if ui.button(if self.capture == Some(k) { "⌨ Appuie…".to_string() } else { format!("⌨ {t}") }).clicked() {
                            self.capture = Some(k);
                        }
                        if ui.small_button("🗑").clicked() {
                            retirer = Some(k);
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("▶ Lancer").clicked() {
                            lancer = Some(k);
                        }
                        if let Some(Some((t0, _))) = ec.get(k) {
                            let reste = m.secondes as i64 - t0.elapsed().as_secs() as i64;
                            ui.label(RichText::new(if reste > 0 { mmss(reste) } else { "PRÊT".into() }).size(22.0).color(if reste > 0 { Color32::from_rgb(255, 200, 120) } else { Color32::LIGHT_GREEN }));
                            ui.ctx().request_repaint_after(Duration::from_millis(250));
                        }
                    });
                });
            }
        }
        if let Some(k) = self.capture {
            let touche = ui.input(|i| i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => vk(*key).map(|x| (x, *modifiers)),
                _ => None,
            }));
            if let Some((x, m)) = touche {
                if let Some(mi) = v.get_mut(k) {
                    mi.raccourci = Some(Raccourci { ctrl: m.ctrl, alt: m.alt, maj: m.shift, win: false, vk: x, outil: String::new() });
                }
                self.capture = None;
            }
            ui.ctx().request_repaint();
        }
        if let Some(k) = lancer {
            let mut ec = self.en_cours.lock().unwrap();
            if ec.len() <= k {
                ec.resize(k + 1, None);
            }
            ec[k] = Some((Instant::now(), 0));
        }
        if let Some(k) = retirer {
            v.remove(k);
            self.en_cours.lock().unwrap().clear();
        }
        if ui.button("➕ Nouveau minuteur").clicked() {
            v.push(Minuteur::default());
        }
        ui.weak("Astuce : prends une touche que le jeu n'utilise pas, par exemple Ctrl+F5 ou Alt+F6.");
        if v != avant {
            enregistrer("minuteurs_jeu", &v);
            *self.liste.lock().unwrap() = v;
            self.recharger.store(true, Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 10 · Optimisation au lancement d'un jeu
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageOptim {
    pub actif: bool,
    pub fermer: Vec<String>,
    pub rouvrir: bool,
    pub performances: bool,
    pub vider_ram: bool,
    pub derniere: String,
}

impl Default for ReglageOptim {
    fn default() -> Self {
        ReglageOptim { actif: false, fermer: vec!["msedge.exe".into(), "chrome.exe".into(), "OneDrive.exe".into()], rouvrir: true, performances: true, vider_ram: true, derniere: String::new() }
    }
}

/// Chemins des processus lancés portant ces noms d'exe.
fn processus(noms: &[String]) -> Vec<(String, String)> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    let bas: Vec<String> = noms.iter().map(|n| n.to_lowercase()).collect();
    let mut v = vec![];
    unsafe {
        let s = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(s, &mut e);
        while ok != 0 {
            let l = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(260);
            let n = String::from_utf16_lossy(&e.szExeFile[..l]);
            if bas.contains(&n.to_lowercase()) {
                if let Some(c) = crate::lanceur::chemin_processus(e.th32ProcessID) {
                    if !v.iter().any(|(x, _): &(String, String)| x.eq_ignore_ascii_case(&n)) {
                        v.push((n, c));
                    }
                }
            }
            ok = Process32NextW(s, &mut e);
        }
        windows_sys::Win32::Foundation::CloseHandle(s);
    }
    v
}

fn plan_actuel() -> Option<String> {
    let o = std::process::Command::new("powercfg").arg("/getactivescheme").creation_flags(SANS_FENETRE).output().ok()?;
    let t = String::from_utf8_lossy(&o.stdout).to_string();
    regex::Regex::new(r"([0-9a-fA-F]{8}-[0-9a-fA-F-]{27})").ok()?.captures(&t).map(|c| c[1].to_string())
}

pub struct Optimisation {
    r: Arc<Mutex<ReglageOptim>>,
    ajout: String,
}

impl Optimisation {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageOptim>> = Arc::new(Mutex::new(charger("optim_jeu")));
        let r2 = r.clone();
        std::thread::spawn(move || {
            let mut jeu: Option<String> = None;
            let mut fermes: Vec<(String, String)> = vec![];
            let mut plan: Option<String> = None;
            loop {
                std::thread::sleep(Duration::from_secs(3));
                let c = r2.lock().unwrap().clone();
                let actuel = crate::outil::jeu_actif();
                match (&jeu, &actuel) {
                    (None, Some(j)) if c.actif => {
                        let mut fait = vec![];
                        fermes = processus(&c.fermer);
                        for (n, _) in &fermes {
                            let _ = std::process::Command::new("taskkill").args(["/IM", n, "/F"]).creation_flags(SANS_FENETRE).status();
                        }
                        if !fermes.is_empty() {
                            fait.push(format!("{} programme(s) fermé(s)", fermes.len()));
                        }
                        if c.performances {
                            plan = plan_actuel();
                            let _ = std::process::Command::new("powercfg").args(["/setactive", "SCHEME_MIN"]).creation_flags(SANS_FENETRE).status();
                            fait.push("mode performances".into());
                        }
                        if c.vider_ram && crate::optimiseur::est_admin() && crate::systeme4::vider_veille().is_ok() {
                            fait.push("mémoire vidée".into());
                        }
                        let m = format!("{j} : {}", if fait.is_empty() { "rien à faire".to_string() } else { fait.join(", ") });
                        crate::coeur::noter("Optimisation jeu", &m);
                        let mut x = r2.lock().unwrap();
                        x.derniere = m;
                        enregistrer("optim_jeu", &*x);
                    }
                    (Some(_), None) => {
                        if let Some(p) = plan.take() {
                            let _ = std::process::Command::new("powercfg").args(["/setactive", &p]).creation_flags(SANS_FENETRE).status();
                        }
                        if c.rouvrir {
                            for (_, chemin) in fermes.drain(..) {
                                let _ = std::process::Command::new(chemin).spawn();
                            }
                        }
                        fermes.clear();
                    }
                    _ => {}
                }
                jeu = actuel;
            }
        });
        Optimisation { r, ajout: String::new() }
    }
}

impl Outil for Optimisation {
    fn nom(&self) -> &'static str {
        "🚀 Optimisation au lancement d'un jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Optimisation au lancement d'un jeu");
        ui.weak("Dès qu'un jeu démarre : ferme les programmes gourmands, passe en mode performances et vide la mémoire. Quand tu quittes le jeu, tout revient comme avant.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.checkbox(&mut r.actif, RichText::new("Activer").strong());
        ui.horizontal_wrapped(|ui| {
            ui.label("Fermer :");
            let mut retirer = None;
            for (k, n) in r.fermer.iter().enumerate() {
                if ui.small_button(format!("{n} ✖")).clicked() {
                    retirer = Some(k);
                }
            }
            if let Some(k) = retirer {
                r.fermer.remove(k);
            }
            let t = ui.add(egui::TextEdit::singleline(&mut self.ajout).hint_text("ex. Spotify.exe").desired_width(120.0));
            if t.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.ajout.trim().is_empty() {
                let mut n = std::mem::take(&mut self.ajout).trim().to_string();
                if !n.to_lowercase().ends_with(".exe") {
                    n.push_str(".exe");
                }
                r.fermer.push(n);
            }
        });
        ui.checkbox(&mut r.rouvrir, "Les rouvrir après le jeu");
        ui.checkbox(&mut r.performances, "Mode d'alimentation « performances » pendant le jeu");
        ui.checkbox(&mut r.vider_ram, "Vider la mémoire en veille (Outils en admin)");
        ui.weak("Les jeux sont reconnus grâce au Lanceur Jeux (Steam, Epic, jeux ajoutés à la main).");
        if !r.derniere.is_empty() {
            ui.label(format!("Dernière fois : {}", r.derniere));
        }
        if r != avant {
            let mut x = self.r.lock().unwrap();
            r.derniere = x.derniere.clone();
            *x = r;
            enregistrer("optim_jeu", &*x);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 11 · Journal de jeu
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Entree {
    pub jeu: String,
    pub debut: i64,
    pub minutes: u32,
    pub texte: String,
    pub captures: Vec<String>,
}

fn date_heure(t: i64) -> String {
    let off = crate::pressepapiers::decalage();
    let l = t + off;
    let (a, m, j) = crate::quotidien2::date_de_jours(l.div_euclid(86_400));
    format!("{j:02}/{m:02}/{a} {:02}h{:02}", l.rem_euclid(86_400) / 3600, l.rem_euclid(3600) / 60)
}

/// Captures prises pendant la session (fichiers modifiés entre début et fin).
fn captures_session(debut: i64, fin: i64) -> Vec<String> {
    crate::jeux4::captures().into_values().flatten().filter(|p| {
        std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).is_some_and(|d| (debut..=fin + 60).contains(&(d.as_secs() as i64)))
    }).map(|p| p.display().to_string()).take(20).collect()
}

pub struct Journal {
    l: Arc<Mutex<Vec<Entree>>>,
    filtre: Option<String>,
    texte_libre: String,
}

impl Journal {
    pub fn new() -> Self {
        let l: Arc<Mutex<Vec<Entree>>> = Arc::new(Mutex::new(charger("journal_jeu")));
        let l2 = l.clone();
        std::thread::spawn(move || {
            let mut en_cours: Option<(String, i64)> = None;
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let j = crate::outil::jeu_actif();
                match (&en_cours, &j) {
                    (None, Some(n)) => en_cours = Some((n.clone(), maintenant())),
                    (Some((n, d)), None) => {
                        let fin = maintenant();
                        let minutes = ((fin - d) / 60) as u32;
                        if minutes >= 5 {
                            let e = Entree { jeu: n.clone(), debut: *d, minutes, texte: String::new(), captures: captures_session(*d, fin) };
                            let mut x = l2.lock().unwrap();
                            x.push(e);
                            enregistrer("journal_jeu", &*x);
                            crate::systeme::notifier("Journal de jeu", &format!("{minutes} min sur {n} : note ce que tu as fait (onglet Journal de jeu)."));
                        }
                        en_cours = None;
                    }
                    _ => {}
                }
            }
        });
        Journal { l, filtre: None, texte_libre: String::new() }
    }
}

impl Outil for Journal {
    fn nom(&self) -> &'static str {
        "📔 Journal de jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn resume(&self) -> Option<String> {
        let n = self.l.lock().unwrap().iter().filter(|e| e.texte.trim().is_empty()).count();
        (n > 0).then(|| format!("📔 {n} session(s) de jeu à raconter"))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Journal de jeu");
        ui.weak("Chaque session de plus de 5 min est ajoutée toute seule, avec les captures prises pendant. Écris ce que tu as fait : pratique pour reprendre une partie des mois plus tard.");
        let mut l = self.l.lock().unwrap();
        let avant = l.clone();
        let mut jeux: Vec<String> = l.iter().map(|e| e.jeu.clone()).collect();
        jeux.sort();
        jeux.dedup();
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.filtre.is_none(), "Tous").clicked() {
                self.filtre = None;
            }
            for j in &jeux {
                if ui.selectable_label(self.filtre.as_ref() == Some(j), j).clicked() {
                    self.filtre = Some(j.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.texte_libre).hint_text("ajouter une note à la main (jeu : texte)").desired_width(360.0));
            if ui.button("➕").clicked() && !self.texte_libre.trim().is_empty() {
                let t = std::mem::take(&mut self.texte_libre);
                let (jeu, texte) = t.split_once(':').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).unwrap_or((self.filtre.clone().unwrap_or("Autre".into()), t.trim().to_string()));
                l.push(Entree { jeu, debut: maintenant(), minutes: 0, texte, captures: vec![] });
            }
        });
        let mut retirer = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (k, e) in l.iter_mut().enumerate().rev() {
                if self.filtre.as_ref().is_some_and(|f| *f != e.jeu) {
                    continue;
                }
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&e.jeu).strong().color(Color32::from_rgb(255, 120, 170)));
                        ui.weak(date_heure(e.debut));
                        if e.minutes > 0 {
                            ui.weak(format!("· {} h {:02}", e.minutes / 60, e.minutes % 60));
                        }
                        if ui.small_button("🗑").clicked() {
                            retirer = Some(k);
                        }
                    });
                    ui.add(egui::TextEdit::multiline(&mut e.texte).hint_text("Qu'as-tu fait ? (boss battu, quête, base construite…)").desired_rows(2).desired_width(f32::INFINITY));
                    if !e.captures.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.weak(format!("📷 {} capture(s) :", e.captures.len()));
                            for (i, c) in e.captures.iter().enumerate().take(8) {
                                if ui.small_button(format!("{}", i + 1)).on_hover_text(c).clicked() {
                                    ouvrir(Path::new(c));
                                }
                            }
                        });
                    }
                });
            }
        });
        if let Some(k) = retirer {
            l.remove(k);
        }
        if *l != avant {
            enregistrer("journal_jeu", &*l);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 12 · Comparateur de configs entre deux PC
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct Config {
    pub nom: String,
    pub cpu: String,
    pub coeurs: u32,
    pub threads: u32,
    pub ghz: f64,
    pub ram_go: u32,
    pub ram_mhz: u32,
    pub gpu: String,
    pub vram_go: f64,
    pub ecran: String,
    pub disque: String,
    pub windows: String,
}

const PS_CONFIG: &str = r#"$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$cs = Get-CimInstance Win32_ComputerSystem
$os = Get-CimInstance Win32_OperatingSystem
$g = Get-CimInstance Win32_VideoController | Sort-Object AdapterRAM -Descending | Select-Object -First 1
$vram = 0
try { $k = Get-ItemProperty 'HKLM:\SYSTEM\ControlSet001\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0*' -Name 'HardwareInformation.qwMemorySize' -ErrorAction SilentlyContinue | Select-Object -First 1; if ($k) { $vram = [double]$k.'HardwareInformation.qwMemorySize' } } catch {}
if (-not $vram) { $vram = [double]$g.AdapterRAM }
$v = Get-CimInstance Win32_VideoController | Where-Object CurrentHorizontalResolution | Select-Object -First 1
$d = Get-PhysicalDisk | Sort-Object Size -Descending | Select-Object -First 1
ConvertTo-Json -Compress -InputObject ([pscustomobject]@{ nom = $env:COMPUTERNAME; cpu = $cpu.Name.Trim(); coeurs = [int]$cpu.NumberOfCores; threads = [int]$cpu.NumberOfLogicalProcessors; ghz = [math]::Round($cpu.MaxClockSpeed/1000, 2); ram_go = [int][math]::Round($cs.TotalPhysicalMemory/1GB); ram_mhz = [int](Get-CimInstance Win32_PhysicalMemory | Select-Object -First 1).Speed; gpu = $g.Name; vram_go = [math]::Round($vram/1GB, 1); ecran = "$($v.CurrentHorizontalResolution)x$($v.CurrentVerticalResolution) $($v.CurrentRefreshRate) Hz"; disque = "$($d.FriendlyName) $($d.MediaType)"; windows = "$($os.Caption) $($os.BuildNumber)" })"#;

/// Code à partager : la config en JSON compact, en base64.
pub fn code_config(c: &Config) -> String {
    use base64::Engine;
    format!("OUTILS-PC:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(serde_json::to_vec(c).unwrap_or_default()))
}

pub fn lire_code(s: &str) -> Option<Config> {
    use base64::Engine;
    let b = s.trim().strip_prefix("OUTILS-PC:")?;
    serde_json::from_slice(&base64::engine::general_purpose::STANDARD_NO_PAD.decode(b.trim_end_matches('=')).ok()?).ok()
}

/// Qui gagne sur une ligne numérique : -1 moi, 1 l'autre, 0 égalité.
pub fn meilleur(a: f64, b: f64) -> i8 {
    if (a - b).abs() < 1e-9 { 0 } else if a > b { -1 } else { 1 }
}

pub struct Comparateur {
    moi: Arc<Mutex<Option<Result<Config, String>>>>,
    code: String,
    autre: Option<Config>,
    message: String,
}

impl Comparateur {
    pub fn new() -> Self {
        Comparateur { moi: Arc::default(), code: String::new(), autre: charger::<Option<Config>>("config_ami"), message: String::new() }
    }
}

impl Outil for Comparateur {
    fn nom(&self) -> &'static str {
        "🆚 Comparer avec le PC d'un pote"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Comparer ton PC avec celui d'un pote");
        ui.weak("Copie ton code et envoie-le (Discord…). Ton pote colle le sien ici : le tableau montre qui a quoi de mieux.");
        if self.moi.lock().unwrap().is_none() {
            *self.moi.lock().unwrap() = Some(Err("lecture de ta config…".into()));
            let m = self.moi.clone();
            std::thread::spawn(move || *m.lock().unwrap() = Some(crate::optimiseur::ps(PS_CONFIG).and_then(|s| serde_json::from_str::<Config>(s.trim()).map_err(|e| e.to_string()))));
        }
        let moi = match self.moi.lock().unwrap().clone() {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(e);
                });
                ui.ctx().request_repaint_after(Duration::from_millis(400));
                return;
            }
            None => return,
        };
        ui.horizontal(|ui| {
            if ui.button("📋 Copier mon code").clicked() {
                ui.ctx().copy_text(code_config(&moi));
                self.message = "Code copié : colle-le à ton pote.".into();
            }
            let r = ui.add(egui::TextEdit::singleline(&mut self.code).hint_text("colle ici le code de ton pote (OUTILS-PC:…)").desired_width(320.0));
            if r.changed() {
                match lire_code(&self.code) {
                    Some(c) => {
                        enregistrer("config_ami", &Some(c.clone()));
                        self.autre = Some(c);
                        self.message = "✔ Config reçue.".into();
                        self.code.clear();
                    }
                    None if self.code.trim().len() > 12 => self.message = "Code illisible : recopie-le en entier.".into(),
                    None => {}
                }
            }
        });
        ui.label(&self.message);
        let a = self.autre.clone().unwrap_or_default();
        let deux = self.autre.is_some();
        let vert = Color32::LIGHT_GREEN;
        egui::Grid::new("configs").striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
            ui.label("");
            ui.strong(format!("💻 {} (toi)", moi.nom));
            ui.strong(if deux { format!("💻 {}", a.nom) } else { "— (colle un code)".into() });
            ui.end_row();
            let ligne = |ui: &mut egui::Ui, nom: &str, x: String, y: String, sx: f64, sy: f64| {
                let m = if deux { meilleur(sx, sy) } else { 0 };
                ui.label(nom);
                ui.label(if m == -1 { RichText::new(x).color(vert).strong() } else { RichText::new(x) });
                ui.label(if m == 1 { RichText::new(y).color(vert).strong() } else { RichText::new(y) });
                ui.end_row();
            };
            ligne(ui, "Processeur", moi.cpu.clone(), a.cpu.clone(), 0.0, 0.0);
            ligne(ui, "Cœurs / threads", format!("{} / {}", moi.coeurs, moi.threads), format!("{} / {}", a.coeurs, a.threads), moi.threads as f64, a.threads as f64);
            ligne(ui, "Fréquence max", format!("{} GHz", moi.ghz), format!("{} GHz", a.ghz), moi.ghz, a.ghz);
            ligne(ui, "Mémoire", format!("{} Go à {} MHz", moi.ram_go, moi.ram_mhz), format!("{} Go à {} MHz", a.ram_go, a.ram_mhz), moi.ram_go as f64 * 10_000.0 + moi.ram_mhz as f64, a.ram_go as f64 * 10_000.0 + a.ram_mhz as f64);
            ligne(ui, "Carte graphique", moi.gpu.clone(), a.gpu.clone(), 0.0, 0.0);
            ligne(ui, "Mémoire vidéo", format!("{} Go", moi.vram_go), format!("{} Go", a.vram_go), moi.vram_go, a.vram_go);
            ligne(ui, "Écran", moi.ecran.clone(), a.ecran.clone(), 0.0, 0.0);
            ligne(ui, "Disque principal", moi.disque.clone(), a.disque.clone(), 0.0, 0.0);
            ligne(ui, "Windows", moi.windows.clone(), a.windows.clone(), 0.0, 0.0);
        });
        if deux {
            ui.weak("En vert : le meilleur des deux. Pour la carte graphique et le processeur, compare leurs modèles sur un site de benchmark.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 13 · Jeux gratuits qui vont expirer
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Offre {
    pub id: u64,
    pub titre: String,
    pub plateformes: String,
    pub valeur: String,
    pub fin: i64, // 0 : pas de date
    pub url: String,
    pub image: String,
}

/// « 2026-11-27 23:59:00 » (heure de la plateforme, prise comme UTC) → secondes.
pub fn lire_fin(s: &str) -> i64 {
    let Some((a, m, j)) = crate::quotidien2::lire_date(s) else { return 0 };
    let h = s.get(11..13).and_then(|x| x.parse::<i64>().ok()).unwrap_or(23);
    let mn = s.get(14..16).and_then(|x| x.parse::<i64>().ok()).unwrap_or(59);
    crate::quotidien2::jours_depuis_1970(a, m, j) * 86_400 + h * 3600 + mn * 60
}

pub fn reste_texte(s: i64) -> String {
    match s {
        s if s <= 0 => "terminé".into(),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86_400 => format!("{} h", s / 3600),
        s => format!("{} j {} h", s / 86_400, s % 86_400 / 3600),
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct DonneesOffres {
    offres: Vec<Offre>,
    recuperes: Vec<u64>,
    prevenus: Vec<u64>,
    derniere: i64,
}

fn charger_offres() -> Result<Vec<Offre>, String> {
    let j: serde_json::Value = ureq::get("https://www.gamerpower.com/api/giveaways").timeout(Duration::from_secs(20)).query("type", "game").query("sort-by", "date")
        .call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    Ok(j.as_array().into_iter().flatten().filter_map(|g| Some(Offre {
        id: g["id"].as_u64()?,
        titre: g["title"].as_str()?.replace(" (Epic Games) Giveaway", "").replace(" Giveaway", ""),
        plateformes: g["platforms"].as_str().unwrap_or("").to_string(),
        valeur: g["worth"].as_str().unwrap_or("").to_string(),
        fin: lire_fin(g["end_date"].as_str().unwrap_or("")),
        url: g["open_giveaway_url"].as_str().unwrap_or("").to_string(),
        image: g["thumbnail"].as_str().unwrap_or("").to_string(),
    })).collect())
}

pub struct Expirent {
    d: Arc<Mutex<DonneesOffres>>,
    tout: bool,
}

impl Expirent {
    pub fn new() -> Self {
        let d: Arc<Mutex<DonneesOffres>> = Arc::new(Mutex::new(charger("gratuits_expirent")));
        let d2 = d.clone();
        std::thread::spawn(move || loop {
            let t = maintenant();
            if t - d2.lock().unwrap().derniere > 3 * 3600 {
                if let Ok(v) = charger_offres() {
                    let mut x = d2.lock().unwrap();
                    x.offres = v;
                    x.derniere = t;
                    enregistrer("gratuits_expirent", &*x);
                }
            }
            // rappel 24 h avant la fin d'une offre pas encore récupérée
            {
                let mut x = d2.lock().unwrap();
                let a_prevenir: Vec<(u64, String, i64)> = x.offres.iter().filter(|o| o.fin > t && o.fin - t < 86_400 && !x.recuperes.contains(&o.id) && !x.prevenus.contains(&o.id)).map(|o| (o.id, o.titre.clone(), o.fin - t)).collect();
                for (id, titre, reste) in a_prevenir {
                    crate::systeme::notifier_toujours("⏳ Jeu gratuit bientôt fini", &format!("{titre} : plus que {}", reste_texte(reste)));
                    x.prevenus.push(id);
                }
                if x.prevenus.len() > 500 {
                    let n = x.prevenus.len() - 500;
                    x.prevenus.drain(..n);
                }
            }
            std::thread::sleep(Duration::from_secs(900));
        });
        Expirent { d, tout: false }
    }
}

impl Outil for Expirent {
    fn nom(&self) -> &'static str {
        "⏳ Jeux gratuits qui expirent"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn resume(&self) -> Option<String> {
        let d = self.d.lock().unwrap();
        let t = maintenant();
        let n = d.offres.iter().filter(|o| o.fin > t && o.fin - t < 2 * 86_400 && !d.recuperes.contains(&o.id)).count();
        (n > 0).then(|| format!("⏳ {n} jeu(x) gratuit(s) finissent dans moins de 2 jours"))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Jeux gratuits qui expirent");
        ui.weak("Les jeux offerts (Epic, Steam, GOG, Prime Gaming, Itch…) triés par date de fin. Coche ceux que tu as récupérés : Outils te prévient 24 h avant la fin des autres.");
        let mut d = self.d.lock().unwrap();
        let avant = d.recuperes.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.tout, "Afficher aussi ceux sans date de fin");
            if d.offres.is_empty() {
                ui.spinner();
                ui.weak("chargement…");
                ui.ctx().request_repaint_after(Duration::from_secs(1));
            }
        });
        let t = maintenant();
        let mut v: Vec<Offre> = d.offres.iter().filter(|o| (o.fin > t) || (self.tout && o.fin == 0)).cloned().collect();
        v.sort_by_key(|o| if o.fin == 0 { i64::MAX } else { o.fin });
        egui::ScrollArea::vertical().show(ui, |ui| {
            for o in v {
                ui.horizontal(|ui| {
                    let mut r = d.recuperes.contains(&o.id);
                    if ui.checkbox(&mut r, "").on_hover_text("Récupéré").changed() {
                        if r {
                            d.recuperes.push(o.id);
                        } else {
                            d.recuperes.retain(|x| *x != o.id);
                        }
                    }
                    if !o.image.is_empty() {
                        ui.add(egui::Image::new(o.image.as_str()).fit_to_exact_size(egui::vec2(96.0, 45.0)));
                    }
                    ui.vertical(|ui| {
                        let titre = RichText::new(&o.titre).strong();
                        if ui.link(if r { titre.strikethrough() } else { titre }).clicked() {
                            let _ = std::process::Command::new("explorer").arg(&o.url).spawn();
                        }
                        ui.horizontal(|ui| {
                            if o.fin > 0 {
                                let reste = o.fin - t;
                                ui.colored_label(if reste < 86_400 { Color32::LIGHT_RED } else if reste < 3 * 86_400 { Color32::from_rgb(250, 210, 90) } else { Color32::GRAY }, format!("⏳ {}", reste_texte(reste)));
                            }
                            ui.weak(format!("{} · {}", o.plateformes, o.valeur));
                        });
                    });
                });
                ui.separator();
            }
        });
        if d.recuperes != avant {
            enregistrer("gratuits_expirent", &*d);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 14 · Profils de mods Minecraft (côté joueur)
// ---------------------------------------------------------------------------------------------------

fn dossier_mc() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(".minecraft")
}

fn dossier_profils() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("Outils").join("profils_mods")
}

/// Versions de Minecraft avec chargeur de mods installées : (chargeur, version du jeu), ex. ("fabric", "1.21.1").
pub fn chargeur_de_version(nom: &str) -> Option<(String, String)> {
    let n = nom.to_lowercase();
    if let Some(r) = n.strip_prefix("fabric-loader-") {
        return Some(("fabric".into(), r.rsplit('-').next()?.to_string()));
    }
    if let Some(r) = n.strip_prefix("quilt-loader-") {
        return Some(("quilt".into(), r.rsplit('-').next()?.to_string()));
    }
    if n.starts_with("neoforge-") {
        // neoforge-21.1.77 → 1.21.1
        let v: Vec<&str> = n.trim_start_matches("neoforge-").split('.').collect();
        return Some(("neoforge".into(), format!("1.{}{}", v.first()?, v.get(1).filter(|x| **x != "0").map(|x| format!(".{x}")).unwrap_or_default())));
    }
    if n.contains("-forge-") {
        return Some(("forge".into(), n.split("-forge-").next()?.to_string()));
    }
    None
}

fn versions_installees() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(dossier_mc().join("versions")).into_iter().flatten().flatten().filter_map(|e| chargeur_de_version(&e.file_name().to_string_lossy())).collect();
    v.sort();
    v.dedup();
    v
}

fn jars(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".jar")).collect();
    v.sort();
    v
}

/// Remplace le contenu de .minecraft/mods par celui du profil (les mods actuels sont gardés dans « avant_profil »).
fn activer(nom: &str) -> Result<String, String> {
    let mods = dossier_mc().join("mods");
    let src = dossier_profils().join(nom);
    let garde = dossier_profils().join("_mods_avant_outils");
    std::fs::create_dir_all(&mods).map_err(|e| e.to_string())?;
    let actif: Option<String> = charger("profil_mods_actif");
    // le premier passage garde les mods d'origine
    if actif.is_none() && !jars(&mods).is_empty() && !garde.exists() {
        std::fs::create_dir_all(&garde).map_err(|e| e.to_string())?;
        for j in jars(&mods) {
            let _ = std::fs::copy(mods.join(&j), garde.join(&j));
        }
    }
    for j in jars(&mods) {
        let _ = std::fs::remove_file(mods.join(j));
    }
    let mut n = 0;
    for j in jars(&src) {
        std::fs::copy(src.join(&j), mods.join(&j)).map_err(|e| e.to_string())?;
        n += 1;
    }
    enregistrer("profil_mods_actif", &Some(nom.to_string()));
    Ok(format!("✔ Profil « {nom} » activé ({n} mods). Lance Minecraft avec la bonne version."))
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct InfoProfil {
    pub chargeur: String,
    pub version: String,
}

pub struct ModsJoueur {
    choisi: Option<String>,
    nouveau: String,
    recherche: String,
    res: Arc<Mutex<Option<Result<Vec<(String, String, String)>, String>>>>,
    e: Etat,
}

impl ModsJoueur {
    pub fn new() -> Self {
        ModsJoueur { choisi: None, nouveau: String::new(), recherche: String::new(), res: Arc::default(), e: Arc::default() }
    }
}

fn profils() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dossier_profils()).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('_')).collect();
    v.sort();
    v
}

impl Outil for ModsJoueur {
    fn nom(&self) -> &'static str {
        "🧩 Mes profils de mods Minecraft"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Profils de mods Minecraft (pour jouer)");
        ui.weak("Plusieurs ensembles de mods pour ton jeu (optimisation, aventure, créatif…) : un clic les met dans .minecraft/mods. Il faut Fabric, Forge ou NeoForge installé dans le launcher.");
        let versions = versions_installees();
        if versions.is_empty() {
            ui.colored_label(Color32::from_rgb(250, 210, 90), "Aucune version moddée trouvée dans .minecraft/versions : installe d'abord Fabric (fabricmc.net) ou NeoForge.");
        }
        let actif: Option<String> = charger("profil_mods_actif");
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal_wrapped(|ui| {
            for p in profils() {
                let info: InfoProfil = charger(&format!("profils_mods_{p}"));
                let t = format!("{}{p}  ({} {})", if actif.as_ref() == Some(&p) { "✔ " } else { "" }, info.chargeur, info.version);
                if ui.selectable_label(self.choisi.as_ref() == Some(&p), t).clicked() {
                    self.choisi = Some(p.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nouveau).hint_text("nouveau profil").desired_width(140.0));
            egui::ComboBox::from_id_salt("version_mods").selected_text("pour la version…").show_ui(ui, |ui| {
                for (c, v) in &versions {
                    if ui.button(format!("{c} {v}")).clicked() && !self.nouveau.trim().is_empty() {
                        let n: String = self.nouveau.trim().chars().map(|x| if r#"\/:*?"<>|"#.contains(x) { '_' } else { x }).collect();
                        let _ = std::fs::create_dir_all(dossier_profils().join(&n));
                        enregistrer(&format!("profils_mods_{n}"), &InfoProfil { chargeur: c.clone(), version: v.clone() });
                        self.choisi = Some(n);
                        self.nouveau.clear();
                        ui.close_menu();
                    }
                }
            });
            if ui.button("📥 Copier les mods actuels dans un profil").clicked() && !self.nouveau.trim().is_empty() {
                let n = self.nouveau.trim().to_string();
                let d = dossier_profils().join(&n);
                let _ = std::fs::create_dir_all(&d);
                for j in jars(&dossier_mc().join("mods")) {
                    let _ = std::fs::copy(dossier_mc().join("mods").join(&j), d.join(&j));
                }
                if let Some((c, v)) = versions.last() {
                    enregistrer(&format!("profils_mods_{n}"), &InfoProfil { chargeur: c.clone(), version: v.clone() });
                }
                self.choisi = Some(n);
                self.nouveau.clear();
            }
        });
        let Some(p) = self.choisi.clone() else { return };
        let d = dossier_profils().join(&p);
        let info: InfoProfil = charger(&format!("profils_mods_{p}"));
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading(&p);
            ui.weak(format!("{} {}", info.chargeur, info.version));
            if ui.add_enabled(!occupe, egui::Button::new(RichText::new("▶ Activer ce profil").strong())).clicked() {
                let n = p.clone();
                en_fond(&self.e, move || activer(&n));
            }
            if ui.small_button("📂").clicked() {
                ouvrir(&d);
            }
        });
        let mut retirer = None;
        for j in jars(&d) {
            ui.horizontal(|ui| {
                ui.label(format!("🧩 {j}"));
                if ui.small_button("✖").clicked() {
                    retirer = Some(j.clone());
                }
            });
        }
        if let Some(j) = retirer {
            let _ = std::fs::remove_file(d.join(j));
        }
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("chercher un mod (sodium, journeymap, jei…)").desired_width(280.0));
            if ui.button("🔍").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                let (q, res, c, v) = (self.recherche.clone(), self.res.clone(), info.chargeur.clone(), info.version.clone());
                *res.lock().unwrap() = Some(Err("recherche…".into()));
                std::thread::spawn(move || {
                    let f = format!("[[\"project_type:mod\"],[\"categories:{c}\"],[\"versions:{v}\"]]");
                    let r = ureq::get("https://api.modrinth.com/v2/search").query("query", &q).query("facets", &f).query("limit", "15").call().map_err(|e| e.to_string())
                        .and_then(|r| r.into_json::<serde_json::Value>().map_err(|e| e.to_string()))
                        .map(|j| j["hits"].as_array().into_iter().flatten().filter_map(|h| Some((h["project_id"].as_str()?.to_string(), h["title"].as_str()?.to_string(), h["description"].as_str().unwrap_or("").to_string()))).collect());
                    *res.lock().unwrap() = Some(r);
                });
            }
        });
        if let Some(Ok(v)) = self.res.lock().unwrap().clone() {
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for (id, t, desc) in v {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!occupe, egui::Button::new("📥")).clicked() {
                            let (c, ve, dd) = (info.chargeur.clone(), info.version.clone(), d.clone());
                            en_fond(&self.e, move || crate::mcplus::installer_projet(&id, &c, &ve, &dd).map(|n| format!("✔ {n} ajouté au profil")));
                        }
                        ui.strong(t);
                        ui.weak(desc.chars().take(80).collect::<String>());
                    });
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        let mut d = 0u8;
        assert_eq!(annonce("Dragon", 40, 30, &mut d), None);
        assert_eq!(annonce("Dragon", 29, 30, &mut d), Some("Dragon dans 30 secondes".into()));
        assert_eq!(annonce("Dragon", 20, 30, &mut d), None);
        assert_eq!(annonce("Dragon", 0, 30, &mut d), Some("Dragon prêt".into()));
        assert_eq!(annonce("Dragon", -3, 30, &mut d), None);
        assert_eq!(mmss(125), "2:05");
        let c = Config { nom: "PC".into(), cpu: "Ryzen".into(), ram_go: 16, ..Default::default() };
        assert_eq!(lire_code(&code_config(&c)), Some(c));
        assert_eq!(lire_code("n'importe quoi"), None);
        assert_eq!(meilleur(16.0, 32.0), 1);
        assert_eq!(lire_fin("1970-01-02 01:00:00"), 86_400 + 3600);
        assert_eq!(lire_fin("N/A"), 0);
        assert_eq!(reste_texte(90_000), "1 j 1 h");
        assert_eq!(chargeur_de_version("fabric-loader-0.16.5-1.21.1"), Some(("fabric".into(), "1.21.1".into())));
        assert_eq!(chargeur_de_version("1.20.1-forge-47.3.0"), Some(("forge".into(), "1.20.1".into())));
        assert_eq!(chargeur_de_version("neoforge-21.1.77"), Some(("neoforge".into(), "1.21.1".into())));
        assert_eq!(chargeur_de_version("neoforge-21.0.10"), Some(("neoforge".into(), "1.21".into())));
        assert_eq!(chargeur_de_version("1.21.1"), None);
    }
}
