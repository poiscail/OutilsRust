//! Outils système propres à Linux : infos du PC, processus, espace disque, nettoyage,
//! mises à jour (apt, dnf, pacman, zypper, flatpak, snap) et programmes au démarrage.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn taille(o: u64) -> String {
    let f = o as f64;
    if f >= 1e12 {
        format!("{:.1} To", f / 1e12)
    } else if f >= 1e9 {
        format!("{:.1} Go", f / 1e9)
    } else if f >= 1e6 {
        format!("{:.0} Mo", f / 1e6)
    } else if f >= 1e3 {
        format!("{:.0} Ko", f / 1e3)
    } else {
        format!("{o} o")
    }
}

fn lire(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
}

fn existe(prog: &str) -> bool {
    std::env::var("PATH").unwrap_or_default().split(':').any(|d| Path::new(d).join(prog).is_file())
}

fn sortie(prog: &str, args: &[&str]) -> String {
    Command::new(prog).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default()
}

/// Valeur d'une ligne « clé: valeur » (meminfo, cpuinfo, os-release…).
pub fn champ<'a>(texte: &'a str, cle: &str, sep: char) -> Option<&'a str> {
    texte.lines().find_map(|l| {
        let (k, v) = l.split_once(sep)?;
        (k.trim() == cle).then(|| v.trim().trim_matches('"'))
    })
}

/// Mémoire (total, disponible) en octets, d'après /proc/meminfo.
pub fn memoire(meminfo: &str) -> (u64, u64) {
    let ko = |k: &str| champ(meminfo, k, ':').and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0) * 1024;
    (ko("MemTotal"), ko("MemAvailable"))
}

/// Temps processeur (actif, total) d'après la première ligne de /proc/stat.
pub fn temps_cpu(stat: &str) -> (u64, u64) {
    let v: Vec<u64> = stat.lines().next().unwrap_or("").split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    let total: u64 = v.iter().take(8).sum();
    let repos = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
    (total.saturating_sub(repos), total)
}

// ===================================================================================================
// Infos du PC
// ===================================================================================================

pub struct InfosPc {
    infos: Vec<(String, String)>,
    cpu: (u64, u64, f32),
    vu: Instant,
}

impl InfosPc {
    pub fn new() -> Self {
        InfosPc { infos: vec![], cpu: (0, 0, 0.0), vu: Instant::now() - Duration::from_secs(10) }
    }

    fn lire(&mut self) {
        let mut v = vec![];
        let os = lire("/etc/os-release");
        v.push(("Système".into(), champ(&os, "PRETTY_NAME", '=').unwrap_or("Linux").to_string()));
        let mut u: libc::utsname = unsafe { std::mem::zeroed() };
        unsafe { libc::uname(&mut u) };
        let c = |x: &[libc::c_char]| unsafe { std::ffi::CStr::from_ptr(x.as_ptr()) }.to_string_lossy().to_string();
        v.push(("Noyau".into(), format!("{} ({})", c(&u.release), c(&u.machine))));
        v.push(("Nom du PC".into(), c(&u.nodename)));
        let bureau = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        if !bureau.is_empty() {
            v.push(("Bureau".into(), format!("{bureau} ({session})")));
        }
        let cpu = lire("/proc/cpuinfo");
        let modele = champ(&cpu, "model name", ':').or_else(|| champ(&cpu, "Model", ':')).unwrap_or("?").to_string();
        let fils = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        v.push(("Processeur".into(), format!("{modele} · {fils} fils")));
        let (t, d) = memoire(&lire("/proc/meminfo"));
        v.push(("Mémoire".into(), format!("{} ({} libres)", taille(t), taille(d))));
        if existe("lspci") {
            for l in sortie("lspci", &[]).lines().filter(|l| l.contains("VGA") || l.contains("3D controller") || l.contains("Display controller")) {
                v.push(("Carte graphique".into(), l.split_once(": ").map(|x| x.1).unwrap_or(l).to_string()));
            }
        }
        let up: f64 = lire("/proc/uptime").split_whitespace().next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
        v.push(("Allumé depuis".into(), format!("{} h {:02} min", up as u64 / 3600, up as u64 / 60 % 60)));
        for e in std::fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten() {
            let p = e.path();
            if lire(&p.join("type").display().to_string()).trim() == "Battery" {
                let cap = lire(&p.join("capacity").display().to_string());
                let st = lire(&p.join("status").display().to_string());
                v.push(("Batterie".into(), format!("{} % · {}", cap.trim(), st.trim())));
            }
        }
        let mut temps = vec![];
        for e in std::fs::read_dir("/sys/class/thermal").into_iter().flatten().flatten() {
            let t: f32 = lire(&e.path().join("temp").display().to_string()).trim().parse().unwrap_or(0.0);
            let ty = lire(&e.path().join("type").display().to_string());
            if t > 1000.0 {
                temps.push(format!("{} {:.0} °C", ty.trim(), t / 1000.0));
            }
        }
        if !temps.is_empty() {
            v.push(("Températures".into(), temps.join(" · ")));
        }
        self.infos = v;
    }
}

