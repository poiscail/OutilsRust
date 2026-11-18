//! Analyseur d'espace disque : carte en blocs (treemap) des dossiers et recherche de fichiers en double.
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

pub struct Noeud {
    pub nom: String,
    pub taille: u64,
    pub enfants: Vec<Noeud>, // triés du plus gros au plus petit ; vide pour un fichier
    pub dossier: bool,
}

/// Parcourt `chemin` ; `fichiers` reçoit (chemin, taille) de chaque fichier pour les doublons.
fn parcourir(chemin: &Path, compte: &AtomicU64, stop: &AtomicBool, fichiers: &mut Vec<(PathBuf, u64)>) -> Noeud {
    let nom = chemin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| chemin.display().to_string());
    let mut n = Noeud { nom, taille: 0, enfants: vec![], dossier: true };
    let Ok(lecture) = std::fs::read_dir(chemin) else { return n };
    for e in lecture.flatten() {
        if stop.load(Relaxed) {
            break;
        }
        let Ok(t) = e.file_type() else { continue };
        if t.is_symlink() {
            continue; // jonctions : on évite les boucles et les doubles comptes
        }
        if t.is_dir() {
            n.enfants.push(parcourir(&e.path(), compte, stop, fichiers));
        } else if let Ok(m) = e.metadata() {
            compte.fetch_add(1, Relaxed);
            fichiers.push((e.path(), m.len()));
            n.enfants.push(Noeud { nom: e.file_name().to_string_lossy().into_owned(), taille: m.len(), enfants: vec![], dossier: false });
        }
    }
    n.enfants.sort_by(|a, b| b.taille.cmp(&a.taille));
    n.taille = n.enfants.iter().map(|e| e.taille).sum();
    n
}

/// Découpe `zone` en rectangles proches du carré, proportionnels à `tailles` (déjà triées décroissantes).
pub fn squarifier(tailles: &[u64], zone: Rect) -> Vec<Rect> {
    let total: f64 = tailles.iter().map(|&t| t as f64).sum();
    let mut sortie = Vec::with_capacity(tailles.len());
    if total <= 0.0 || zone.area() <= 0.0 {
        return vec![Rect::NOTHING; tailles.len()];
    }
    let echelle = zone.area() as f64 / total;
    let aires: Vec<f64> = tailles.iter().map(|&t| t as f64 * echelle).collect();
    let mut reste = zone;
    let mut i = 0;
    while i < aires.len() {
        let cote = reste.width().min(reste.height()) as f64;
        // ajoute des éléments à la rangée tant que le pire rapport s'améliore
        let pire = |r: &[f64]| {
            let s: f64 = r.iter().sum();
            let (mx, mn) = r.iter().fold((0f64, f64::MAX), |(a, b), &x| (a.max(x), b.min(x)));
            ((cote * cote * mx) / (s * s)).max((s * s) / (cote * cote * mn.max(1e-9)))
        };
        let mut j = i + 1;
        while j < aires.len() && pire(&aires[i..=j]) <= pire(&aires[i..j]) {
            j += 1;
        }
        let rangee = &aires[i..j];
        let s: f64 = rangee.iter().sum();
        let epaisseur = (s / cote) as f32;
        let mut pos = 0f32;
        if reste.width() >= reste.height() {
            for &a in rangee {
                let h = (a / s) as f32 * reste.height();
                sortie.push(Rect::from_min_size(Pos2::new(reste.min.x, reste.min.y + pos), egui::vec2(epaisseur, h)));
                pos += h;
            }
            reste.min.x += epaisseur;
        } else {
            for &a in rangee {
                let w = (a / s) as f32 * reste.width();
                sortie.push(Rect::from_min_size(Pos2::new(reste.min.x + pos, reste.min.y), egui::vec2(w, epaisseur)));
                pos += w;
            }
            reste.min.y += epaisseur;
        }
        i = j;
    }
    sortie
}

pub fn taille_lisible(o: u64) -> String {
    let u = ["o", "Ko", "Mo", "Go", "To"];
    let mut x = o as f64;
    let mut i = 0;
    while x >= 1024.0 && i < u.len() - 1 {
        x /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{o} o") } else { format!("{x:.1} {}", u[i]) }
}

fn empreinte(p: &Path, max: Option<u64>) -> Option<[u8; 32]> {
    let f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut lecteur: Box<dyn Read> = match max {
        Some(m) => Box::new(f.take(m)),
        None => Box::new(f),
    };
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = lecteur.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().into())
}

