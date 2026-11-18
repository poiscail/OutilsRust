//! Démarrage du PC : durée de chaque démarrage et ce qui l'a ralenti (applis, pilotes, services),
//! d'après le journal Windows « Diagnostics-Performance » (lisible en administrateur).
use eframe::egui::{self, Color32, Pos2, Sense, Stroke};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Evenement {
    pub id: u32,
    pub t: String,
    pub boot: Option<String>,
    pub main: Option<String>,
    pub post: Option<String>,
    pub nom: Option<String>,
    pub exe: Option<String>,
    pub degr: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Demarrage {
    pub date: String,
    pub total_ms: u64,
    pub principal_ms: u64, // jusqu'au bureau
}

#[derive(Clone, Debug, PartialEq)]
pub struct Coupable {
    pub nom: String,
    pub genre: &'static str,
    pub fois: u32,
    pub moyenne_ms: u64,
    pub derniere: String,
}

const SCRIPT: &str = r#"
$r = @()
Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-Diagnostics-Performance/Operational'; Id=100,101,102,103,106,109} -MaxEvents 400 | ForEach-Object {
  $x = [xml]$_.ToXml(); $d = @{}
  foreach ($n in $x.Event.EventData.Data) { $d[$n.Name] = $n.'#text' }
  $r += [pscustomobject]@{ id=$_.Id; t=$_.TimeCreated.ToString('dd/MM/yyyy HH:mm'); boot=$d['BootTime']; main=$d['MainPathBootTime']; post=$d['BootPostBootTime']; nom=$d['FriendlyName']; exe=$d['Name']; degr=$d['DegradationTime'] }
}
ConvertTo-Json -InputObject @($r) -Compress
"#;

fn nombre(s: &Option<String>) -> u64 {
    s.as_deref().and_then(|x| x.trim().parse().ok()).unwrap_or(0)
}

