//! Jeux, suite : profils de performance par jeu (29) et comparateur de FPS avant / après une mise à jour (32).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::os::windows::process::CommandExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SANS_FENETRE: u32 = 0x0800_0000;

// ===================================================================================================
// 29 · Profils de performance par jeu
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct ProfilJeu {
    pub priorite: u8, // 0 normale, 1 au-dessus de la normale, 2 haute
    pub plan: u8,     // 0 ne pas toucher, 1 performances maximales, 2 économie
    pub volume: Option<f32>,
    pub fermer: Vec<String>,
}

impl Default for ProfilJeu {
    fn default() -> Self {
        ProfilJeu { priorite: 1, plan: 1, volume: None, fermer: vec![] }
    }
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
struct Profils {
    actif: bool,
    jeux: BTreeMap<String, ProfilJeu>,
    dernier: String,
}

/// Classe de priorité Windows d'un niveau 0, 1, 2.
pub fn classe(p: u8) -> u32 {
    match p {
        2 => 0x0000_0080,  // HIGH_PRIORITY_CLASS
        1 => 0x0000_8000,  // ABOVE_NORMAL_PRIORITY_CLASS
        _ => 0x0000_0020,  // NORMAL_PRIORITY_CLASS
    }
}

/// Met la priorité de tous les programmes d'un dossier de jeu. Renvoie leur nombre.
fn prioriser(dossier: &str, niveau: u8) -> usize {
    use windows_sys::Win32::System::Threading::{OpenProcess, SetPriorityClass, PROCESS_SET_INFORMATION};
    let d = dossier.to_lowercase().replace('/', "\\");
    if d.len() < 4 {
        return 0;
    }
    let mut pids = vec![0u32; 4096];
    let mut n = 0u32;
    unsafe { windows_sys::Win32::System::ProcessStatus::K32EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut n) };
    let mut k = 0;
    for &p in &pids[..n as usize / 4] {
        let Some(c) = crate::lanceur::chemin_processus(p) else { continue };
        if c.to_lowercase().starts_with(&format!("{}\\", d.trim_end_matches('\\'))) {
            unsafe {
                let h = OpenProcess(PROCESS_SET_INFORMATION, 0, p);
                if !h.is_null() {
                    SetPriorityClass(h, classe(niveau));
                    windows_sys::Win32::Foundation::CloseHandle(h);
                    k += 1;
                }
            }
        }
    }
    k
}

fn plan_actuel() -> Option<String> {
    let o = std::process::Command::new("powercfg").arg("/getactivescheme").creation_flags(SANS_FENETRE).output().ok()?;
    let t = String::from_utf8_lossy(&o.stdout).to_string();
    regex::Regex::new(r"([0-9a-fA-F]{8}-[0-9a-fA-F-]{27})").ok()?.captures(&t).map(|c| c[1].to_string())
}

fn volume_actuel() -> Option<f32> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let d = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let v: IAudioEndpointVolume = d.Activate(CLSCTX_ALL, None).ok()?;
        v.GetMasterVolumeLevelScalar().ok()
    }
}

pub struct ProfilsJeux {
    p: Arc<Mutex<Profils>>,
    choisi: String,
    ajout: String,
}

