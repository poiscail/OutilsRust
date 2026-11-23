//! Outils lui-même, suite : nouveautés après une mise à jour (1), profils Jeu / Travail / Nuit (2),
//! recherche partout (3), raccourcis pour ouvrir un outil (4), sauvegarde des réglages sur clé USB (5),
//! mode grand écran à la manette (6).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SANS_FENETRE: u32 = 0x0800_0000;

/// Outil à ouvrir (par son nom), demandé par un raccourci, la recherche ou un profil.
pub static OUVRIR: Mutex<Option<String>> = Mutex::new(None);

/// Noms de tous les outils (remplis par la fenêtre principale au démarrage).
pub static NOMS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn noms() -> Vec<String> {
    NOMS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn ouvrir_outil(nom: &str) {
    *OUVRIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(nom.to_string());
    crate::systeme::montrer();
}

// ---------------------------------------------------------------------------------------------------
// 1 · Nouveautés
// ---------------------------------------------------------------------------------------------------

pub const NOUVEAUTES: &str = include_str!("../NOUVEAUTES.md");

/// Section « ## version » du journal des nouveautés.
pub fn section(journal: &str, version: &str) -> Option<String> {
    let entete = format!("## {version}");
    let debut = journal.find(&entete)?;
    let reste = &journal[debut + entete.len()..];
    let fin = reste.find("\n## ").unwrap_or(reste.len());
    Some(reste[..fin].trim().to_string())
}

/// Vrai au premier lancement d'une nouvelle version (la fenêtre « Quoi de neuf » s'affiche).
pub fn nouvelle_version() -> bool {
    let vue: Option<String> = charger("version_vue");
    let v = env!("CARGO_PKG_VERSION");
    if vue.as_deref() == Some(v) {
        return false;
    }
    enregistrer("version_vue", &Some(v.to_string()));
    vue.is_some() // pas au tout premier lancement : il y a déjà la visite guidée
}

pub fn fenetre_nouveautes(ctx: &egui::Context, ouvert: &mut bool) {
    let texte = section(NOUVEAUTES, env!("CARGO_PKG_VERSION")).unwrap_or_default();
    egui::Window::new(format!("✨ Quoi de neuf dans la version {}", env!("CARGO_PKG_VERSION"))).open(ouvert).default_width(480.0).collapsible(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for l in texte.lines() {
                let l = l.trim();
                if let Some(t) = l.strip_prefix("### ") {
                    ui.add_space(6.0);
                    ui.strong(t);
                } else if let Some(t) = l.strip_prefix("- ") {
                    ui.label(format!("• {t}"));
                } else if !l.is_empty() {
                    ui.label(l);
                }
            }
        });
    });
}

// ---------------------------------------------------------------------------------------------------
// 2 · Profils
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Profil {
    pub nom: String,
    pub emoji: String,
    pub theme: Option<String>,
    pub volume: Option<u32>,
    pub alimentation: Option<u8>, // 0 économie, 1 équilibré, 2 performances
    pub lumiere_bleue: Option<bool>,
    pub notifications: Option<bool>,
    pub stream: Option<bool>,
    pub ouvrir: Vec<String>,
    pub fermer: Vec<String>,
    pub outil: Option<String>,
}

impl Default for Profil {
    fn default() -> Self {
        Profil { nom: String::new(), emoji: "⭐".into(), theme: None, volume: None, alimentation: None, lumiere_bleue: None, notifications: None, stream: None, ouvrir: vec![], fermer: vec![], outil: None }
    }
}

fn profils_de_base() -> Vec<Profil> {
    vec![
        Profil { nom: "Jeu".into(), emoji: "🎮".into(), theme: Some("Néon Tokyo".into()), volume: Some(70), alimentation: Some(2), lumiere_bleue: Some(false), notifications: Some(false), fermer: vec!["msedge.exe".into()], ..Default::default() },
        Profil { nom: "Travail".into(), emoji: "📚".into(), theme: Some("Clair".into()), volume: Some(30), alimentation: Some(1), lumiere_bleue: Some(false), notifications: Some(true), ..Default::default() },
        Profil { nom: "Nuit".into(), emoji: "🌙".into(), theme: Some("Sakura nuit".into()), volume: Some(15), alimentation: Some(0), lumiere_bleue: Some(true), notifications: Some(false), ..Default::default() },
    ]
}

