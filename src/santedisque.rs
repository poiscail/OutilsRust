//! Santé des disques : état donné par Windows, température, usure (SSD), heures d'utilisation, erreurs
//! (compteurs SMART, lisibles en administrateur). Vérifie au lancement puis toutes les 12 h, alerte si ça se gâte.
use eframe::egui::{self, Color32};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Disque {
    pub nom: String,
    #[serde(rename = "type")]
    pub genre: String,
    pub bus: String,
    pub taille: u64,
    pub sante: String,
    pub etat: String,
    pub temp: Option<i64>,
    pub tempmax: Option<i64>,
    pub usure: Option<i64>,
    pub heures: Option<i64>,
    pub err_lec: Option<i64>,
    pub err_ecr: Option<i64>,
}

const SCRIPT: &str = r#"
$r = @()
foreach ($d in Get-PhysicalDisk) {
  $c = $null
  try { $c = $d | Get-StorageReliabilityCounter -ErrorAction Stop } catch {}
  $r += [pscustomobject]@{ nom=$d.FriendlyName; type=[string]$d.MediaType; bus=[string]$d.BusType; taille=[uint64]$d.Size; sante=[string]$d.HealthStatus;
    etat=($d.OperationalStatus -join ', '); temp=$c.Temperature; tempmax=$c.TemperatureMax; usure=$c.Wear; heures=$c.PowerOnHours;
    err_lec=$c.ReadErrorsTotal; err_ecr=$c.WriteErrorsTotal }
}
ConvertTo-Json -InputObject @($r) -Compress
"#;

/// Problèmes à signaler pour un disque.
pub fn alertes(d: &Disque) -> Vec<String> {
    let mut v = vec![];
    if !d.sante.is_empty() && d.sante != "Healthy" {
        v.push(format!("état « {} » selon Windows", traduire_sante(&d.sante)));
    }
    if let Some(u) = d.usure.filter(|u| *u >= 80) {
        v.push(format!("usure {u} %"));
    }
    if let Some(t) = d.temp.filter(|t| *t >= 60) {
        v.push(format!("température {t} °C"));
    }
    let erreurs = d.err_lec.unwrap_or(0) + d.err_ecr.unwrap_or(0);
    if erreurs > 0 {
        v.push(format!("{erreurs} erreur(s) de lecture/écriture"));
    }
    v
}

pub fn traduire_sante(s: &str) -> &str {
    match s {
        "Healthy" => "Bon",
        "Warning" => "Attention",
        "Unhealthy" => "Mauvais",
        "" => "?",
        x => x,
    }
}

fn lire() -> Result<Vec<Disque>, String> {
    let s = crate::optimiseur::ps(SCRIPT)?;
    serde_json::from_str(s.trim()).map_err(|e| e.to_string())
}

#[derive(Default)]
struct Etat {
    disques: Option<Vec<Disque>>,
    erreur: Option<String>,
    occupe: bool,
}

pub struct SanteDisque {
    e: Arc<Mutex<Etat>>,
}

fn charger(e: &Arc<Mutex<Etat>>, prevenir: bool) {
    {
        let mut x = e.lock().unwrap();
        if x.occupe {
            return;
        }
        x.occupe = true;
    }
    let r = lire();
    let mut x = e.lock().unwrap();
    x.occupe = false;
    match r {
        Ok(v) => {
            if prevenir {
                let pb: Vec<String> = v.iter().filter_map(|d| {
                    let a = alertes(d);
                    (!a.is_empty()).then(|| format!("{} : {}", d.nom, a.join(", ")))
                }).collect();
                if !pb.is_empty() {
                    crate::systeme::notifier_toujours("Santé des disques", &pb.join(" · "));
                }
            }
            x.disques = Some(v);
            x.erreur = None;
        }
        Err(m) => x.erreur = Some(m),
    }
}