/// Groupes de fichiers identiques (taille, puis début du fichier, puis contenu entier), du plus coûteux au moins coûteux.
pub fn doublons(fichiers: &[(PathBuf, u64)], min: u64, stop: &AtomicBool, fait: &AtomicU64) -> Vec<(u64, Vec<PathBuf>)> {
    let mut par_taille: HashMap<u64, Vec<&PathBuf>> = HashMap::new();
    for (p, t) in fichiers {
        if *t >= min {
            par_taille.entry(*t).or_default().push(p);
        }
    }
    let mut groupes = vec![];
    for (t, liste) in par_taille.into_iter().filter(|(_, l)| l.len() > 1) {
        let mut debut: HashMap<[u8; 32], Vec<&PathBuf>> = HashMap::new();
        for p in liste {
            if stop.load(Relaxed) {
                return groupes;
            }
            fait.fetch_add(1, Relaxed);
            if let Some(e) = empreinte(p, Some(64 * 1024)) {
                debut.entry(e).or_default().push(p);
            }
        }
        for l in debut.into_values().filter(|l| l.len() > 1) {
            let mut complet: HashMap<[u8; 32], Vec<PathBuf>> = HashMap::new();
            for p in l {
                if let Some(e) = if t <= 64 * 1024 { empreinte(p, Some(t)) } else { empreinte(p, None) } {
                    complet.entry(e).or_default().push(p.clone());
                }
            }
            groupes.extend(complet.into_values().filter(|g| g.len() > 1).map(|mut g| {
                g.sort();
                (t, g)
            }));
        }
    }
    groupes.sort_by(|a, b| (b.0 * (b.1.len() as u64 - 1)).cmp(&(a.0 * (a.1.len() as u64 - 1))));
    groupes
}

/// Envoie à la corbeille (annulable).
pub fn corbeille(p: &Path) -> bool {
    use windows_sys::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
    let source: Vec<u16> = p.as_os_str().encode_wide_nul2();
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE;
    op.pFrom = source.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT) as u16;
    unsafe { SHFileOperationW(&mut op) == 0 && op.fAnyOperationsAborted == 0 }
}

trait DoubleNul {
    fn encode_wide_nul2(&self) -> Vec<u16>;
}
impl DoubleNul for std::ffi::OsStr {
    fn encode_wide_nul2(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0, 0]).collect() // liste terminée par deux zéros
    }
}

/// Ouvre l'explorateur sur le fichier sélectionné.
pub fn montrer_dans_explorateur(p: &Path) {
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
}

#[derive(Default)]
struct Scan {
    racine: Option<Arc<Noeud>>,
    fichiers: Arc<Vec<(PathBuf, u64)>>,
    doublons: Option<Vec<(u64, Vec<PathBuf>)>>,
    en_cours: bool,
    doublons_en_cours: bool,
}

pub struct Disque {
    dossier: PathBuf,
    scan: Arc<Mutex<Scan>>,
    compte: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    chemin: Vec<usize>, // indices des enfants depuis la racine (zoom)
    min_mo: u64,
    vue_doublons: bool,
    message: String,
}

impl Disque {
    pub fn new() -> Self {
        Disque {
            dossier: PathBuf::from(std::env::var("USERPROFILE").unwrap_or("C:\\".into())),
            scan: Arc::default(), compte: Arc::default(), stop: Arc::default(), chemin: vec![], min_mo: 1, vue_doublons: false, message: String::new(),
        }
    }

    fn lancer(&mut self) {
        self.stop.store(false, Relaxed);
        self.compte.store(0, Relaxed);
        self.chemin.clear();
        {
            let mut s = self.scan.lock().unwrap();
            *s = Scan { en_cours: true, ..Default::default() };
        }
        let (d, scan, compte, stop) = (self.dossier.clone(), self.scan.clone(), self.compte.clone(), self.stop.clone());
        std::thread::spawn(move || {
            let mut fichiers = vec![];
            let mut n = parcourir(&d, &compte, &stop, &mut fichiers);
            n.nom = d.display().to_string();
            let mut s = scan.lock().unwrap();
            s.racine = Some(Arc::new(n));
            s.fichiers = Arc::new(fichiers);
            s.en_cours = false;
        });
    }

    fn chercher_doublons(&mut self) {
        self.stop.store(false, Relaxed);
        self.compte.store(0, Relaxed);
        let fichiers = {
            let mut s = self.scan.lock().unwrap();
            s.doublons_en_cours = true;
            s.fichiers.clone()
        };
        let (scan, stop, compte, min) = (self.scan.clone(), self.stop.clone(), self.compte.clone(), self.min_mo << 20);
        std::thread::spawn(move || {
            let g = doublons(&fichiers, min.max(1), &stop, &compte);
            let mut s = scan.lock().unwrap();
            s.doublons = Some(g);
            s.doublons_en_cours = false;
        });
    }