pub fn volume_maitre(v: f32) -> Result<(), String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.message())?;
        let d = en.GetDefaultAudioEndpoint(eRender, eMultimedia).map_err(|e| e.message())?;
        let vol: IAudioEndpointVolume = d.Activate(CLSCTX_ALL, None).map_err(|e| e.message())?;
        vol.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null()).map_err(|e| e.message())
    }
}

/// Applique un profil ; renvoie ce qui a été fait.
pub fn appliquer_profil(p: &Profil) -> Vec<String> {
    let mut fait = vec![];
    if let Some(t) = &p.theme {
        *crate::theme::DEMANDE.lock().unwrap_or_else(|e| e.into_inner()) = Some(t.clone());
        fait.push(format!("thème {t}"));
    }
    if let Some(v) = p.volume {
        if volume_maitre(v as f32 / 100.0).is_ok() {
            fait.push(format!("volume {v} %"));
        }
    }
    if let Some(a) = p.alimentation {
        // SCHEME_MAX = économie, SCHEME_BALANCED, SCHEME_MIN = performances
        let plan = ["SCHEME_MAX", "SCHEME_BALANCED", "SCHEME_MIN"][a.min(2) as usize];
        let _ = std::process::Command::new("powercfg").args(["/setactive", plan]).creation_flags(SANS_FENETRE).status();
        fait.push(["économie d'énergie", "équilibré", "performances"][a.min(2) as usize].to_string());
    }
    if let Some(b) = p.lumiere_bleue {
        crate::systeme4::forcer_lumiere(b);
        fait.push(if b { "filtre de lumière bleue".into() } else { "lumière normale".into() });
    }
    if let Some(n) = p.notifications {
        crate::systeme::NOTIFS.store(n, Relaxed);
        fait.push(if n { "notifications".into() } else { "sans notifications".into() });
    }
    if let Some(s) = p.stream {
        crate::outil::STREAM.store(s, Relaxed);
        if s {
            fait.push("mode stream".into());
        }
    }
    for f in &p.fermer {
        let _ = std::process::Command::new("taskkill").args(["/IM", f]).creation_flags(SANS_FENETRE).status();
    }
    if !p.fermer.is_empty() {
        fait.push(format!("{} programme(s) fermé(s)", p.fermer.len()));
    }
    for o in &p.ouvrir {
        let _ = std::process::Command::new("cmd").args(["/c", "start", "", o]).creation_flags(SANS_FENETRE).spawn();
    }
    if let Some(o) = &p.outil {
        ouvrir_outil(o);
    }
    enregistrer("profil_actif", &Some(p.nom.clone()));
    crate::coeur::noter("Profil", &format!("{} {} : {}", p.emoji, p.nom, fait.join(", ")));
    fait
}

pub struct Profils {
    liste: Vec<Profil>,
    choisi: usize,
    message: String,
    nouveau: String,
}

impl Profils {
    pub fn new() -> Self {
        let mut liste: Vec<Profil> = charger("profils");
        if liste.is_empty() {
            liste = profils_de_base();
        }
        Profils { liste, choisi: 0, message: String::new(), nouveau: String::new() }
    }

}

fn option_ui<T: Clone + PartialEq>(ui: &mut egui::Ui, nom: &str, v: &mut Option<T>, defaut: T, f: impl FnOnce(&mut egui::Ui, &mut T)) {
    ui.horizontal(|ui| {
        let mut on = v.is_some();
        if ui.checkbox(&mut on, nom).changed() {
            *v = on.then(|| defaut.clone());
        }
        if let Some(x) = v {
            f(ui, x);
        }
    });
}

