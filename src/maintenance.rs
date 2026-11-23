//! Maintenance automatique : une fois par semaine, la nuit (ou au prochain moment calme si le PC
//! était éteint), Outils nettoie, met à jour, vérifie les disques et fait un point de restauration.
//! Un bilan est gardé, et envoyé en notification (et au téléphone associé).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const JOURS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub actif: bool,
    pub jour: u8,  // 0 = lundi
    pub heure: u8, // 0..23
    pub nettoyage: bool,
    pub mises_a_jour: bool,
    pub disques: bool,
    pub optimiser_disques: bool,
    pub restauration: bool,
    pub reveiller: bool,
    pub derniere: u64,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { actif: false, jour: 6, heure: 3, nettoyage: true, mises_a_jour: false, disques: true, optimiser_disques: true, restauration: true, reveiller: false, derniere: 0 }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Bilan {
    pub quand: String,
    pub lignes: Vec<(bool, String)>,
}

fn maintenant() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// (jour de la semaine 0 = lundi, heure) en heure locale.
fn jour_heure() -> (u8, u8) {
    let mut t = unsafe { std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    (((t.wDayOfWeek + 6) % 7) as u8, t.wHour as u8)
}

/// Faut-il lancer la maintenance maintenant ?
/// À l'heure prévue ; ou en rattrapage si la dernière date de plus de 8 jours et que le PC est inutilisé.
pub fn a_lancer(r: &Reglages, jour: u8, heure: u8, maintenant: u64, inactif_s: u64, jeu: bool) -> bool {
    if !r.actif || jeu {
        return false;
    }
    let depuis = maintenant.saturating_sub(r.derniere);
    if depuis < 20 * 3600 {
        return false;
    }
    (jour == r.jour && heure == r.heure && inactif_s > 300) || (depuis > 8 * 86400 && inactif_s > 900)
}

fn cmd(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new(prog).args(args).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    if o.status.success() { Ok(s) } else { Err(s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("échec").trim().to_string()) }
}

pub fn executer(r: &Reglages, etat: &Mutex<String>) -> Bilan {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
    unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) }; // pas de mise en veille pendant ce temps
    let mut l = vec![];
    let etape = |t: &str| *etat.lock().unwrap() = t.to_string();
    if r.restauration {
        etape("Point de restauration…");
        l.push(match crate::optimiseur::point_restauration() {
            Ok(()) => (true, "Point de restauration créé".to_string()),
            Err(e) => (false, format!("Point de restauration : {e}")),
        });
    }
    if r.nettoyage {
        etape("Nettoyage…");
        l.push((true, crate::optimiseur::nettoyage_rapide()));
    }
    if r.mises_a_jour {
        etape("Mises à jour des logiciels…");
        l.push(match crate::majlogiciels::winget(&["upgrade", "--all", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"]) {
            Ok(s) => {
                let n = s.lines().filter(|x| x.contains("Installé") || x.contains("Successfully installed")).count();
                (true, format!("Logiciels : {n} mis à jour"))
            }
            Err(e) => (false, format!("Mises à jour : {e}")),
        });
    }
    if r.disques {
        etape("Santé des disques…");
        l.push(match crate::optimiseur::ps("Get-PhysicalDisk | ForEach-Object { $_.FriendlyName + '|' + $_.HealthStatus }") {
            Ok(s) => {
                let mauvais: Vec<String> = s.lines().filter_map(|x| x.split_once('|')).filter(|(_, h)| !h.trim().eq_ignore_ascii_case("healthy")).map(|(n, h)| format!("{n} ({h})")).collect();
                if mauvais.is_empty() { (true, "Disques en bonne santé".into()) } else { (false, format!("⚠ Disque à surveiller : {}", mauvais.join(", "))) }
            }
            Err(e) => (false, format!("Santé des disques : {e}")),
        });
    }
    if r.optimiser_disques {
        etape("Optimisation des disques (TRIM / défragmentation)…");
        l.push(match cmd("defrag", &["/C", "/O", "/U"]) {
            Ok(_) => (true, "Disques optimisés".into()),
            Err(e) => (false, format!("Optimisation des disques : {e} (droits administrateur nécessaires)")),
        });
    }
    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    Bilan { quand: crate::minecraft::date_lisible(), lignes: l }
}