    fn chemin_reel(racine: &Path, noms: &[String]) -> PathBuf {
        noms.iter().fold(racine.to_path_buf(), |p, n| p.join(n))
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Analyseur d'espace disque");
        let (en_cours, dbl_en_cours, racine) = {
            let s = self.scan.lock().unwrap();
            (s.en_cours, s.doublons_en_cours, s.racine.clone())
        };
        ui.horizontal(|ui| {
            ui.label("Dossier :");
            ui.monospace(self.dossier.display().to_string());
            if ui.add_enabled(!en_cours, egui::Button::new("Choisir…")).clicked() {
                if let Some(d) = rfd::FileDialog::new().set_directory(&self.dossier).pick_folder() {
                    self.dossier = d;
                }
            }
            if en_cours || dbl_en_cours {
                ui.spinner();
                ui.label(format!("{} fichiers…", self.compte.load(Relaxed)));
                if ui.button("Arrêter").clicked() {
                    self.stop.store(true, Relaxed);
                }
            } else if ui.button("🔍 Analyser").clicked() {
                self.lancer();
            }
        });
        if en_cours || dbl_en_cours {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        let Some(racine) = racine else {
            ui.weak("Choisis un dossier (ou un disque entier, ex. C:\\) et lance l'analyse.");
            return;
        };
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.vue_doublons, false, "🗺 Carte");
            ui.selectable_value(&mut self.vue_doublons, true, "👯 Doublons");
        });
        ui.separator();
        if self.vue_doublons {
            self.ui_doublons(ui, dbl_en_cours);
        } else {
            self.ui_carte(ui, &racine);
        }
    }

    fn ui_carte(&mut self, ui: &mut egui::Ui, racine: &Arc<Noeud>) {
        // descend dans l'arbre selon le chemin de zoom
        let mut noeud: &Noeud = racine;
        let mut noms = vec![];
        let mut valide = 0;
        for &i in &self.chemin {
            match noeud.enfants.get(i) {
                Some(e) if e.dossier => {
                    noms.push(e.nom.clone());
                    noeud = e;
                    valide += 1;
                }
                _ => break,
            }
        }
        self.chemin.truncate(valide);
        let base = PathBuf::from(&racine.nom);
        ui.horizontal_wrapped(|ui| {
            if ui.link(&racine.nom).clicked() {
                self.chemin.clear();
            }
            for (k, n) in noms.iter().enumerate() {
                ui.label("›");
                if ui.link(n).clicked() {
                    self.chemin.truncate(k + 1);
                }
            }
            ui.strong(format!("  {}", taille_lisible(noeud.taille)));
        });
        let visibles: Vec<&Noeud> = noeud.enfants.iter().filter(|e| e.taille > 0).take(300).collect();
        let liste_h = 170.0;
        let taille = egui::vec2(ui.available_width(), (ui.available_height() - liste_h).max(200.0));
        let (zone, _) = ui.allocate_exact_size(taille, Sense::hover());
        let rects = squarifier(&visibles.iter().map(|e| e.taille).collect::<Vec<_>>(), zone);
        let peintre = ui.painter_at(zone);
        let mut clic = None;
        for (k, (e, r)) in visibles.iter().zip(&rects).enumerate() {
            if r.width() < 1.0 || r.height() < 1.0 {
                continue;
            }
            let teinte = (k as f32 * 0.13) % 1.0;
            let fond: Color32 = egui::ecolor::Hsva::new(teinte, if e.dossier { 0.55 } else { 0.25 }, 0.55, 1.0).into();
            let rep = ui.interact(*r, ui.id().with(("bloc", k)), Sense::click());
            peintre.rect(r.shrink(1.0), 3.0f32, if rep.hovered() { fond.gamma_multiply(1.3) } else { fond }, Stroke::new(1.0f32, Color32::from_black_alpha(120)), StrokeKind::Inside);
            if r.width() > 60.0 && r.height() > 30.0 {
                peintre.text(r.min + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, &e.nom, egui::FontId::proportional(13.0), Color32::WHITE);
                peintre.text(r.min + egui::vec2(6.0, 20.0), egui::Align2::LEFT_TOP, taille_lisible(e.taille), egui::FontId::proportional(11.0), Color32::from_white_alpha(200));
            }
            let chemin = Self::chemin_reel(&base, &noms).join(&e.nom);
            let rep = rep.on_hover_text(format!("{}\n{}{}", chemin.display(), taille_lisible(e.taille), if e.dossier { "\nClic : entrer · clic droit : menu" } else { "" }));
            if rep.clicked() && e.dossier {
                clic = Some(k);
            }
            rep.context_menu(|ui| {
                if ui.button("Montrer dans l'explorateur").clicked() {
                    montrer_dans_explorateur(&chemin);
                    ui.close_menu();
                }
                if ui.button("🗑 Mettre à la corbeille").clicked() {
                    self.message = if corbeille(&chemin) { format!("{} mis à la corbeille (relance l'analyse pour mettre à jour).", e.nom) } else { "Suppression impossible.".into() };
                    ui.close_menu();
                }
            });
        }
        if let Some(k) = clic {
            // retrouve l'indice réel dans enfants (les visibles sont un préfixe, tailles > 0)
            self.chemin.push(k);
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        egui::ScrollArea::vertical().id_salt("liste_disque").max_height(liste_h - 30.0).show(ui, |ui| {
            for e in noeud.enfants.iter().take(50) {
                ui.horizontal(|ui| {
                    let part = e.taille as f32 / noeud.taille.max(1) as f32;
                    ui.add(egui::ProgressBar::new(part).desired_width(120.0).text(format!("{:.0} %", part * 100.0)));
                    ui.label(format!("{} {}", if e.dossier { "📁" } else { "📄" }, e.nom));
                    ui.weak(taille_lisible(e.taille));
                });
            }
        });
    }

    fn ui_doublons(&mut self, ui: &mut egui::Ui, en_cours: bool) {
        ui.horizontal(|ui| {
            ui.label("Taille minimale");
            ui.add(egui::DragValue::new(&mut self.min_mo).range(0..=10_000).suffix(" Mo"));
            if ui.add_enabled(!en_cours, egui::Button::new("Chercher les doublons")).clicked() {
                self.chercher_doublons();
            }
        });
        let groupes = self.scan.lock().unwrap().doublons.clone();
        let Some(groupes) = groupes else {
            ui.weak("Compare les fichiers de même taille, puis leur contenu (SHA-256).");
            return;
        };
        let perdu: u64 = groupes.iter().map(|(t, g)| t * (g.len() as u64 - 1)).sum();
        ui.label(format!("{} groupe(s), {} récupérables.", groupes.len(), taille_lisible(perdu)));
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        let mut supprime = None;
        egui::ScrollArea::vertical().id_salt("doublons").show(ui, |ui| {
            for (gi, (t, g)) in groupes.iter().enumerate().take(500) {
                egui::CollapsingHeader::new(format!("{} × {} · {}", g.len(), taille_lisible(*t), g[0].file_name().unwrap_or_default().to_string_lossy()))
                    .id_salt(("grp", gi)).show(ui, |ui| {
                        for p in g {
                            ui.horizontal(|ui| {
                                if ui.small_button("📂").on_hover_text("Montrer").clicked() {
                                    montrer_dans_explorateur(p);
                                }
                                if ui.small_button("🗑").on_hover_text("Corbeille").clicked() {
                                    supprime = Some((gi, p.clone()));
                                }
                                ui.label(p.display().to_string());
                            });
                        }
                    });
            }
        });
        if let Some((gi, p)) = supprime {
            if corbeille(&p) {
                self.message = format!("{} mis à la corbeille.", p.display());
                let mut s = self.scan.lock().unwrap();
                if let Some(gr) = s.doublons.as_mut() {
                    gr[gi].1.retain(|x| x != &p);
                    if gr[gi].1.len() < 2 {
                        gr.remove(gi);
                    }
                }
            } else {
                self.message = "Suppression impossible.".into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carte_couvre_la_zone() {
        let zone = Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 400.0));
        let t = [500, 300, 100, 60, 40];
        let r = squarifier(&t, zone);
        assert_eq!(r.len(), 5);
        let aire: f32 = r.iter().map(|x| x.area()).sum();
        assert!((aire - zone.area()).abs() < 1.0);
        assert!((r[0].area() / zone.area() - 0.5).abs() < 0.01);
        assert!(r.iter().all(|x| zone.expand(0.5).contains_rect(*x)));
    }

    #[test]
    fn tailles() {
        assert_eq!(taille_lisible(512), "512 o");
        assert_eq!(taille_lisible(1536), "1.5 Ko");
        assert_eq!(taille_lisible(3 << 30), "3.0 Go");
    }

    #[test]
    fn trouve_les_doublons() {
        let d = std::env::temp_dir().join(format!("outils_dbl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sous")).unwrap();
        std::fs::write(d.join("a.txt"), "pareil").unwrap();
        std::fs::write(d.join("sous/b.txt"), "pareil").unwrap();
        std::fs::write(d.join("c.txt"), "autre!").unwrap(); // même taille, contenu différent
        let (c, s) = (AtomicU64::new(0), AtomicBool::new(false));
        let mut f = vec![];
        let n = parcourir(&d, &c, &s, &mut f);
        assert_eq!(n.taille, 18);
        let g = doublons(&f, 1, &s, &c);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].1.len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