impl Outil for Profils {
    fn nom(&self) -> &'static str {
        "🎛 Profils (Jeu, Travail, Nuit)"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn resume(&self) -> Option<String> {
        let a: Option<String> = charger("profil_actif");
        let p = self.liste.iter().find(|p| Some(&p.nom) == a.as_ref())?;
        Some(format!("{} Profil {}", p.emoji, p.nom))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Profils");
        ui.weak("Un clic change tout d'un coup : thème, volume, mode d'alimentation, lumière, notifications, programmes ouverts ou fermés.");
        let avant = self.liste.clone();
        let actif: Option<String> = charger("profil_actif");
        let mut appliquer = None;
        ui.horizontal_wrapped(|ui| {
            for (k, p) in self.liste.iter().enumerate() {
                let t = RichText::new(format!("{} {}", p.emoji, p.nom)).size(20.0);
                let b = egui::Button::new(if Some(&p.nom) == actif.as_ref() { t.strong() } else { t }).min_size(egui::vec2(130.0, 56.0)).selected(self.choisi == k);
                let r = ui.add(b).on_hover_text("Clic : appliquer · clic droit : modifier");
                if r.clicked() {
                    appliquer = Some(k);
                }
                if r.secondary_clicked() {
                    self.choisi = k;
                }
            }
        });
        if let Some(k) = appliquer {
            self.choisi = k;
            self.message = format!("✔ {} : {}", self.liste[k].nom, appliquer_profil(&self.liste[k]).join(", "));
        }
        ui.label(&self.message);
        ui.separator();
        let themes = crate::theme::noms_themes();
        let noms_outils = noms();
        let mut supprimer = false;
        if let Some(p) = self.liste.get_mut(self.choisi) {
            ui.horizontal(|ui| {
                ui.strong("Modifier :");
                ui.add(egui::TextEdit::singleline(&mut p.emoji).desired_width(30.0));
                ui.add(egui::TextEdit::singleline(&mut p.nom).desired_width(140.0));
                if ui.small_button("🗑 Supprimer").clicked() {
                    supprimer = true;
                }
            });
            option_ui(ui, "Thème", &mut p.theme, themes.first().cloned().unwrap_or_default(), |ui, t| {
                egui::ComboBox::from_id_salt("theme_profil").selected_text(t.as_str()).show_ui(ui, |ui| {
                    for n in &themes {
                        ui.selectable_value(t, n.clone(), n);
                    }
                });
            });
            option_ui(ui, "Volume", &mut p.volume, 50, |ui, v| {
                ui.add(egui::Slider::new(v, 0..=100).suffix(" %"));
            });
            option_ui(ui, "Alimentation", &mut p.alimentation, 1, |ui, a| {
                for (k, n) in ["économie", "équilibré", "performances"].iter().enumerate() {
                    ui.selectable_value(a, k as u8, *n);
                }
            });
            option_ui(ui, "Lumière bleue", &mut p.lumiere_bleue, true, |ui, b| {
                ui.checkbox(b, "filtre actif");
            });
            option_ui(ui, "Notifications d'Outils", &mut p.notifications, true, |ui, b| {
                ui.checkbox(b, "activées");
            });
            option_ui(ui, "Mode stream", &mut p.stream, true, |ui, b| {
                ui.checkbox(b, "masquer les onglets privés");
            });
            option_ui(ui, "Ouvrir l'outil", &mut p.outil, noms_outils.first().cloned().unwrap_or_default(), |ui, o| {
                egui::ComboBox::from_id_salt("outil_profil").width(240.0).selected_text(o.as_str()).show_ui(ui, |ui| {
                    for n in &noms_outils {
                        ui.selectable_value(o, n.clone(), n);
                    }
                });
            });
            for (titre, liste, aide) in [("Programmes à lancer", &mut p.ouvrir, "chemin ou adresse"), ("Programmes à fermer", &mut p.fermer, "ex. Discord.exe")] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(titre);
                    let mut retirer = None;
                    for (k, x) in liste.iter().enumerate() {
                        if ui.small_button(format!("{} ✖", Path::new(x).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(x.clone()))).clicked() {
                            retirer = Some(k);
                        }
                    }
                    if let Some(k) = retirer {
                        liste.remove(k);
                    }
                    if titre.starts_with("Programmes à lancer") {
                        if ui.small_button("➕").clicked() {
                            if let Some(f) = rfd::FileDialog::new().add_filter("Programme", &["exe", "lnk", "url", "bat"]).pick_file() {
                                liste.push(f.display().to_string());
                            }
                        }
                    } else {
                        let id = ui.id().with(titre);
                        let mut t: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                        let r = ui.add(egui::TextEdit::singleline(&mut t).hint_text(aide).desired_width(120.0));
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !t.trim().is_empty() {
                            let mut n = t.trim().to_string();
                            if !n.to_lowercase().ends_with(".exe") {
                                n.push_str(".exe");
                            }
                            liste.push(n);
                            t.clear();
                        }
                        ui.data_mut(|d| d.insert_temp(id, t));
                    }
                });
            }
        }
        if supprimer && self.liste.len() > 1 {
            self.liste.remove(self.choisi);
            self.choisi = 0;
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nouveau).hint_text("nouveau profil").desired_width(140.0));
            if ui.button("➕ Créer").clicked() && !self.nouveau.trim().is_empty() {
                self.liste.push(Profil { nom: std::mem::take(&mut self.nouveau).trim().to_string(), ..Default::default() });
                self.choisi = self.liste.len() - 1;
            }
        });
        if self.liste != avant {
            enregistrer("profils", &self.liste);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 3 · Recherche partout
// ---------------------------------------------------------------------------------------------------

/// Nom lisible d'un fichier de données d'Outils (« agenda.json » → outil).
pub fn source_lisible(fichier: &str) -> &'static str {
    match fichier.trim_end_matches(".json") {
        "notes" => "📝 Notes",
        "agenda" => "📅 Agenda",
        "ecole" => "🎒 Devoirs et emploi du temps",
        "rebours" => "⏳ Comptes à rebours",
        "raccourcis_texte" => "✍ Raccourcis de texte",
        "kanban" => "🗂 Tableau Kanban",
        "taches" | "listes" => "✅ Tâches",
        "lieux_minecraft" => "🧮 Lieux Minecraft",
        "calculs" => "🧮 Calculatrice",
        "dictee" => "🎙 Dictée",
        "flux_rss" => "📰 Actus",
        "musique" => "🎵 Musique",
        "radios" => "📻 Radio",
        "pseudos" => "🏷 Pseudos",
        "mes_themes" => "🎨 Thèmes",
        "profils" => "🎛 Profils",
        _ => "📦 Données d'Outils",
    }
}

