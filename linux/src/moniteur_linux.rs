//! Moniteur de jeu Linux : processeur, carte graphique, mémoire et températures en direct,
//! et MangoHud (FPS en jeu) réglé en un clic, avec le bilan des FPS de chaque partie.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn lire(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn existe(prog: &str) -> bool {
    std::env::var("PATH").unwrap_or_default().split(':').any(|d| Path::new(d).join(prog).is_file())
}

#[derive(Default, Clone, Copy)]
pub struct Mesure {
    pub cpu: f32,
    pub cpu_t: f32,
    pub gpu: f32,
    pub gpu_t: f32,
    pub ram: f32,
}

/// Température du processeur et de la carte graphique (capteurs hwmon).
fn temperatures() -> (f32, f32) {
    let (mut c, mut g) = (-1.0f32, -1.0f32);
    for e in std::fs::read_dir("/sys/class/hwmon").into_iter().flatten().flatten() {
        let n = lire(&e.path().join("name"));
        let t = lire(&e.path().join("temp1_input")).trim().parse::<f32>().unwrap_or(-1000.0) / 1000.0;
        match n.trim() {
            "k10temp" | "coretemp" | "zenpower" | "cpu_thermal" | "acpitz" if c < 0.0 => c = t,
            "amdgpu" | "nouveau" | "radeon" | "i915" | "xe" => g = g.max(t),
            _ => {}
        }
    }
    (c, g)
}

/// Charge de la carte graphique (AMD par sysfs, NVIDIA par nvidia-smi) et sa température.
fn gpu() -> (f32, f32) {
    for e in std::fs::read_dir("/sys/class/drm").into_iter().flatten().flatten() {
        let p = e.path().join("device/gpu_busy_percent");
        if let Ok(t) = std::fs::read_to_string(&p) {
            return (t.trim().parse().unwrap_or(-1.0), -1.0);
        }
    }
    if existe("nvidia-smi") {
        let o = std::process::Command::new("nvidia-smi").args(["--query-gpu=utilization.gpu,temperature.gpu", "--format=csv,noheader,nounits"]).output();
        if let Ok(o) = o {
            let t = String::from_utf8_lossy(&o.stdout);
            let v: Vec<f32> = t.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() == 2 {
                return (v[0], v[1]);
            }
        }
    }
    (-1.0, -1.0)
}

/// Bilan d'un journal MangoHud (CSV) : (FPS moyen, 1 % bas, nombre de mesures).
pub fn bilan_mangohud(t: &str) -> Option<(f32, f32, usize)> {
    let mut lignes = t.lines();
    let entete = lignes.by_ref().find(|l| l.starts_with("fps,"))?;
    let k = entete.split(',').position(|c| c == "fps")?;
    let mut v: Vec<f32> = lignes.filter_map(|l| l.split(',').nth(k)?.parse().ok()).filter(|x: &f32| *x > 0.0).collect();
    if v.is_empty() {
        return None;
    }
    let moy = v.iter().sum::<f32>() / v.len() as f32;
    v.sort_by(|a, b| a.total_cmp(b));
    let n = (v.len() / 100).max(1);
    let bas = v[..n].iter().sum::<f32>() / n as f32;
    Some((moy, bas, v.len()))
}

fn dossier_journaux() -> PathBuf {
    crate::outil::dossier().join("mangohud")
}

fn config_mangohud() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/MangoHud/MangoHud.conf")
}

pub struct Moniteur {
    m: Mesure,
    hist: Vec<Mesure>,
    avant: (u64, u64),
    vu: Instant,
    message: String,
}

impl Moniteur {
    pub fn new() -> Self {
        Moniteur { m: Mesure::default(), hist: vec![], avant: (0, 0), vu: Instant::now() - Duration::from_secs(5), message: String::new() }
    }

    fn mesurer(&mut self) {
        let (a, t) = crate::systeme_linux::temps_cpu(&lire(Path::new("/proc/stat")));
        let cpu = if self.avant.1 > 0 && t > self.avant.1 { (a - self.avant.0) as f32 / (t - self.avant.1) as f32 * 100.0 } else { 0.0 };
        self.avant = (a, t);
        let (tot, dispo) = crate::systeme_linux::memoire(&lire(Path::new("/proc/meminfo")));
        let (ct, gt) = temperatures();
        let (g, gt2) = gpu();
        self.m = Mesure { cpu, cpu_t: ct, gpu: g, gpu_t: if gt2 >= 0.0 { gt2 } else { gt }, ram: if tot > 0 { (tot - dispo) as f32 * 100.0 / tot as f32 } else { 0.0 } };
        self.hist.push(self.m);
        if self.hist.len() > 120 {
            self.hist.remove(0);
        }
    }
}