impl ProfilsJeux {
    pub fn new() -> Self {
        let p: Arc<Mutex<Profils>> = Arc::new(Mutex::new(charger("profils_jeux")));
        let p2 = p.clone();
        std::thread::spawn(move || {
            let mut en_cours: Option<String> = None;
            let mut a_restaurer: (Option<String>, Option<f32>) = (None, None);
            let mut dossier = String::new();
            let mut tour = 0u32;
            loop {
                std::thread::sleep(Duration::from_secs(3));
                tour += 1;
                let x = p2.lock().unwrap().clone();
                let jeu = crate::outil::jeu_actif();
                match (&en_cours, &jeu) {
                    (None, Some(j)) if x.actif => {
                        if let Some(pr) = x.jeux.get(j) {
                            let mut fait = vec![];
                            dossier = crate::jeux4::jeux_installes().into_iter().find(|g| g.nom == *j).map(|g| g.dir).unwrap_or_default();
                            // « Optimisation au lancement » gère déjà le mode performances : on ne s'en mêle pas
                            let optim: crate::jeux5::ReglageOptim = charger("optim_jeu");
                            if pr.plan > 0 && !(pr.plan == 1 && optim.actif && optim.performances) {
                                a_restaurer.0 = plan_actuel();
                                let s = if pr.plan == 1 { "SCHEME_MIN" } else { "SCHEME_MAX" };
                                let _ = std::process::Command::new("powercfg").args(["/setactive", s]).creation_flags(SANS_FENETRE).status();
                                fait.push(if pr.plan == 1 { "performances maximales" } else { "économie d'énergie" });
                            }
                            if let Some(v) = pr.volume {
                                a_restaurer.1 = volume_actuel();
                                let _ = crate::interface2::volume_maitre(v);
                                fait.push("volume réglé");
                            }
                            for n in &pr.fermer {
                                let _ = std::process::Command::new("taskkill").args(["/IM", n, "/F"]).creation_flags(SANS_FENETRE).status();
                            }
                            if !pr.fermer.is_empty() {
                                fait.push("programmes fermés");
                            }
                            let m = format!("{j} : profil appliqué ({})", fait.join(", "));
                            crate::coeur::noter("Profil de jeu", &m);
                            let mut y = p2.lock().unwrap();
                            y.dernier = m;
                            enregistrer("profils_jeux", &*y);
                        }
                        en_cours = jeu.clone();
                    }
                    (Some(_), None) => {
                        if let Some(pl) = a_restaurer.0.take() {
                            let _ = std::process::Command::new("powercfg").args(["/setactive", &pl]).creation_flags(SANS_FENETRE).status();
                        }
                        if let Some(v) = a_restaurer.1.take() {
                            let _ = crate::interface2::volume_maitre(v);
                        }
                        en_cours = None;
                        dossier.clear();
                    }
                    _ => {}
                }
                // la priorité est réappliquée : les jeux lancent souvent d'autres programmes après coup
                if let (Some(j), true) = (&en_cours, tour % 4 == 0) {
                    if let Some(pr) = x.jeux.get(j).filter(|pr| pr.priorite > 0) {
                        prioriser(&dossier, pr.priorite);
                    }
                }
            }
        });
        ProfilsJeux { p, choisi: String::new(), ajout: String::new() }
    }
}

impl Outil for ProfilsJeux {
    fn nom(&self) -> &'static str {
        "🎚 Profils de performance par jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Profils de performance par jeu");
        ui.weak("Pour chaque jeu, Outils règle tout seul la priorité, le plan d'alimentation et le volume quand il démarre, puis remet tout comme avant quand tu le quittes.");
        let avant = self.p.lock().unwrap().clone();
        let mut p = avant.clone();
        ui.checkbox(&mut p.actif, RichText::new("Activer les profils").strong());
        if !p.dernier.is_empty() {
            ui.weak(&p.dernier);
        }
        ui.horizontal(|ui| {
            let jeux: Vec<String> = crate::jeux4::jeux_installes().into_iter().map(|g| g.nom).filter(|n| !p.jeux.contains_key(n)).collect();
            egui::ComboBox::from_id_salt("ajout_profil").selected_text(if self.ajout.is_empty() { "Ajouter un jeu…" } else { &self.ajout }).show_ui(ui, |ui| {
                for j in jeux {
                    ui.selectable_value(&mut self.ajout, j.clone(), j);
                }
            });
            if ui.button("➕").clicked() && !self.ajout.is_empty() {
                p.jeux.insert(std::mem::take(&mut self.ajout), ProfilJeu::default());
            }
        });
        let mut retirer = None;
        for (nom, pr) in p.jeux.iter_mut() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(nom).size(16.0).strong());
                    if ui.small_button("✖").clicked() {
                        retirer = Some(nom.clone());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Priorité");
                    for (k, n) in ["normale", "au-dessus", "haute"].iter().enumerate() {
                        ui.selectable_value(&mut pr.priorite, k as u8, *n);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Alimentation");
                    for (k, n) in ["ne pas toucher", "performances maximales", "économie (portable)"].iter().enumerate() {
                        ui.selectable_value(&mut pr.plan, k as u8, *n);
                    }
                });
                ui.horizontal(|ui| {
                    let mut v = pr.volume.is_some();
                    if ui.checkbox(&mut v, "Volume").changed() {
                        pr.volume = v.then_some(0.5);
                    }
                    if let Some(x) = &mut pr.volume {
                        ui.add(egui::Slider::new(x, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0} %", v * 100.0)));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Fermer aussi :");
                    let t = pr.fermer.join(", ");
                    let mut e = t.clone();
                    if ui.add(egui::TextEdit::singleline(&mut e).hint_text("ex. Discord.exe, Spotify.exe").desired_width(260.0)).changed() {
                        pr.fermer = e.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                    }
                });
            });
        }
        if let Some(n) = retirer {
            p.jeux.remove(&n);
        }
        ui.weak("Priorité « haute » : le jeu passe avant le reste du PC (utile si des programmes tournent en fond). Évite de l'utiliser avec un streaming en même temps.");
        if p != avant {
            enregistrer("profils_jeux", &p);
            *self.p.lock().unwrap() = p;
        }
        let _ = &self.choisi;
    }
}