/// Textes d'un JSON contenant la recherche (sans les clés ni les nombres).
pub fn textes_trouves(v: &serde_json::Value, q: &str, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) if crate::fichiers3::simple(s).contains(q) && s.chars().count() > 1 => out.push(s.chars().take(160).collect()),
        serde_json::Value::Array(a) => a.iter().for_each(|x| textes_trouves(x, q, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| textes_trouves(x, q, out)),
        _ => {}
    }
}

const PS_RECHERCHE: &str = r#"param($q)
$c = New-Object -ComObject ADODB.Connection
$c.Open("Provider=Search.CollatorDSO;Extended Properties='Application=Windows';")
$q = $q.Replace("'", "''")
$r = $c.Execute("SELECT TOP 40 System.ItemPathDisplay FROM SYSTEMINDEX WHERE (System.FileName LIKE '%$q%' OR CONTAINS('""$q*""')) AND SCOPE='file:'")
$o = @(); while (-not $r.EOF) { $o += $r.Fields.Item('System.ItemPathDisplay').Value; $r.MoveNext() }
ConvertTo-Json -Compress -InputObject @($o)"#;

fn chercher_fichiers(q: &str) -> Vec<String> {
    let script = PS_RECHERCHE.replacen("param($q)", &format!("$q = '{}'", q.replace('\'', "''")), 1);
    crate::optimiseur::ps(&script).ok().and_then(|s| serde_json::from_str::<Vec<String>>(s.trim()).ok()).unwrap_or_default()
}

pub struct RecherchePartout {
    q: String,
    donnees: Vec<(String, Vec<String>)>,
    fichiers: Arc<Mutex<Option<Vec<String>>>>,
    derniere: String,
}

impl RecherchePartout {
    pub fn new() -> Self {
        RecherchePartout { q: String::new(), donnees: vec![], fichiers: Arc::default(), derniere: String::new() }
    }


    fn chercher(&mut self) {
        let q = crate::fichiers3::simple(self.q.trim());
        self.derniere = self.q.clone();
        self.donnees.clear();
        if q.chars().count() < 2 {
            return;
        }
        let d = crate::outil::base();
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !n.ends_with(".json") || ["utilisation.json", "notifications.json", "version_vue.json"].contains(&n.as_str()) {
                continue;
            }
            let Some(v) = std::fs::read_to_string(e.path()).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) else { continue };
            let mut t = vec![];
            textes_trouves(&v, &q, &mut t);
            t.dedup();
            if !t.is_empty() {
                self.donnees.push((n, t));
            }
        }
        let (f, qq) = (self.fichiers.clone(), self.q.trim().to_string());
        *f.lock().unwrap() = None;
        std::thread::spawn(move || *f.lock().unwrap() = Some(chercher_fichiers(&qq)));
    }
}

