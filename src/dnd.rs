//! Ne pas déranger : pendant un jeu, sur une plage horaire ou à la main, active le mode « Ne pas déranger »
//! de Windows, coupe les notifications d'Outils et baisse le son des applis choisies ; tout revient à la fin.
//! Réglages dans %APPDATA%\Outils\ne_pas_deranger.json.
use crate::mixeur::{Mixeur, Ordre};
use crate::planif::lire_heure;
use crate::pressepapiers::{decalage, maintenant_utc};
use crate::systeme;
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub manuel: bool,
    pub pendant_jeux: bool,
    pub plage: bool,
    pub debut: String,
    pub fin: String,
    pub windows: bool,
    pub couper_notifs: bool,
    pub baisser: Vec<String>, // exe dont on baisse le son
    pub niveau: f32,          // 0..1
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            manuel: false, pendant_jeux: true, plage: false, debut: "23:00".into(), fin: "07:00".into(), windows: true, couper_notifs: true,
            baisser: vec![], niveau: 0.2,
        }
    }
}

fn fichier() -> PathBuf {
    crate::outil::base().join("ne_pas_deranger.json")
}

/// Vrai si `minute` est dans la plage [debut, fin[ (qui peut passer minuit).
pub fn dans_plage(debut: i64, fin: i64, minute: i64) -> bool {
    if debut <= fin { minute >= debut && minute < fin } else { minute >= debut || minute < fin }
}

/// Mode « Ne pas déranger » de Windows (Assistant de concentration) : 0 = désactivé, 2 = alarmes seulement.
/// Passe par l'état WNF que Windows utilise lui-même (non documenté).
pub fn windows_ne_pas_deranger(actif: bool) -> bool {
    type Maj = unsafe extern "system" fn(*const u64, *const core::ffi::c_void, u32, *const core::ffi::c_void, *const core::ffi::c_void, u32, u32) -> i32;
    const ETAT: u64 = 0x0D83_063E_A3BF_1C75; // WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED
    unsafe {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast());
        let Some(f) = GetProcAddress(ntdll, c"NtUpdateWnfStateData".as_ptr().cast()) else { return false };
        let f: Maj = std::mem::transmute(f);
        let v: u32 = if actif { 2 } else { 0 };
        f(&ETAT, (&v as *const u32).cast(), 4, std::ptr::null(), std::ptr::null(), 0, 0) == 0
    }
}

pub struct Dnd {
    r: Reglages,
    actif: bool,
    raison: String,
    notifs_avant: bool,
    volumes_avant: BTreeMap<String, f32>,
}