impl Outil for InfosPc {
    fn nom(&self) -> &'static str {
        "💻 Infos du PC"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.vu.elapsed() > Duration::from_secs(2) {
            self.vu = Instant::now();
            self.lire();
            let (a, t) = temps_cpu(&lire("/proc/stat"));
            if self.cpu.1 > 0 && t > self.cpu.1 {
                self.cpu.2 = (a - self.cpu.0) as f32 / (t - self.cpu.1) as f32 * 100.0;
            }
            self.cpu = (a, t, self.cpu.2);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        ui.heading("Infos du PC");
        egui::Grid::new("infos_linux").num_columns(2).spacing([20.0, 6.0]).striped(true).show(ui, |ui| {
            for (k, v) in &self.infos {
                ui.strong(k);
                ui.label(v);
                ui.end_row();
            }
            ui.strong("Processeur utilisé");
            ui.add(egui::ProgressBar::new(self.cpu.2 / 100.0).desired_width(260.0).text(format!("{:.0} %", self.cpu.2)));
            ui.end_row();
        });
        if ui.button("📋 Copier la fiche").clicked() {
            ui.ctx().copy_text(self.infos.iter().map(|(k, v)| format!("{k} : {v}")).collect::<Vec<_>>().join("\n"));
        }
    }
}

// ===================================================================================================
// Processus
// ===================================================================================================

#[derive(Clone)]
struct Proc {
    pid: i32,
    nom: String,
    ram: u64,
    temps: u64,
    cpu: f32,
    moi: bool,
}

fn processus() -> Vec<Proc> {
    let uid = unsafe { libc::getuid() };
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
    let mut v = vec![];
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
        let stat = lire(&format!("/proc/{pid}/stat"));
        let Some(f) = stat.rfind(')') else { continue };
        let nom = stat[stat.find('(').unwrap_or(0) + 1..f].to_string();
        let c: Vec<&str> = stat[f + 2..].split_whitespace().collect();
        let temps = c.get(11).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0) + c.get(12).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
        let rss = c.get(21).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0) * page;
        let proprietaire = std::fs::metadata(format!("/proc/{pid}")).map(|m| std::os::unix::fs::MetadataExt::uid(&m)).unwrap_or(0);
        v.push(Proc { pid, nom, ram: rss, temps, cpu: 0.0, moi: proprietaire == uid });
    }
    v
}

pub struct Processus {
    liste: Vec<Proc>,
    avant: (HashMap<i32, u64>, u64),
    vu: Instant,
    tri_cpu: bool,
    filtre: String,
    message: String,
}

impl Processus {
    pub fn new() -> Self {
        Processus { liste: vec![], avant: (HashMap::new(), 0), vu: Instant::now() - Duration::from_secs(10), tri_cpu: true, filtre: String::new(), message: String::new() }
    }
}