impl Outil for RecherchePartout {
    fn nom(&self) -> &'static str {
        "🔎 Recherche partout"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Recherche partout");
        let r = ui.add(egui::TextEdit::singleline(&mut self.q).hint_text("un mot : il est cherché dans les outils, tes notes, l'agenda, les devoirs… et les fichiers du PC").font(egui::TextStyle::Heading).desired_width(f32::INFINITY));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || (r.changed() && self.q.chars().count() >= 3) {
            self.chercher();
        }
        if self.derniere.trim().chars().count() < 2 {
            ui.weak("Tape au moins deux lettres, puis Entrée.");
            return;
        }
        let q = crate::fichiers3::simple(self.derniere.trim());
        egui::ScrollArea::vertical().show(ui, |ui| {
            let tous = noms();
            let outils: Vec<&String> = tous.iter().filter(|n| crate::fichiers3::simple(n).contains(&q)).collect();
            if !outils.is_empty() {
                ui.strong("🧰 Outils");
                ui.horizontal_wrapped(|ui| {
                    for n in outils {
                        if ui.button(n.as_str()).clicked() {
                            ouvrir_outil(n);
                        }
                    }
                });
                ui.add_space(6.0);
            }
            for (f, t) in &self.donnees {
                ui.strong(source_lisible(f));
                for x in t.iter().take(8) {
                    ui.label(format!("   {x}"));
                }
                ui.add_space(4.0);
            }
            ui.strong("📁 Fichiers du PC");
            match self.fichiers.lock().unwrap().clone() {
                None => {
                    ui.spinner();
                    ui.ctx().request_repaint_after(Duration::from_millis(300));
                }
                Some(v) if v.is_empty() => {
                    ui.weak("Aucun fichier trouvé (seuls les dossiers indexés par Windows sont fouillés : Documents, Bureau, Images…).");
                }
                Some(v) => {
                    for p in v {
                        ui.horizontal(|ui| {
                            if ui.link(Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(p.clone())).clicked() {
                                crate::outil::ouvrir(Path::new(&p));
                            }
                            ui.weak(Path::new(&p).parent().map(|x| x.display().to_string()).unwrap_or_default());
                        });
                    }
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 4 · Raccourcis pour ouvrir un outil
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Raccourci {
    pub ctrl: bool,
    pub alt: bool,
    pub maj: bool,
    pub win: bool,
    pub vk: u32,
    pub outil: String,
}

/// Code de touche Windows d'une touche egui (lettres, chiffres, F1–F24, pavé).
pub fn vk(k: egui::Key) -> Option<u32> {
    use egui::Key::*;
    let n = k.name();
    if n.len() == 1 {
        let c = n.chars().next()?.to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Some(c as u32);
        }
    }
    if let Some(f) = n.strip_prefix('F').and_then(|x| x.parse::<u32>().ok()).filter(|x| (1..=24).contains(x)) {
        return Some(0x70 + f - 1);
    }
    Some(match k {
        Space => 0x20,
        Home => 0x24,
        End => 0x23,
        PageUp => 0x21,
        PageDown => 0x22,
        Insert => 0x2D,
        _ => return None,
    })
}

pub fn texte_raccourci(r: &Raccourci) -> String {
    let mut s = vec![];
    if r.ctrl {
        s.push("Ctrl".to_string());
    }
    if r.alt {
        s.push("Alt".into());
    }
    if r.maj {
        s.push("Maj".into());
    }
    if r.win {
        s.push("Win".into());
    }
    s.push(match r.vk {
        0x70..=0x87 => format!("F{}", r.vk - 0x6F),
        0x20 => "Espace".into(),
        0x21 => "Page↑".into(),
        0x22 => "Page↓".into(),
        0x23 => "Fin".into(),
        0x24 => "Origine".into(),
        0x2D => "Inser".into(),
        0x13 => "Pause".into(),
        v => char::from_u32(v).map(|c| c.to_string()).unwrap_or("?".into()),
    });
    s.join("+")
}

/// Ctrl+Alt (= AltGr) + chiffre ou certaines lettres bloque la frappe en AZERTY.
pub fn dangereux(r: &Raccourci) -> bool {
    r.ctrl && r.alt && !r.maj && !r.win && ((0x30..=0x39).contains(&r.vk) || r.vk == 'E' as u32)
}

static RECHARGER: AtomicBool = AtomicBool::new(false);

fn fil_raccourcis() {
    std::thread::spawn(|| unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let mut actifs: Vec<Raccourci> = vec![];
        loop {
            // (ré)enregistrement à la demande
            if RECHARGER.swap(false, Relaxed) || actifs.is_empty() {
                for k in 0..actifs.len() {
                    UnregisterHotKey(std::ptr::null_mut(), 900 + k as i32);
                }
                actifs = charger("raccourcis_outils");
                for (k, r) in actifs.iter().enumerate() {
                    let m = MOD_NOREPEAT | if r.ctrl { MOD_CONTROL } else { 0 } | if r.alt { MOD_ALT } else { 0 } | if r.maj { MOD_SHIFT } else { 0 } | if r.win { MOD_WIN } else { 0 };
                    crate::outil::raccourci(std::ptr::null_mut(), 900 + k as i32, m, r.vk);
                }
            }
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), WM_HOTKEY, WM_HOTKEY, PM_REMOVE) != 0 {
                if let Some(r) = actifs.get((msg.wParam as i32 - 900) as usize) {
                    ouvrir_outil(&r.outil);
                }
            }
            std::thread::sleep(Duration::from_millis(60));
        }
    });
}

pub struct RaccourcisOutils {
    liste: Vec<Raccourci>,
    capture: bool,
    choisi: String,
    message: String,
}

impl RaccourcisOutils {
    pub fn new() -> Self {
        fil_raccourcis();
        RaccourcisOutils { liste: charger("raccourcis_outils"), capture: false, choisi: String::new(), message: String::new() }
    }

}

const EXISTANTS: [(&str, &str); 16] = [
    ("Ctrl+Alt+O", "Afficher Outils"),
    ("Ctrl+Alt+K", "Palette de commandes"),
    ("Ctrl+Alt+S / W / P", "Capture zone / fenêtre / écran"),
    ("Ctrl+Alt+C / M", "Pipette / règle"),
    ("Ctrl+Alt+T", "Traduction instantanée"),
    ("Ctrl+Alt+L", "Lecture à voix haute"),
    ("Ctrl+Alt+H", "Overlay en jeu"),
    ("Ctrl+Alt+X", "Viseur"),
    ("Ctrl+Alt+A", "Sortie audio suivante"),
    ("Ctrl+Alt+J", "Fin du chargement d'un jeu"),
    ("Ctrl+Alt+Y", "Corriger l'orthographe du texte sélectionné"),
    ("Ctrl+Alt+F", "Recherche de fichiers sur tout le PC"),
    ("Ctrl+Alt+F1…F11", "Macros"),
    ("Ctrl+Alt+pavé numérique", "Ranger les fenêtres"),
    ("Ctrl+Maj+F1…F9", "Boîte à sons"),
    ("Ctrl+Alt+Page↑/↓", "Transparence des fenêtres"),
];

impl Outil for RaccourcisOutils {
    fn nom(&self) -> &'static str {
        "⌨ Raccourcis clavier"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Raccourcis clavier");
        ui.weak("Donne un raccourci à n'importe quel outil : il s'ouvre de partout, même pendant un jeu.");
        let avant = self.liste.clone();
        let mut retirer = None;
        for (k, r) in self.liste.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(texte_raccourci(r)).monospace().strong());
                ui.label("→");
                ui.label(&r.outil);
                if dangereux(r) {
                    ui.colored_label(Color32::from_rgb(250, 210, 90), "⚠ bloque AltGr en AZERTY");
                }
                if ui.small_button("🗑").clicked() {
                    retirer = Some(k);
                }
            });
        }
        if let Some(k) = retirer {
            self.liste.remove(k);
        }
        ui.separator();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("outil_raccourci").width(260.0).selected_text(if self.choisi.is_empty() { "choisir un outil" } else { &self.choisi }).show_ui(ui, |ui| {
                for n in &noms() {
                    ui.selectable_value(&mut self.choisi, n.clone(), n);
                }
            });
            if ui.add_enabled(!self.choisi.is_empty(), egui::Button::new(if self.capture { "⌨ Appuie sur la combinaison…" } else { "Choisir la combinaison" }).selected(self.capture)).clicked() {
                self.capture = !self.capture;
            }
        });
        if self.capture {
            let touche = ui.input(|i| i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => vk(*key).map(|v| (v, *modifiers)),
                _ => None,
            }));
            if let Some((v, m)) = touche {
                let r = Raccourci { ctrl: m.ctrl, alt: m.alt, maj: m.shift, win: false, vk: v, outil: self.choisi.clone() };
                if !(r.ctrl || r.alt || r.win) && !(0x70..=0x87).contains(&v) {
                    self.message = "Ajoute Ctrl ou Alt (ou prends une touche F1–F12), sinon la touche ne servirait plus qu'à ça.".into();
                } else if EXISTANTS.iter().any(|(t, _)| *t == texte_raccourci(&r).replace("Maj", "Maj")) || self.liste.iter().any(|x| x.ctrl == r.ctrl && x.alt == r.alt && x.maj == r.maj && x.vk == r.vk) {
                    self.message = format!("{} est déjà utilisé.", texte_raccourci(&r));
                } else {
                    self.message = format!("✔ {} ouvre maintenant « {} »", texte_raccourci(&r), r.outil);
                    self.liste.push(r);
                }
                self.capture = false;
            }
            ui.ctx().request_repaint();
        }
        ui.label(&self.message);
        if self.liste != avant {
            enregistrer("raccourcis_outils", &self.liste);
            RECHARGER.store(true, Relaxed);
        }
        ui.separator();
        ui.collapsing("Raccourcis déjà utilisés par Outils", |ui| {
            egui::Grid::new("existants").striped(true).show(ui, |ui| {
                for (t, d) in EXISTANTS {
                    ui.label(RichText::new(t).monospace());
                    ui.label(d);
                    ui.end_row();
                }
            });
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 5 · Sauvegarde des réglages sur clé USB
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageCle {
    pub serie: u32,
    pub nom: String,
    pub garder: usize,
    pub derniere: String,
}

impl Default for ReglageCle {
    fn default() -> Self {
        ReglageCle { serie: 0, nom: String::new(), garder: 5, derniere: String::new() }
    }
}

fn lecteurs_amovibles() -> Vec<(String, u32, String)> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    let m = unsafe { GetLogicalDrives() };
    (0..26u8).filter(|i| m & (1 << i) != 0).filter_map(|i| {
        let d = format!("{}:\\", (b'A' + i) as char);
        let w: Vec<u16> = d.encode_utf16().chain([0]).collect();
        if unsafe { GetDriveTypeW(w.as_ptr()) } != 2 {
            return None; // DRIVE_REMOVABLE
        }
        let mut nom = [0u16; 261];
        let mut serie = 0u32;
        let ok = unsafe { GetVolumeInformationW(w.as_ptr(), nom.as_mut_ptr(), 261, &mut serie, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) };
        (ok != 0).then(|| (d, serie, String::from_utf16_lossy(&nom).trim_end_matches('\0').to_string()))
    }).collect()
}

fn sauver_sur(lettre: &str, garder: usize) -> Result<String, String> {
    let base = PathBuf::from(std::env::var("APPDATA").map_err(|e| e.to_string())?);
    let d = PathBuf::from(lettre).join("Outils - sauvegardes");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let f = d.join(format!("Outils {}.zip", crate::minecraft::horodatage()));
    let n = crate::coeur::exporter(&base, &f)?;
    let mut v: Vec<PathBuf> = std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "zip")).collect();
    v.sort();
    while v.len() > garder.max(1) {
        let _ = std::fs::remove_file(v.remove(0));
    }
    Ok(format!("✔ {n} fichiers sauvegardés sur {lettre}"))
}