impl Dnd {
    pub fn new() -> Self {
        let r = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Dnd { r, actif: false, raison: String::new(), notifs_avant: false, volumes_avant: BTreeMap::new() }
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&self.r).unwrap_or_default());
    }

    fn voulu(&self, jeu: Option<&str>) -> Option<String> {
        if self.r.manuel {
            return Some("activé à la main".into());
        }
        if self.r.pendant_jeux {
            if let Some(j) = jeu {
                return Some(format!("pendant {j}"));
            }
        }
        if self.r.plage {
            if let (Some(d), Some(f)) = (lire_heure(&self.r.debut), lire_heure(&self.r.fin)) {
                let m = (maintenant_utc() + decalage()).rem_euclid(86400) / 60;
                if dans_plage(d, f, m) {
                    return Some(format!("de {} à {}", self.r.debut, self.r.fin));
                }
            }
        }
        None
    }

    fn activer(&mut self, mixeur: &Mixeur, raison: String) {
        if self.r.windows {
            windows_ne_pas_deranger(true);
        }
        if self.r.couper_notifs {
            self.notifs_avant = systeme::NOTIFS.swap(false, Relaxed);
        }
        self.volumes_avant.clear();
        for s in mixeur.applis() {
            if self.r.baisser.iter().any(|b| b.eq_ignore_ascii_case(&s.nom)) && s.volume > self.r.niveau {
                self.volumes_avant.insert(s.nom.clone(), s.volume);
                mixeur.envoyer(Ordre::VolumeApp(s.nom, self.r.niveau));
            }
        }
        self.actif = true;
        self.raison = raison;
    }

    fn desactiver(&mut self, mixeur: &Mixeur) {
        if self.r.windows {
            windows_ne_pas_deranger(false);
        }
        if self.r.couper_notifs && self.notifs_avant {
            systeme::NOTIFS.store(true, Relaxed);
        }
        for (exe, v) in std::mem::take(&mut self.volumes_avant) {
            mixeur.envoyer(Ordre::VolumeApp(exe, v));
        }
        self.actif = false;
        self.raison.clear();
    }

    /// À chaque image.
    pub fn tic(&mut self, jeu: Option<&str>, mixeur: &Mixeur) {
        match (self.voulu(jeu), self.actif) {
            (Some(r), false) => self.activer(mixeur, r),
            (None, true) => self.desactiver(mixeur),
            (Some(r), true) => self.raison = r,
            _ => {}
        }
    }

    pub fn resume(&self) -> String {
        if self.actif { format!("Actif ({})", self.raison) } else { "Inactif".into() }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, mixeur: &Mixeur) {
        ui.heading("Ne pas déranger");
        let avant = self.r.clone();
        if self.actif {
            ui.colored_label(Color32::from_rgb(255, 120, 170), format!("🌙 Actif : {}", self.raison));
        } else {
            ui.weak("Inactif");
        }
        ui.horizontal(|ui| {
            let t = if self.r.manuel { "🌙 Désactiver" } else { "🌙 Activer maintenant" };
            if ui.button(t).clicked() {
                self.r.manuel = !self.r.manuel;
            }
        });
        ui.separator();
        ui.strong("Quand");
        ui.checkbox(&mut self.r.pendant_jeux, "Pendant les jeux (détectés via le Lanceur)");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.r.plage, "Tous les jours de");
            ui.add(egui::TextEdit::singleline(&mut self.r.debut).desired_width(50.0));
            ui.label("à");
            ui.add(egui::TextEdit::singleline(&mut self.r.fin).desired_width(50.0));
        });
        ui.separator();
        ui.strong("Ce qui change");
        ui.checkbox(&mut self.r.windows, "Mode « Ne pas déranger » de Windows (alarmes seulement)");
        ui.checkbox(&mut self.r.couper_notifs, "Couper les notifications d'Outils");
        ui.horizontal(|ui| {
            ui.label("Baisser à");
            let mut p = self.r.niveau * 100.0;
            if ui.add(egui::Slider::new(&mut p, 0.0..=100.0).suffix(" %").fixed_decimals(0)).changed() {
                self.r.niveau = p / 100.0;
            }
            ui.label("le son de :");
        });
        let mut connues: Vec<String> = mixeur.applis().into_iter().map(|s| s.nom).collect();
        for b in &self.r.baisser {
            if !connues.iter().any(|c| c.eq_ignore_ascii_case(b)) {
                connues.push(b.clone());
            }
        }
        ui.horizontal_wrapped(|ui| {
            for n in connues {
                let mut coche = self.r.baisser.iter().any(|b| b.eq_ignore_ascii_case(&n));
                if ui.checkbox(&mut coche, n.trim_end_matches(".exe")).changed() {
                    if coche {
                        self.r.baisser.push(n.clone());
                    } else {
                        self.r.baisser.retain(|b| !b.eq_ignore_ascii_case(&n));
                    }
                }
            }
        });
        ui.weak("Les applis listées sont celles qui jouent du son en ce moment (lance-les pour les voir).");
        ui.weak("Le Moniteur Jeu s'occupe déjà de la priorité, du plan d'alimentation et de la mise en pause d'applis pendant les jeux.");
        if self.r != avant {
            self.enregistrer();
        }
    }
}

impl Drop for Dnd {
    /// En quittant Outils, on ne laisse pas Windows en « Ne pas déranger ».
    fn drop(&mut self) {
        if self.actif && self.r.windows {
            windows_ne_pas_deranger(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plages() {
        assert!(dans_plage(23 * 60, 7 * 60, 23 * 60 + 30));
        assert!(dans_plage(23 * 60, 7 * 60, 6 * 60));
        assert!(!dans_plage(23 * 60, 7 * 60, 12 * 60));
        assert!(dans_plage(9 * 60, 17 * 60, 10 * 60));
        assert!(!dans_plage(9 * 60, 17 * 60, 17 * 60));
    }
}