// ===================================================================================================
// 32 · Comparateur de FPS avant / après une mise à jour
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Session {
    pub jeu: String,
    pub date: u64,
    pub minutes: u32,
    pub moyenne: f32,
    pub bas: f32, // 1 % les plus bas
    pub pilote: String,
    pub windows: String,
}

/// Moyenne et « 1 % low » (moyenne du pourcent le plus bas) d'une série de FPS.
pub fn stats(fps: &[f32]) -> (f32, f32) {
    if fps.is_empty() {
        return (0.0, 0.0);
    }
    let moy = fps.iter().sum::<f32>() / fps.len() as f32;
    let mut t = fps.to_vec();
    t.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = (t.len() / 100).max(1);
    (moy, t[..n].iter().sum::<f32>() / n as f32)
}

/// Pour un jeu : (configuration d'avant, d'après, variation en %) si le pilote ou Windows a changé.
pub fn comparer(sessions: &[Session]) -> Option<(String, String, f32)> {
    let mut l: Vec<&Session> = sessions.iter().collect();
    l.sort_by_key(|s| s.date);
    let cfg = |s: &Session| format!("pilote {} · Windows {}", s.pilote, s.windows);
    let dernier = cfg(l.last()?);
    let apres: Vec<&&Session> = l.iter().rev().take_while(|s| cfg(s) == dernier).collect();
    let avant: Vec<&&Session> = l.iter().rev().skip(apres.len()).take_while(|s| cfg(s) == cfg(l[l.len() - apres.len() - 1])).collect();
    if avant.is_empty() {
        return None;
    }
    let m = |v: &[&&Session]| v.iter().map(|s| s.moyenne).sum::<f32>() / v.len() as f32;
    let (a, b) = (m(&avant), m(&apres));
    Some((cfg(avant[0]), dernier, (b - a) / a.max(1.0) * 100.0))
}

fn versions() -> (String, String) {
    let o = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "$v=(Get-CimInstance Win32_VideoController | Where-Object { $_.Name -notmatch 'Basic|Virtual|Parsec' } | Select-Object -First 1).DriverVersion; $r=Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion'; \"$v|$($r.CurrentBuild).$($r.UBR)\""]).creation_flags(SANS_FENETRE).output();
    let t = o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let (a, b) = t.split_once('|').unwrap_or(("?", "?"));
    (a.trim().to_string(), b.trim().to_string())
}

pub struct AvantApres {
    sessions: Arc<Mutex<Vec<Session>>>,
    choisi: String,
    en_cours: Arc<Mutex<Option<(String, Instant, usize)>>>,
}

impl AvantApres {
    pub fn new() -> Self {
        let s: Arc<Mutex<Vec<Session>>> = Arc::new(Mutex::new(charger("fps_sessions")));
        let en_cours: Arc<Mutex<Option<(String, Instant, usize)>>> = Arc::default();
        let (s2, e2) = (s.clone(), en_cours.clone());
        std::thread::spawn(move || {
            let (pilote, windows) = versions();
            let mut jeu: Option<(String, Instant)> = None;
            let mut fps: Vec<f32> = vec![];
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let actuel = crate::outil::jeu_actif();
                match (&jeu, &actuel) {
                    (None, Some(j)) => {
                        jeu = Some((j.clone(), Instant::now()));
                        fps.clear();
                    }
                    (Some((j, debut)), None) => {
                        let minutes = (debut.elapsed().as_secs() / 60) as u32;
                        if minutes >= 3 && fps.len() >= 60 {
                            let (moyenne, bas) = stats(&fps);
                            let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                            let mut v = s2.lock().unwrap();
                            v.push(Session { jeu: j.clone(), date: d, minutes, moyenne, bas, pilote: pilote.clone(), windows: windows.clone() });
                            let n = v.len();
                            if n > 600 {
                                v.drain(..n - 600);
                            }
                            enregistrer("fps_sessions", &*v);
                        }
                        jeu = None;
                    }
                    _ => {}
                }
                if let Some((j, debut)) = &jeu {
                    if let Some(l) = crate::telephone::MESURES.get() {
                        let (m, _) = l(true);
                        if m.fps > 0.0 {
                            fps.push(m.fps);
                        }
                    }
                    *e2.lock().unwrap() = Some((j.clone(), *debut, fps.len()));
                } else {
                    *e2.lock().unwrap() = None;
                }
            }
        });
        AvantApres { sessions: s, choisi: String::new(), en_cours }
    }
}