impl Outil for Processus {
    fn nom(&self) -> &'static str {
        "⚙ Processus"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.vu.elapsed() > Duration::from_secs(2) {
            self.vu = Instant::now();
            let (_, total) = temps_cpu(&lire("/proc/stat"));
            let fils = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
            let mut l = processus();
            let dt = total.saturating_sub(self.avant.1).max(1) as f32;
            for p in &mut l {
                if let Some(a) = self.avant.0.get(&p.pid) {
                    p.cpu = (p.temps.saturating_sub(*a)) as f32 / dt * 100.0 * fils;
                }
            }
            self.avant = (l.iter().map(|p| (p.pid, p.temps)).collect(), total);
            self.liste = l;
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        ui.heading("Processus");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tri_cpu, true, "Par processeur");
            ui.selectable_value(&mut self.tri_cpu, false, "Par mémoire");
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("filtrer").desired_width(160.0));
            ui.label(&self.message);
        });
        let mut l = self.liste.clone();
        let f = self.filtre.to_lowercase();
        l.retain(|p| p.ram > 0 && (f.is_empty() || p.nom.to_lowercase().contains(&f)));
        if self.tri_cpu {
            l.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        } else {
            l.sort_by(|a, b| b.ram.cmp(&a.ram));
        }
        let mut tuer = None;
        egui::ScrollArea::vertical().id_salt("proc").show(ui, |ui| {
            egui::Grid::new("proc_grille").num_columns(5).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                for t in ["Programme", "PID", "Processeur", "Mémoire", ""] {
                    ui.strong(t);
                }
                ui.end_row();
                for p in l.iter().take(120) {
                    ui.label(&p.nom);
                    ui.weak(p.pid.to_string());
                    ui.label(format!("{:.1} %", p.cpu));
                    ui.label(taille(p.ram));
                    if p.moi && ui.small_button("✖ Fermer").clicked() {
                        tuer = Some((p.pid, p.nom.clone()));
                    } else if !p.moi {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((pid, nom)) = tuer {
            let ok = unsafe { libc::kill(pid, libc::SIGTERM) } == 0;
            self.message = if ok { format!("{nom} fermé") } else { format!("Impossible de fermer {nom}") };
        }
    }
}

// ===================================================================================================
// Espace disque
// ===================================================================================================

/// Systèmes de fichiers « réels » (pas les montages virtuels).
pub fn fs_reel(t: &str) -> bool {
    ["ext2", "ext3", "ext4", "btrfs", "xfs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk", "f2fs", "zfs", "jfs", "reiserfs"].contains(&t)
}

fn montages() -> Vec<(String, String, u64, u64)> {
    let mut v = vec![];
    for l in lire("/proc/mounts").lines() {
        let c: Vec<&str> = l.split_whitespace().collect();
        if c.len() < 3 || !fs_reel(c[2]) || c[1].starts_with("/snap") || c[1].starts_with("/boot/efi") {
            continue;
        }
        let point = c[1].replace("\\040", " ");
        let cs = std::ffi::CString::new(point.clone()).unwrap_or_default();
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(cs.as_ptr(), &mut s) } == 0 {
            let total = s.f_blocks as u64 * s.f_frsize as u64;
            let libre = s.f_bavail as u64 * s.f_frsize as u64;
            if total > 0 && !v.iter().any(|x: &(String, String, u64, u64)| x.0 == c[0]) {
                v.push((c[0].to_string(), point, total, libre));
            }
        }
    }
    v
}

fn taille_dossier(p: &Path, stop: &AtomicBool) -> u64 {
    let mut t = 0;
    let mut pile = vec![p.to_path_buf()];
    while let Some(d) = pile.pop() {
        if stop.load(Relaxed) {
            break;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() && !m.file_type().is_symlink() {
                pile.push(e.path());
            } else if m.is_file() {
                t += m.len();
            }
        }
    }
    t
}

pub struct Disques {
    dossiers: Arc<Mutex<Vec<(String, u64)>>>,
    en_cours: Arc<AtomicBool>,
    lance: bool,
}

impl Disques {
    pub fn new() -> Self {
        Disques { dossiers: Arc::default(), en_cours: Arc::default(), lance: false }
    }
}

impl Outil for Disques {
    fn nom(&self) -> &'static str {
        "💽 Espace disque"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Espace disque");
        for (dev, point, total, libre) in montages() {
            let pris = 1.0 - libre as f32 / total.max(1) as f32;
            ui.horizontal(|ui| {
                ui.add_sized([160.0, 18.0], egui::Label::new(RichText::new(&point).strong()));
                ui.add(egui::ProgressBar::new(pris).desired_width(260.0).fill(if pris > 0.9 { Color32::from_rgb(220, 80, 80) } else { ui.visuals().selection.bg_fill }).text(format!("{} libres sur {}", taille(libre), taille(total))));
                ui.weak(dev);
            });
        }
        ui.separator();
        ui.strong("Ce qui prend de la place dans ton dossier personnel");
        if !self.lance {
            self.lance = true;
            let (d, e) = (self.dossiers.clone(), self.en_cours.clone());
            e.store(true, Relaxed);
            std::thread::spawn(move || {
                let stop = AtomicBool::new(false);
                for x in std::fs::read_dir(home()).into_iter().flatten().flatten() {
                    if x.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                        let n = x.file_name().to_string_lossy().to_string();
                        let t = taille_dossier(&x.path(), &stop);
                        let mut g = d.lock().unwrap();
                        g.push((n, t));
                        g.sort_by(|a, b| b.1.cmp(&a.1));
                    }
                }
                e.store(false, Relaxed);
            });
        }
        if self.en_cours.load(Relaxed) {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        let l = self.dossiers.lock().unwrap().clone();
        let max = l.first().map(|x| x.1).unwrap_or(1).max(1);
        for (n, t) in l.iter().take(20) {
            ui.horizontal(|ui| {
                if ui.add_sized([160.0, 18.0], egui::Button::new(n).frame(false)).on_hover_text("Ouvrir").clicked() {
                    crate::outil::ouvrir(&home().join(n));
                }
                ui.add(egui::ProgressBar::new(*t as f32 / max as f32).desired_width(260.0).text(taille(*t)));
            });
        }
    }
}