fn courbe(ui: &mut egui::Ui, v: &[f32], max: f32, c: Color32) {
    let (r, p) = ui.allocate_painter(egui::vec2(ui.available_width().min(460.0), 50.0), egui::Sense::hover());
    let r = r.rect;
    p.rect_filled(r, 4.0, ui.visuals().extreme_bg_color);
    if v.len() >= 2 {
        let pts: Vec<egui::Pos2> = v.iter().enumerate().map(|(i, x)| egui::pos2(r.left() + r.width() * i as f32 / 119.0, r.bottom() - r.height() * (x / max).clamp(0.0, 1.0))).collect();
        p.add(egui::Shape::line(pts, egui::Stroke::new(1.5_f32, c)));
    }
}

impl Outil for Moniteur {
    fn nom(&self) -> &'static str {
        "📊 Moniteur de jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.vu.elapsed() > Duration::from_secs(1) {
            self.vu = Instant::now();
            self.mesurer();
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        ui.heading("Moniteur de jeu");
        let m = self.m;
        let val = |x: f32, u: &str| if x >= 0.0 { format!("{x:.0} {u}") } else { "—".into() };
        egui::Grid::new("moni").num_columns(2).spacing([20.0, 4.0]).show(ui, |ui| {
            ui.strong("Processeur");
            ui.label(format!("{} · {}", val(m.cpu, "%"), val(m.cpu_t, "°C")));
            ui.end_row();
            ui.strong("Carte graphique");
            ui.label(format!("{} · {}", val(m.gpu, "%"), val(m.gpu_t, "°C")));
            ui.end_row();
            ui.strong("Mémoire");
            ui.label(val(m.ram, "%"));
            ui.end_row();
        });
        courbe(ui, &self.hist.iter().map(|x| x.cpu).collect::<Vec<_>>(), 100.0, Color32::from_rgb(120, 180, 255));
        if m.gpu >= 0.0 {
            courbe(ui, &self.hist.iter().map(|x| x.gpu).collect::<Vec<_>>(), 100.0, Color32::from_rgb(120, 220, 140));
        }
        if m.cpu_t > 90.0 || m.gpu_t > 87.0 {
            ui.colored_label(Color32::LIGHT_RED, "🌡 Ça chauffe beaucoup : le PC va ralentir pour se protéger. Dépoussière les ventilateurs.");
        }
        ui.separator();
        ui.strong("🎮 FPS en jeu avec MangoHud");
        let installe = existe("mangohud");
        if !installe {
            ui.label("MangoHud affiche les FPS, la charge et les températures par-dessus les jeux. Installe-le avec ton gestionnaire de paquets (paquet « mangohud ») ou depuis la Logithèque / Flathub.");
        } else {
            ui.label("Pour l'activer dans un jeu Steam : clic droit sur le jeu › Propriétés › Options de lancement : mangohud %command%");
            if ui.button("⚙ Régler MangoHud (FPS, températures, et bilan de chaque partie)").clicked() {
                let _ = std::fs::create_dir_all(config_mangohud().parent().unwrap());
                let _ = std::fs::create_dir_all(dossier_journaux());
                let conf = format!("# réglé par Outils\nfps\nframetime\ncpu_stats\ncpu_temp\ngpu_stats\ngpu_temp\nram\nposition=top-left\nfont_size=20\noutput_folder={}\nautostart_log=5\nlog_duration=0\n", dossier_journaux().display());
                self.message = match std::fs::write(config_mangohud(), conf) {
                    Ok(()) => "✔ MangoHud réglé : les FPS de chaque partie seront notés ici.".into(),
                    Err(e) => format!("✖ {e}"),
                };
            }
            ui.label(&self.message);
        }
        let mut journaux: Vec<PathBuf> = std::fs::read_dir(dossier_journaux()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "csv")).collect();
        journaux.sort();
        journaux.reverse();
        if !journaux.is_empty() {
            ui.strong("Dernières parties");
            for j in journaux.iter().take(12) {
                if let Some((moy, bas, n)) = bilan_mangohud(&lire(j)) {
                    let nom = j.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    ui.label(RichText::new(format!("{nom} · {moy:.0} FPS en moyenne · 1 % bas : {bas:.0} FPS · {n} mesures")).monospace());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangohud() {
        let t = "os,cpu,gpu\nLinux,Ryzen,RX\nfps,frametime,cpu_load\n60,16.6,30\n120,8.3,40\n90,11,35\n";
        let (m, b, n) = bilan_mangohud(t).unwrap();
        assert_eq!(n, 3);
        assert!((m - 90.0).abs() < 0.01);
        assert_eq!(b, 60.0);
        assert!(bilan_mangohud("rien").is_none());
    }
}