/// Sépare les démarrages (événement 100, du plus récent au plus ancien) et les coupables (101 à 109).
pub fn analyser(ev: &[Evenement]) -> (Vec<Demarrage>, Vec<Coupable>) {
    let demarrages = ev.iter().filter(|e| e.id == 100).map(|e| Demarrage { date: e.t.clone(), total_ms: nombre(&e.boot), principal_ms: nombre(&e.main) }).collect();
    let mut par: BTreeMap<(String, &'static str), (u32, u64, String)> = BTreeMap::new();
    for e in ev.iter().filter(|e| e.id != 100) {
        let genre = match e.id {
            101 => "appli",
            102 => "pilote",
            103 => "service",
            106 => "optimisation",
            109 => "périphérique",
            _ => "autre",
        };
        let nom = e.nom.clone().filter(|n| !n.trim().is_empty()).or_else(|| e.exe.clone()).unwrap_or_else(|| "?".into());
        let x = par.entry((nom, genre)).or_insert((0, 0, e.t.clone()));
        x.0 += 1;
        x.1 += nombre(&e.degr);
    }
    let mut c: Vec<Coupable> = par.into_iter().map(|((nom, genre), (fois, somme, derniere))| Coupable { nom, genre, fois, moyenne_ms: somme / fois.max(1) as u64, derniere }).collect();
    c.sort_by(|a, b| (b.moyenne_ms * b.fois as u64).cmp(&(a.moyenne_ms * a.fois as u64)));
    (demarrages, c)
}

fn duree_allumee() -> String {
    let ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let m = ms / 60_000;
    if m >= 1440 { format!("{} j {} h", m / 1440, m / 60 % 24) } else { format!("{} h {:02}", m / 60, m % 60) }
}

#[derive(Default)]
struct Etat {
    charge: bool,
    en_cours: bool,
    demarrages: Vec<Demarrage>,
    coupables: Vec<Coupable>,
    erreur: Option<String>,
}

pub struct DemarragePc {
    e: Arc<Mutex<Etat>>,
}

impl DemarragePc {
    pub fn new() -> Self {
        DemarragePc { e: Arc::default() }
    }

    fn charger(&self) {
        let e = self.e.clone();
        {
            let mut x = e.lock().unwrap();
            if x.en_cours {
                return;
            }
            x.en_cours = true;
        }
        std::thread::spawn(move || {
            let r = crate::optimiseur::ps(SCRIPT).and_then(|s| serde_json::from_str::<Vec<Evenement>>(s.trim()).map_err(|x| x.to_string()));
            let mut x = e.lock().unwrap();
            x.en_cours = false;
            x.charge = true;
            match r {
                Ok(ev) => {
                    let (d, c) = analyser(&ev);
                    x.demarrages = d;
                    x.coupables = c;
                    x.erreur = None;
                }
                Err(m) => x.erreur = Some(m),
            }
        });
    }

    /// Renvoie true pour aller à l'Optimiseur (gestion des programmes au démarrage).
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        ui.heading("Démarrage du PC");
        if !self.e.lock().unwrap().charge {
            self.charger();
        }
        let mut aller = false;
        let admin = crate::optimiseur::est_admin();
        ui.horizontal(|ui| {
            ui.label(format!("Allumé depuis {}", duree_allumee()));
            if ui.button("⟳ Actualiser").clicked() {
                self.charger();
            }
            if ui.button("🚀 Gérer les programmes au démarrage").on_hover_text("Onglet Démarrage de l'Optimiseur").clicked() {
                aller = true;
            }
        });
        let e = self.e.lock().unwrap();
        if e.en_cours {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Lecture du journal de Windows…");
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        if let Some(m) = &e.erreur {
            ui.colored_label(Color32::LIGHT_RED, format!("Journal illisible : {m}"));
            if !admin {
                ui.horizontal(|ui| {
                    ui.label("Windows réserve ce journal aux administrateurs.");
                    if ui.button("Relancer Outils en administrateur").clicked() && crate::optimiseur::relancer_admin() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            }
            return aller;
        }
        if e.charge && e.demarrages.is_empty() {
            ui.weak("Aucun démarrage enregistré (le suivi des performances de Windows est peut-être désactivé).");
            return aller;
        }
        if let Some(d) = e.demarrages.first() {
            let moy = e.demarrages.iter().map(|d| d.total_ms).sum::<u64>() / e.demarrages.len() as u64;
            ui.horizontal(|ui| {
                ui.label("Dernier démarrage :");
                ui.strong(format!("{:.1} s", d.total_ms as f64 / 1000.0));
                ui.weak(format!("(bureau en {:.1} s, le {})", d.principal_ms as f64 / 1000.0, d.date));
                ui.separator();
                ui.label(format!("moyenne {:.1} s sur {} démarrages", moy as f64 / 1000.0, e.demarrages.len()));
            });
            // histogramme, le plus ancien à gauche
            let v: Vec<&Demarrage> = e.demarrages.iter().take(30).rev().collect();
            let (zone, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 140.0), Sense::hover());
            let p = ui.painter_at(zone);
            p.rect_filled(zone, 4.0, Color32::from_black_alpha(60));
            let max = v.iter().map(|d| d.total_ms).max().unwrap_or(1).max(1) as f32 * 1.1;
            let l = zone.width() / v.len().max(1) as f32;
            for (i, d) in v.iter().enumerate() {
                let x = zone.left() + i as f32 * l;
                let h = |ms: u64| zone.height() * ms as f32 / max;
                let r = egui::Rect::from_min_max(Pos2::new(x + 2.0, zone.bottom() - h(d.total_ms)), Pos2::new(x + l - 2.0, zone.bottom()));
                p.rect_filled(r, 2.0, Color32::from_rgb(255, 140, 190).gamma_multiply(0.45));
                let r2 = egui::Rect::from_min_max(Pos2::new(x + 2.0, zone.bottom() - h(d.principal_ms)), Pos2::new(x + l - 2.0, zone.bottom()));
                p.rect_filled(r2, 2.0, Color32::from_rgb(255, 95, 162));
                if ui.rect_contains_pointer(r) {
                    p.rect_stroke(r, 2.0, Stroke::new(1.0f32, Color32::WHITE), egui::StrokeKind::Inside);
                    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("boot_tip"), |ui| {
                        ui.label(format!("{} : {:.1} s (bureau {:.1} s)", d.date, d.total_ms as f64 / 1000.0, d.principal_ms as f64 / 1000.0));
                    });
                }
            }
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(255, 95, 162), "■ jusqu'au bureau");
                ui.colored_label(Color32::from_rgb(255, 140, 190).gamma_multiply(0.6), "■ jusqu'à ce que tout soit chargé");
            });
        }
        ui.separator();
        ui.strong("Ce qui a ralenti le démarrage");
        if e.coupables.is_empty() {
            ui.weak("Rien de signalé par Windows.");
        }
        egui::ScrollArea::vertical().id_salt("coupables").show(ui, |ui| {
            egui::Grid::new("grille_coupables").num_columns(4).striped(true).spacing([16.0, 6.0]).show(ui, |ui| {
                ui.strong("Nom");
                ui.strong("Type");
                ui.strong("Retard moyen");
                ui.strong("Fois · dernière");
                ui.end_row();
                for c in e.coupables.iter().take(40) {
                    ui.label(&c.nom);
                    ui.label(c.genre);
                    ui.label(format!("+{:.1} s", c.moyenne_ms as f64 / 1000.0));
                    ui.weak(format!("{} · {}", c.fois, c.derniere));
                    ui.end_row();
                }
            });
        });
        aller
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture() {
        let json = r#"[{"id":100,"t":"18/11/2026 08:01","boot":"42000","main":"18000","post":"24000","nom":null,"exe":null,"degr":null},
            {"id":101,"t":"18/11/2026 08:01","boot":null,"main":null,"post":null,"nom":"Steam","exe":"steam.exe","degr":"3000"},
            {"id":101,"t":"17/11/2026 08:00","boot":null,"main":null,"post":null,"nom":"Steam","exe":"steam.exe","degr":"5000"},
            {"id":103,"t":"17/11/2026 08:00","boot":null,"main":null,"post":null,"nom":"","exe":"Spooler","degr":"1000"}]"#;
        let ev: Vec<Evenement> = serde_json::from_str(json).unwrap();
        let (d, c) = analyser(&ev);
        assert_eq!(d, vec![Demarrage { date: "18/11/2026 08:01".into(), total_ms: 42000, principal_ms: 18000 }]);
        assert_eq!(c[0].nom, "Steam");
        assert_eq!((c[0].fois, c[0].moyenne_ms, c[0].genre), (2, 4000, "appli"));
        assert_eq!(c[1].nom, "Spooler");
        assert_eq!(c[1].genre, "service");
    }
}