// ===================================================================================================
// Nettoyage
// ===================================================================================================

struct Element {
    nom: &'static str,
    explication: &'static str,
    chemin: Option<PathBuf>,
    commande: Option<Vec<String>>,
}

fn elements() -> Vec<Element> {
    let h = home();
    let mut v = vec![
        Element { nom: "Corbeille", explication: "les fichiers supprimés", chemin: Some(h.join(".local/share/Trash")), commande: None },
        Element { nom: "Miniatures", explication: "aperçus des images, recréés au besoin", chemin: Some(h.join(".cache/thumbnails")), commande: None },
        Element { nom: "Cache des applis", explication: "fichiers temporaires des applis (navigateurs, etc.), recréés au besoin", chemin: Some(h.join(".cache")), commande: None },
    ];
    if existe("apt-get") {
        v.push(Element { nom: "Paquets téléchargés (apt)", explication: "installateurs déjà installés", chemin: Some("/var/cache/apt/archives".into()), commande: Some(vec!["pkexec".into(), "apt-get".into(), "clean".into()]) });
        v.push(Element { nom: "Paquets inutiles (apt)", explication: "dépendances plus utilisées", chemin: None, commande: Some(vec!["pkexec".into(), "apt-get".into(), "-y".into(), "autoremove".into()]) });
    }
    if existe("dnf") {
        v.push(Element { nom: "Cache de dnf", explication: "paquets téléchargés", chemin: Some("/var/cache/dnf".into()), commande: Some(vec!["pkexec".into(), "dnf".into(), "clean".into(), "all".into()]) });
    }
    if existe("pacman") {
        v.push(Element { nom: "Cache de pacman", explication: "anciennes versions des paquets", chemin: Some("/var/cache/pacman/pkg".into()), commande: Some(vec!["pkexec".into(), "pacman".into(), "-Sc".into(), "--noconfirm".into()]) });
    }
    if existe("flatpak") {
        v.push(Element { nom: "Flatpak inutilisés", explication: "bibliothèques que plus aucune appli n'utilise", chemin: None, commande: Some(vec!["flatpak".into(), "uninstall".into(), "--unused".into(), "-y".into()]) });
    }
    if existe("journalctl") {
        v.push(Element { nom: "Journaux du système", explication: "garde seulement les 200 derniers Mo", chemin: Some("/var/log/journal".into()), commande: Some(vec!["pkexec".into(), "journalctl".into(), "--vacuum-size=200M".into()]) });
    }
    v
}

fn vider(d: &Path) -> u64 {
    let mut libere = 0;
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        let t = if p.is_dir() { taille_dossier(&p, &AtomicBool::new(false)) } else { e.metadata().map(|m| m.len()).unwrap_or(0) };
        let ok = if p.is_dir() { std::fs::remove_dir_all(&p).is_ok() } else { std::fs::remove_file(&p).is_ok() };
        if ok {
            libere += t;
        }
    }
    libere
}