pub struct SauvegardeCle {
    r: Arc<Mutex<ReglageCle>>,
    message: Arc<Mutex<String>>,
}

impl SauvegardeCle {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageCle>> = Arc::new(Mutex::new(charger("sauvegarde_cle")));
        let message: Arc<Mutex<String>> = Arc::default();
        let (r2, m2) = (r.clone(), message.clone());
        std::thread::spawn(move || {
            let mut presente = false;
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let c = r2.lock().unwrap().clone();
                if c.serie == 0 {
                    continue;
                }
                let l = lecteurs_amovibles().into_iter().find(|x| x.1 == c.serie);
                if let (Some((lettre, ..)), false) = (&l, presente) {
                    let m = sauver_sur(lettre, c.garder).unwrap_or_else(|e| format!("✖ {e}"));
                    crate::systeme::notifier("Réglages sauvegardés", &m);
                    *m2.lock().unwrap() = m.clone();
                    let mut x = r2.lock().unwrap();
                    x.derniere = format!("{} — {m}", crate::minecraft::horodatage());
                    enregistrer("sauvegarde_cle", &*x);
                }
                presente = l.is_some();
            }
        });
        SauvegardeCle { r, message }
    }
}

impl Outil for SauvegardeCle {
    fn nom(&self) -> &'static str {
        "🔑 Réglages sur clé USB"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sauvegarder les réglages d'Outils sur une clé USB");
        ui.weak("Dès que ta clé est branchée, tous tes réglages et données d'Outils (notes, agenda, thèmes, coffre chiffré…) y sont copiés. Pour les remettre : outil « Sauvegarde, synchro, portable » › Restaurer.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        let l = lecteurs_amovibles();
        if l.is_empty() {
            ui.label("Branche ta clé USB pour la choisir.");
        }
        for (lettre, serie, nom) in &l {
            ui.horizontal(|ui| {
                let choisie = r.serie == *serie;
                if ui.selectable_label(choisie, format!("🔑 {lettre} {nom}")).clicked() {
                    r.serie = *serie;
                    r.nom = if nom.is_empty() { lettre.clone() } else { nom.clone() };
                }
                if ui.small_button("Sauvegarder maintenant").clicked() {
                    *self.message.lock().unwrap() = sauver_sur(lettre, r.garder).unwrap_or_else(|e| format!("✖ {e}"));
                }
            });
        }
        if r.serie != 0 {
            ui.label(format!("Clé choisie : {} (sauvegarde automatique à chaque branchement)", r.nom));
            ui.horizontal(|ui| {
                ui.label("Garder les");
                ui.add(egui::DragValue::new(&mut r.garder).range(1..=30));
                ui.label("dernières sauvegardes");
                if ui.small_button("Ne plus sauvegarder").clicked() {
                    r.serie = 0;
                }
            });
        }
        if !r.derniere.is_empty() {
            ui.weak(format!("Dernière : {}", r.derniere));
        }
        ui.label(self.message.lock().unwrap().clone());
        if r != avant {
            let mut x = self.r.lock().unwrap();
            r.derniere = x.derniere.clone();
            *x = r;
            enregistrer("sauvegarde_cle", &*x);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(3));
    }
}