/// Tâche Windows qui sort le PC de veille à l'heure de la maintenance.
fn programmer_reveil(r: &Reglages) -> Result<(), String> {
    let nom = "Outils - reveil maintenance";
    if !r.actif || !r.reveiller {
        let _ = cmd("schtasks", &["/Delete", "/TN", nom, "/F"]);
        return Ok(());
    }
    let jours = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let script = format!(
        "$a = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument '/c exit'; $t = New-ScheduledTaskTrigger -Weekly -DaysOfWeek {} -At '{:02}:00'; $s = New-ScheduledTaskSettingsSet -WakeToRun -AllowStartIfOnBatteries; Register-ScheduledTask -TaskName '{nom}' -Action $a -Trigger $t -Settings $s -Force | Out-Null",
        jours[r.jour as usize % 7],
        r.heure
    );
    crate::optimiseur::ps(&script).map(drop)
}

pub struct Maintenance {
    r: Reglages,
    bilans: Arc<Mutex<Vec<Bilan>>>,
    etat: Arc<Mutex<String>>,
    en_cours: Arc<AtomicBool>,
    lance: bool,
    message: String,
}

impl Maintenance {
    pub fn new() -> Self {
        Maintenance { r: charger("maintenance"), bilans: Arc::new(Mutex::new(charger("maintenance_bilans"))), etat: Arc::default(), en_cours: Arc::default(), lance: false, message: String::new() }
    }

    fn lancer(&mut self) {
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        self.r.derniere = maintenant();
        enregistrer("maintenance", &self.r);
        let (r, b, e, c) = (self.r.clone(), self.bilans.clone(), self.etat.clone(), self.en_cours.clone());
        std::thread::spawn(move || {
            let bilan = executer(&r, &e);
            let ok = bilan.lignes.iter().filter(|l| l.0).count();
            crate::systeme::notifier("Maintenance du PC terminée", &format!("{ok}/{} étapes réussies. {}", bilan.lignes.len(), bilan.lignes.iter().find(|l| !l.0).map(|l| l.1.clone()).unwrap_or_default()));
            let mut v = b.lock().unwrap();
            v.insert(0, bilan);
            v.truncate(20);
            enregistrer("maintenance_bilans", &*v);
            *e.lock().unwrap() = String::new();
            c.store(false, Relaxed);
        });
    }
}