pub struct Nettoyage {
    tailles: Arc<Mutex<HashMap<&'static str, u64>>>,
    journal: Arc<Mutex<Vec<String>>>,
    occupe: Arc<AtomicBool>,
    mesure: bool,
}

impl Nettoyage {
    pub fn new() -> Self {
        Nettoyage { tailles: Arc::default(), journal: Arc::default(), occupe: Arc::default(), mesure: false }
    }

    fn mesurer(&self) {
        let t = self.tailles.clone();
        std::thread::spawn(move || {
            for e in elements() {
                if let Some(c) = &e.chemin {
                    let s = taille_dossier(c, &AtomicBool::new(false));
                    t.lock().unwrap().insert(e.nom, s);
                }
            }
        });
    }
}

impl Outil for Nettoyage {
    fn nom(&self) -> &'static str {
        "🧹 Nettoyage"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Nettoyage");
        ui.weak("Libère de la place sans toucher à tes documents. Les actions sur le système demandent ton mot de passe (fenêtre « pkexec »).");
        if !self.mesure {
            self.mesure = true;
            self.mesurer();
        }
        let t = self.tailles.lock().unwrap().clone();
        let occupe = self.occupe.load(Relaxed);
        let mut faire: Option<(Option<PathBuf>, Option<Vec<String>>, &'static str)> = None;
        egui::Grid::new("nettoyage").num_columns(3).spacing([14.0, 8.0]).show(ui, |ui| {
            for e in elements() {
                ui.vertical(|ui| {
                    ui.strong(e.nom);
                    ui.weak(e.explication);
                });
                ui.label(t.get(e.nom).map(|x| taille(*x)).unwrap_or_default());
                if ui.add_enabled(!occupe, egui::Button::new("🧹 Nettoyer")).clicked() {
                    faire = Some((e.chemin.clone(), e.commande.clone(), e.nom));
                }
                ui.end_row();
            }
        });
        if let Some((chemin, commande, nom)) = faire {
            let (j, o, tl) = (self.journal.clone(), self.occupe.clone(), self.tailles.clone());
            o.store(true, Relaxed);
            std::thread::spawn(move || {
                let r = match (commande, chemin) {
                    (Some(c), _) => match Command::new(&c[0]).args(&c[1..]).output() {
                        Ok(x) if x.status.success() => format!("✔ {nom} : fait"),
                        Ok(x) => format!("✖ {nom} : {}", String::from_utf8_lossy(&x.stderr).lines().last().unwrap_or("annulé")),
                        Err(e) => format!("✖ {nom} : {e}"),
                    },
                    (None, Some(p)) => format!("✔ {nom} : {} libérés", taille(vider(&p))),
                    _ => String::new(),
                };
                j.lock().unwrap().insert(0, r);
                tl.lock().unwrap().remove(nom);
                o.store(false, Relaxed);
            });
        }
        if occupe {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(400));
        }
        for l in self.journal.lock().unwrap().iter() {
            ui.label(l);
        }
    }
}

// ===================================================================================================
// Mises à jour
// ===================================================================================================

/// Gestionnaires présents : (nom, commande « liste », commande « mise à jour »).
fn gestionnaires() -> Vec<(&'static str, Vec<&'static str>, Vec<&'static str>)> {
    let mut v = vec![];
    if existe("apt") {
        v.push(("apt (Debian, Ubuntu, Mint)", vec!["apt", "list", "--upgradable"], vec!["pkexec", "sh", "-c", "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y upgrade"]));
    }
    if existe("dnf") {
        v.push(("dnf (Fedora)", vec!["dnf", "-q", "check-update"], vec!["pkexec", "dnf", "-y", "upgrade"]));
    }
    if existe("pacman") {
        let liste = if existe("checkupdates") { vec!["checkupdates"] } else { vec!["pacman", "-Qu"] };
        v.push(("pacman (Arch, Manjaro)", liste, vec!["pkexec", "pacman", "-Syu", "--noconfirm"]));
    }
    if existe("zypper") {
        v.push(("zypper (openSUSE)", vec!["zypper", "-q", "list-updates"], vec!["pkexec", "zypper", "-n", "update"]));
    }
    if existe("flatpak") {
        v.push(("Flatpak", vec!["flatpak", "remote-ls", "--updates"], vec!["flatpak", "update", "-y"]));
    }
    if existe("snap") {
        v.push(("Snap", vec!["snap", "refresh", "--list"], vec!["pkexec", "snap", "refresh"]));
    }
    v
}