impl SanteDisque {
    pub fn new() -> Self {
        let e: Arc<Mutex<Etat>> = Arc::default();
        let e2 = e.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(90)); // laisse le PC finir de démarrer
            loop {
                charger(&e2, true);
                std::thread::sleep(Duration::from_secs(12 * 3600));
            }
        });
        SanteDisque { e }
    }

    pub fn resume(&self) -> String {
        match &self.e.lock().unwrap().disques {
            None => "Pas encore vérifié".into(),
            Some(v) => {
                let n = v.iter().filter(|d| !alertes(d).is_empty()).count();
                if n == 0 { format!("{} disque(s), tout va bien", v.len()) } else { format!("⚠ {n} disque(s) à surveiller") }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Santé des disques");
        let (disques, erreur, occupe) = {
            let x = self.e.lock().unwrap();
            (x.disques.clone(), x.erreur.clone(), x.occupe)
        };
        if disques.is_none() && !occupe && erreur.is_none() {
            let e = self.e.clone();
            std::thread::spawn(move || charger(&e, false));
        }
        ui.horizontal(|ui| {
            if occupe {
                ui.spinner();
                ui.label("Lecture…");
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            } else if ui.button("⟳ Actualiser").clicked() {
                let e = self.e.clone();
                std::thread::spawn(move || charger(&e, false));
            }
            if !crate::optimiseur::est_admin() {
                ui.weak("Sans droits admin, Windows ne donne que l'état général (pas la température ni l'usure).");
            }
        });
        if let Some(m) = erreur {
            ui.colored_label(Color32::LIGHT_RED, m);
        }
        ui.separator();
        for d in disques.iter().flatten() {
            let a = alertes(d);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (txt, c) = if a.is_empty() { ("✔", Color32::LIGHT_GREEN) } else { ("⚠", Color32::from_rgb(255, 190, 80)) };
                    ui.colored_label(c, egui::RichText::new(txt).size(18.0));
                    ui.strong(&d.nom);
                    ui.weak(format!("{} · {} · {}", d.genre, d.bus, crate::disque::taille_lisible(d.taille)));
                });
                egui::Grid::new(("disque", &d.nom)).num_columns(2).spacing([20.0, 4.0]).show(ui, |ui| {
                    ui.label("État");
                    ui.label(format!("{} ({})", traduire_sante(&d.sante), d.etat));
                    ui.end_row();
                    let opt = |v: Option<i64>, u: &str| v.map(|x| format!("{x}{u}")).unwrap_or_else(|| "—".into());
                    ui.label("Température");
                    ui.label(format!("{} (max {})", opt(d.temp, " °C"), opt(d.tempmax, " °C")));
                    ui.end_row();
                    ui.label("Usure");
                    match d.usure {
                        Some(u) => {
                            ui.add(egui::ProgressBar::new(u as f32 / 100.0).desired_width(200.0).text(format!("{u} %")));
                        }
                        None => {
                            ui.label("—");
                        }
                    }
                    ui.end_row();
                    ui.label("Heures d'utilisation");
                    ui.label(d.heures.map(|h| format!("{h} h ({:.1} ans)", h as f64 / 8766.0)).unwrap_or_else(|| "—".into()));
                    ui.end_row();
                    ui.label("Erreurs lecture / écriture");
                    ui.label(format!("{} / {}", opt(d.err_lec, ""), opt(d.err_ecr, "")));
                    ui.end_row();
                });
                for x in &a {
                    ui.colored_label(Color32::from_rgb(255, 190, 80), format!("⚠ {x}"));
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_et_alertes() {
        let json = r#"[{"nom":"Samsung SSD 970","type":"SSD","bus":"NVMe","taille":1000204886016,"sante":"Healthy","etat":"OK","temp":41,"tempmax":70,"usure":3,"heures":5120,"err_lec":0,"err_ecr":0},
                       {"nom":"WD Blue","type":"HDD","bus":"SATA","taille":2000398934016,"sante":"Warning","etat":"Predictive Failure","temp":null,"tempmax":null,"usure":null,"heures":null,"err_lec":null,"err_ecr":null}]"#;
        let v: Vec<Disque> = serde_json::from_str(json).unwrap();
        assert!(alertes(&v[0]).is_empty());
        assert_eq!(alertes(&v[1]), vec!["état « Attention » selon Windows".to_string()]);
        let chaud = Disque { temp: Some(65), usure: Some(85), err_lec: Some(2), ..Default::default() };
        assert_eq!(alertes(&chaud).len(), 3);
    }
}