impl Outil for AvantApres {
    fn nom(&self) -> &'static str {
        "📉 FPS avant / après mise à jour"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("FPS avant / après une mise à jour");
        ui.weak("Outils note les FPS moyens de chaque partie (3 minutes minimum) avec la version du pilote graphique et de Windows. Après une mise à jour, tu vois tout de suite si tes jeux tournent moins bien.");
        if let Some((j, d, n)) = self.en_cours.lock().unwrap().clone() {
            ui.colored_label(Color32::LIGHT_GREEN, format!("● Mesure en cours : {j} ({} min, {n} mesures)", d.elapsed().as_secs() / 60));
            ui.ctx().request_repaint_after(Duration::from_secs(2));
        }
        let s = self.sessions.lock().unwrap().clone();
        if s.is_empty() {
            ui.weak("Aucune partie enregistrée pour l'instant : joue un peu (l'overlay ou le moniteur doit pouvoir mesurer les FPS).");
            return;
        }
        let mut jeux: Vec<String> = s.iter().map(|x| x.jeu.clone()).collect();
        jeux.sort();
        jeux.dedup();
        // alertes
        for j in &jeux {
            let l: Vec<Session> = s.iter().filter(|x| x.jeu == *j).cloned().collect();
            if let Some((a, b, pct)) = comparer(&l) {
                if pct <= -8.0 {
                    ui.colored_label(Color32::from_rgb(255, 120, 120), format!("⚠ {j} : {pct:.0} % de FPS depuis le passage de « {a} » à « {b} »"));
                } else if pct >= 8.0 {
                    ui.colored_label(Color32::LIGHT_GREEN, format!("✔ {j} : +{pct:.0} % de FPS depuis la mise à jour ({b})"));
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            for j in &jeux {
                ui.selectable_value(&mut self.choisi, j.clone(), j);
            }
        });
        if self.choisi.is_empty() {
            self.choisi = jeux[0].clone();
        }
        let l: Vec<&Session> = s.iter().filter(|x| x.jeu == self.choisi).collect();
        let max = l.iter().map(|x| x.moyenne).fold(30.0f32, f32::max);
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 160.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 6.0, ui.visuals().extreme_bg_color);
        let n = l.len().max(2);
        let mut prec_cfg = String::new();
        for (k, x) in l.iter().enumerate() {
            let px = r.left() + k as f32 / (n - 1) as f32 * r.width();
            let cfg = format!("{}{}", x.pilote, x.windows);
            if k > 0 && cfg != prec_cfg {
                ui.painter().line_segment([egui::pos2(px, r.top()), egui::pos2(px, r.bottom())], egui::Stroke::new(1.0_f32, Color32::from_rgb(250, 200, 80)));
            }
            prec_cfg = cfg;
            let y = r.bottom() - x.moyenne / max * r.height();
            ui.painter().circle_filled(egui::pos2(px, y), 4.0, Color32::from_rgb(255, 95, 162));
        }
        ui.weak("Chaque point = une partie ; trait jaune = changement de pilote ou de Windows.");
        egui::Grid::new("sessions_fps").striped(true).show(ui, |ui| {
            for x in l.iter().rev().take(15) {
                let (a, m, j) = crate::quotidien2::date_de_jours(x.date as i64 / 86_400);
                ui.label(format!("{j:02}/{m:02}/{a}"));
                ui.label(RichText::new(format!("{:.0} FPS", x.moyenne)).strong());
                ui.weak(format!("1 % bas : {:.0}", x.bas));
                ui.weak(format!("{} min", x.minutes));
                ui.weak(format!("pilote {}", x.pilote));
                ui.end_row();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps() {
        let mut v: Vec<f32> = vec![100.0; 198];
        v.extend([20.0, 30.0]);
        let (m, b) = stats(&v);
        assert!((m - 99.25).abs() < 0.01);
        assert_eq!(b, 25.0);
        let s = |d: u64, f: f32, p: &str| Session { jeu: "X".into(), date: d, minutes: 30, moyenne: f, bas: f / 2.0, pilote: p.into(), windows: "26100.1".into() };
        let l = vec![s(1, 120.0, "551.1"), s(2, 118.0, "551.1"), s(3, 100.0, "560.0"), s(4, 98.0, "560.0")];
        let (a, b, pct) = comparer(&l).unwrap();
        assert!(a.contains("551.1") && b.contains("560.0"));
        assert!((pct + 17.0).abs() < 1.0, "{pct}");
        assert!(comparer(&l[..2]).is_none());
        assert_eq!(classe(2), 0x80);
    }
}