/// Lignes qui sont vraiment des paquets (on enlève les en-têtes de chaque outil).
pub fn paquets(sortie: &str) -> Vec<String> {
    sortie
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("Listing") && !l.starts_with("En train") && !l.starts_with("Name") && !l.starts_with("Last metadata") && !l.starts_with("All snaps") && !l.contains("Loading") && !l.starts_with("S |") && !l.starts_with("--"))
        .map(|l| l.split(['/', ' ', '\t']).next().unwrap_or(l).to_string())
        .collect()
}

pub struct MisesAJour {
    res: Arc<Mutex<Vec<(&'static str, Vec<String>)>>>,
    journal: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
    lance: bool,
}

impl MisesAJour {
    pub fn new() -> Self {
        MisesAJour { res: Arc::default(), journal: Arc::default(), occupe: Arc::default(), lance: false }
    }

    fn verifier(&self) {
        let (r, o) = (self.res.clone(), self.occupe.clone());
        o.store(true, Relaxed);
        std::thread::spawn(move || {
            let mut v = vec![];
            for (n, l, _) in gestionnaires() {
                v.push((n, paquets(&sortie(l[0], &l[1..]))));
            }
            *r.lock().unwrap() = v;
            o.store(false, Relaxed);
        });
    }
}

impl Outil for MisesAJour {
    fn nom(&self) -> &'static str {
        "⬆ Mises à jour"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mises à jour");
        ui.weak("Toutes les mises à jour du système et des applis (paquets, Flatpak, Snap) au même endroit.");
        if !self.lance {
            self.lance = true;
            self.verifier();
        }
        let occupe = self.occupe.load(Relaxed);
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("⟳ Vérifier")).clicked() {
                self.verifier();
            }
            if occupe {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
        });
        let res = self.res.lock().unwrap().clone();
        if res.is_empty() && !occupe {
            ui.label("Aucun gestionnaire de paquets reconnu.");
        }
        let mut lancer: Option<Vec<&'static str>> = None;
        for (n, l) in &res {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.strong(*n);
                    if l.is_empty() {
                        ui.colored_label(Color32::LIGHT_GREEN, "✔ à jour");
                    } else {
                        ui.colored_label(Color32::from_rgb(255, 200, 100), format!("{} mise(s) à jour", l.len()));
                        if ui.add_enabled(!occupe, egui::Button::new("⬆ Tout mettre à jour")).clicked() {
                            lancer = gestionnaires().into_iter().find(|g| g.0 == *n).map(|g| g.2);
                        }
                    }
                });
                if !l.is_empty() {
                    ui.weak(l.iter().take(40).cloned().collect::<Vec<_>>().join(", "));
                }
            });
        }
        if let Some(c) = lancer {
            let (j, o, s) = (self.journal.clone(), self.occupe.clone(), self.res.clone());
            o.store(true, Relaxed);
            *j.lock().unwrap() = "Mise à jour… (ton mot de passe peut être demandé)".into();
            std::thread::spawn(move || {
                let r = Command::new(c[0]).args(&c[1..]).output();
                *j.lock().unwrap() = match r {
                    Ok(x) if x.status.success() => "✔ Mises à jour installées. Redémarre si le noyau a été mis à jour.".into(),
                    Ok(x) => format!("✖ {}", String::from_utf8_lossy(&x.stderr).lines().last().unwrap_or("annulé")),
                    Err(e) => format!("✖ {e}"),
                };
                let mut v = vec![];
                for (n, l, _) in gestionnaires() {
                    v.push((n, paquets(&sortie(l[0], &l[1..]))));
                }
                *s.lock().unwrap() = v;
                o.store(false, Relaxed);
            });
        }
        ui.label(self.journal.lock().unwrap().clone());
    }
}

// ===================================================================================================
// Démarrage
// ===================================================================================================