// ---------------------------------------------------------------------------------------------------
// 6 · Mode grand écran (TV) à la manette
// ---------------------------------------------------------------------------------------------------

/// Boutons XInput utiles.
pub mod boutons {
    pub const HAUT: u16 = 0x0001;
    pub const BAS: u16 = 0x0002;
    pub const GAUCHE: u16 = 0x0004;
    pub const DROITE: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LB: u16 = 0x0100;
    pub const RB: u16 = 0x0200;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
}

/// (boutons, défilement vertical du stick droit -1..1) de la première manette branchée.
pub fn lire_manette() -> Option<(u16, f32)> {
    use windows_sys::Win32::UI::Input::XboxController::*;
    for i in 0..4 {
        let mut s: XINPUT_STATE = unsafe { std::mem::zeroed() };
        if unsafe { XInputGetState(i, &mut s) } == 0 {
            let y = s.Gamepad.sThumbRY as f32 / 32767.0;
            return Some((s.Gamepad.wButtons, if y.abs() > 0.25 { y } else { 0.0 }));
        }
    }
    None
}

/// Boutons qui viennent d'être appuyés.
pub fn appuyes(avant: u16, maintenant: u16) -> u16 {
    maintenant & !avant
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        let j = "# Nouveautés\n\n## 0.9.0\n### Jeux\n- a\n- b\n\n## 0.8.0\n- vieux\n";
        assert_eq!(section(j, "0.9.0").unwrap(), "### Jeux\n- a\n- b");
        assert_eq!(section(j, "0.8.0").unwrap(), "- vieux");
        assert!(section(j, "1.0.0").is_none());
        let r = Raccourci { ctrl: true, alt: true, maj: false, win: false, vk: '3' as u32, outil: "x".into() };
        assert!(dangereux(&r));
        assert_eq!(texte_raccourci(&r), "Ctrl+Alt+3");
        assert_eq!(texte_raccourci(&Raccourci { vk: 0x71, maj: true, ..r.clone() }), "Ctrl+Alt+Maj+F2");
        assert!(!dangereux(&Raccourci { maj: true, ..r }));
        assert_eq!(vk(egui::Key::K), Some('K' as u32));
        assert_eq!(vk(egui::Key::F5), Some(0x74));
        assert_eq!(vk(egui::Key::Num7), Some('7' as u32));
        assert_eq!(appuyes(0b0101, 0b0110), 0b0010);
        let v: serde_json::Value = serde_json::from_str(r#"{"a":[{"titre":"Réunion Minecraft","n":3},"rien"],"b":"minecraft"}"#).unwrap();
        let mut t = vec![];
        textes_trouves(&v, "minecraft", &mut t);
        assert_eq!(t.len(), 2);
        assert_eq!(source_lisible("agenda.json"), "📅 Agenda");
    }
}