impl Outil for Maintenance {
    fn nom(&self) -> &'static str {
        "🧽 Maintenance automatique"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        // vérifié une fois par minute
        static DERNIER: Mutex<u64> = Mutex::new(0);
        let t = maintenant();
        {
            let mut d = DERNIER.lock().unwrap();
            if t.saturating_sub(*d) < 60 {
                return;
            }
            *d = t;
        }
        if !self.lance {
            self.lance = true;
            return; // pas dans la première minute après le démarrage
        }
        let (j, h) = jour_heure();
        let jeu = crate::outil::JEU_ACTIF.lock().map(|g| g.is_some()).unwrap_or(false);
        if a_lancer(&self.r, j, h, t, crate::quotidien::inactif_depuis(), jeu) {
            self.lancer();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Maintenance automatique");
        ui.weak("Chaque semaine, pendant que tu dors, Outils entretient le PC. S'il était éteint, il rattrape au prochain moment où tu ne t'en sers pas (jamais pendant un jeu).");
        let avant = self.r.clone();
        ui.checkbox(&mut self.r.actif, RichText::new("Activer la maintenance automatique").strong());
        ui.horizontal(|ui| {
            ui.label("Le");
            egui::ComboBox::from_id_salt("jour_maint").selected_text(JOURS[self.r.jour as usize % 7]).show_ui(ui, |ui| {
                for (k, j) in JOURS.iter().enumerate() {
                    ui.selectable_value(&mut self.r.jour, k as u8, *j);
                }
            });
            ui.label("à");
            ui.add(egui::DragValue::new(&mut self.r.heure).range(0..=23).suffix(" h"));
        });
        ui.checkbox(&mut self.r.restauration, "Créer un point de restauration avant (pour pouvoir revenir en arrière)");
        ui.checkbox(&mut self.r.nettoyage, "Nettoyer (fichiers temporaires, caches, corbeille…)");
        ui.checkbox(&mut self.r.mises_a_jour, "Mettre à jour les logiciels (winget) — peut fermer les programmes à mettre à jour");
        ui.checkbox(&mut self.r.disques, "Vérifier la santé des disques");
        ui.checkbox(&mut self.r.optimiser_disques, "Optimiser les disques (TRIM des SSD, défragmentation des disques durs)");
        ui.checkbox(&mut self.r.reveiller, "Sortir le PC de veille pour la maintenance (s'il est en veille, pas éteint)");
        ui.weak("Certaines étapes demandent de lancer Outils en administrateur (point de restauration, optimisation des disques).");
        if self.r != avant {
            enregistrer("maintenance", &self.r);
            if self.r.reveiller != avant.reveiller || self.r.jour != avant.jour || self.r.heure != avant.heure || self.r.actif != avant.actif {
                self.message = match programmer_reveil(&self.r) {
                    Ok(()) => String::new(),
                    Err(e) => format!("Réveil automatique : {e}"),
                };
            }
        }
        ui.horizontal(|ui| {
            if self.en_cours.load(Relaxed) {
                ui.spinner();
                ui.label(self.etat.lock().unwrap().clone());
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            } else if ui.button("▶ Lancer maintenant").clicked() {
                self.lancer();
            }
            ui.label(&self.message);
        });
        ui.separator();
        ui.strong("Derniers bilans");
        let b = self.bilans.lock().unwrap().clone();
        if b.is_empty() {
            ui.weak("Pas encore de maintenance faite.");
        }
        for x in b.iter().take(8) {
            egui::CollapsingHeader::new(format!("{} · {}/{} réussies", x.quand, x.lignes.iter().filter(|l| l.0).count(), x.lignes.len())).id_salt(&x.quand).show(ui, |ui| {
                for (ok, t) in &x.lignes {
                    ui.colored_label(if *ok { Color32::LIGHT_GREEN } else { Color32::from_rgb(255, 170, 80) }, format!("{} {t}", if *ok { "✔" } else { "✖" }));
                }
            });
        }
    }
    fn resume(&self) -> Option<String> {
        self.r.actif.then(|| format!("Chaque {} à {} h", JOURS[self.r.jour as usize % 7], self.r.heure))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quand_lancer() {
        let mut r = Reglages { actif: true, jour: 6, heure: 3, ..Default::default() };
        let t = 1_000_000_000;
        assert!(a_lancer(&r, 6, 3, t, 600, false));
        assert!(!a_lancer(&r, 6, 3, t, 600, true), "jamais pendant un jeu");
        assert!(!a_lancer(&r, 6, 3, t, 60, false), "pas si quelqu'un s'en sert");
        assert!(!a_lancer(&r, 5, 3, t, 600, false));
        r.derniere = t - 3600;
        assert!(!a_lancer(&r, 6, 3, t, 600, false), "déjà faite");
        r.derniere = t - 9 * 86400;
        assert!(a_lancer(&r, 2, 15, t, 1000, false), "rattrapage");
        r.actif = false;
        assert!(!a_lancer(&r, 6, 3, t, 600, false));
    }
}