/// (nom affiché, commande, caché ?) d'un fichier .desktop.
pub fn lire_desktop(t: &str) -> (String, String, bool) {
    let mut nom = String::new();
    let mut exec = String::new();
    let mut cache = false;
    let mut dans_entree = false;
    for l in t.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            dans_entree = l == "[Desktop Entry]";
            continue;
        }
        if !dans_entree {
            continue;
        }
        if let Some(v) = l.strip_prefix("Name=") {
            nom = v.to_string();
        } else if let Some(v) = l.strip_prefix("Exec=") {
            exec = v.to_string();
        } else if l == "Hidden=true" || l == "X-GNOME-Autostart-enabled=false" {
            cache = true;
        }
    }
    (nom, exec, cache)
}

pub struct Demarrage {
    message: String,
}

impl Demarrage {
    pub fn new() -> Self {
        Demarrage { message: String::new() }
    }
}

fn entrees_demarrage() -> Vec<(String, String, String, bool)> {
    // (fichier, nom, commande, actif) : ceux de l'utilisateur remplacent ceux du système
    let mut m: std::collections::BTreeMap<String, (String, String, bool)> = std::collections::BTreeMap::new();
    for d in [PathBuf::from("/etc/xdg/autostart"), home().join(".config/autostart")] {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let f = e.file_name().to_string_lossy().to_string();
            if !f.ends_with(".desktop") {
                continue;
            }
            let (n, x, cache) = lire_desktop(&std::fs::read_to_string(e.path()).unwrap_or_default());
            m.insert(f, (if n.is_empty() { e.file_name().to_string_lossy().to_string() } else { n }, x, !cache));
        }
    }
    m.into_iter().map(|(f, (n, x, a))| (f, n, x, a)).collect()
}

impl Outil for Demarrage {
    fn nom(&self) -> &'static str {
        "🚀 Démarrage"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Programmes lancés à l'ouverture de session");
        ui.weak("Décoche ce qui n'a pas besoin de démarrer avec le PC : la session s'ouvre plus vite.");
        let dossier = home().join(".config/autostart");
        for (f, n, x, actif) in entrees_demarrage() {
            let mut a = actif;
            ui.horizontal(|ui| {
                if ui.checkbox(&mut a, RichText::new(&n).strong()).changed() {
                    let _ = std::fs::create_dir_all(&dossier);
                    let source = [dossier.join(&f), PathBuf::from("/etc/xdg/autostart").join(&f)].into_iter().find(|p| p.is_file());
                    let mut t = source.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_else(|| format!("[Desktop Entry]\nType=Application\nName={n}\nExec={x}\n"));
                    t = t.lines().filter(|l| !l.starts_with("Hidden=") && !l.starts_with("X-GNOME-Autostart-enabled=")).collect::<Vec<_>>().join("\n");
                    if !a {
                        t.push_str("\nHidden=true\n");
                    }
                    self.message = match std::fs::write(dossier.join(&f), t) {
                        Ok(()) => format!("✔ {n} {}", if a { "démarrera avec la session" } else { "ne démarrera plus tout seul" }),
                        Err(e) => format!("✖ {e}"),
                    };
                }
                ui.weak(x.chars().take(70).collect::<String>());
            });
        }
        ui.label(&self.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lectures() {
        let mi = "MemTotal:       16303384 kB\nMemFree:         1234 kB\nMemAvailable:    8000000 kB\n";
        assert_eq!(memoire(mi), (16303384 * 1024, 8000000 * 1024));
        assert_eq!(temps_cpu("cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 2"), (150, 1000));
        assert_eq!(champ("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"", "PRETTY_NAME", '='), Some("Ubuntu 24.04 LTS"));
        assert!(fs_reel("ext4") && !fs_reel("tmpfs"));
        let d = "[Desktop Entry]\nName=Discord\nExec=discord --start-minimized\nX-GNOME-Autostart-enabled=false\n[Desktop Action x]\nName=Autre\n";
        assert_eq!(lire_desktop(d), ("Discord".into(), "discord --start-minimized".into(), true));
        assert_eq!(paquets("Listing... Done\nfirefox/noble-updates 1.2 amd64 [upgradable from: 1.1]\nlibc6/noble 2.39\n"), vec!["firefox", "libc6"]);
        assert_eq!(taille(1_500_000_000), "1.5 Go");
    }
}
