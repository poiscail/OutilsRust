//! Mises à jour des logiciels via winget : liste ce qui peut être mis à jour, met à jour un logiciel ou tout.
use eframe::egui::{self, Color32};
use std::collections::BTreeSet;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::{Arc, Mutex};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq)]
pub struct Paquet {
    pub nom: String,
    pub id: String,
    pub version: String,
    pub dispo: String,
}

/// Lit le tableau de `winget upgrade` (colonnes alignées, en-têtes traduits selon la langue).
pub fn lire_tableau(sortie: &str) -> Vec<Paquet> {
    // la barre de progression de winget réécrit la ligne avec \r : on garde le dernier morceau
    let lignes: Vec<String> = sortie.lines().map(|l| l.rsplit('\r').next().unwrap_or("").trim_end().to_string()).collect();
    let Some(sep) = lignes.iter().position(|l| l.len() > 10 && l.chars().all(|c| c == '-')) else { return vec![] };
    if sep == 0 {
        return vec![];
    }
    let entete: Vec<char> = lignes[sep - 1].chars().collect();
    let mut debuts = vec![];
    for i in 0..entete.len() {
        if entete[i] != ' ' && (i == 0 || entete[i - 1] == ' ') {
            debuts.push(i);
        }
    }
    if debuts.len() < 4 {
        return vec![];
    }
    let col = |l: &[char], k: usize| -> String {
        let a = debuts[k].min(l.len());
        let b = debuts.get(k + 1).copied().unwrap_or(l.len()).min(l.len());
        l[a..b].iter().collect::<String>().trim().to_string()
    };
    let mut v = vec![];
    for l in &lignes[sep + 1..] {
        let c: Vec<char> = l.chars().collect();
        if c.len() <= debuts[3] {
            continue; // ligne de résumé ou vide
        }
        let p = Paquet { nom: col(&c, 0), id: col(&c, 1), version: col(&c, 2), dispo: col(&c, 3) };
        if !p.id.is_empty() && !p.id.contains(' ') && !p.dispo.is_empty() {
            v.push(p);
        }
    }
    v
}

fn winget(args: &[&str]) -> Result<String, String> {
    // chcp 65001 : sortie en UTF-8 (accents)
    let commande = format!("chcp 65001>nul & winget {}", args.join(" "));
    let o = Command::new("cmd.exe").args(["/C", &commande]).creation_flags(CREATE_NO_WINDOW).output()
        .map_err(|e| e.to_string())?;
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    if s.contains("n'est pas reconnu") || s.contains("is not recognized") {
        return Err("winget introuvable : installe « App Installer » depuis le Microsoft Store.".into());
    }
    Ok(s)
}

#[derive(Default)]
struct Etat {
    paquets: Option<Vec<Paquet>>,
    occupe: Option<String>,
    journal: Vec<String>,
    faits: BTreeSet<String>,
}

pub struct MajLogiciels {
    e: Arc<Mutex<Etat>>,
    choisis: BTreeSet<String>,
}

impl MajLogiciels {
    pub fn new() -> Self {
        MajLogiciels { e: Arc::default(), choisis: BTreeSet::new() }
    }

    fn chercher(&self) {
        let e = self.e.clone();
        e.lock().unwrap().occupe = Some("Recherche des mises à jour…".into());
        std::thread::spawn(move || {
            let r = winget(&["upgrade", "--include-unknown", "--accept-source-agreements", "--disable-interactivity"]);
            let mut x = e.lock().unwrap();
            x.occupe = None;
            match r {
                Ok(s) => {
                    x.paquets = Some(lire_tableau(&s));
                    x.faits.clear();
                }
                Err(m) => x.journal.insert(0, m),
            }
        });
    }

    fn installer(&self, ids: Vec<String>) {
        let e = self.e.clone();
        std::thread::spawn(move || {
            for (i, id) in ids.iter().enumerate() {
                e.lock().unwrap().occupe = Some(format!("Mise à jour de {id} ({}/{})…", i + 1, ids.len()));
                let r = winget(&["upgrade", "--id", id, "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"]);
                let mut x = e.lock().unwrap();
                match r {
                    Ok(s) => {
                        let ok = s.contains("réussi") || s.contains("Successfully") || s.contains("succès");
                        x.journal.insert(0, format!("{} {id}", if ok { "✔" } else { "⚠" }) + if ok { "" } else { " : voir winget (peut demander les droits admin)" });
                        if ok {
                            x.faits.insert(id.clone());
                        }
                    }
                    Err(m) => x.journal.insert(0, format!("✖ {id} : {m}")),
                }
            }
            e.lock().unwrap().occupe = None;
            crate::systeme::notifier("Mises à jour", &format!("{} logiciel(s) traités.", ids.len()));
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mises à jour des logiciels");
        let (paquets, occupe, journal, faits) = {
            let x = self.e.lock().unwrap();
            (x.paquets.clone(), x.occupe.clone(), x.journal.clone(), x.faits.clone())
        };
        ui.horizontal(|ui| {
            if let Some(o) = &occupe {
                ui.spinner();
                ui.label(o);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
            } else {
                if ui.button("🔍 Chercher les mises à jour").clicked() {
                    self.chercher();
                }
                let restants: Vec<String> = paquets.iter().flatten().filter(|p| !faits.contains(&p.id)).map(|p| p.id.clone()).collect();
                if !self.choisis.is_empty() && ui.button(format!("⬆ Mettre à jour la sélection ({})", self.choisis.len())).clicked() {
                    self.installer(self.choisis.iter().cloned().collect());
                    self.choisis.clear();
                }
                if !restants.is_empty() && ui.button(format!("⬆ Tout mettre à jour ({})", restants.len())).clicked() {
                    self.installer(restants);
                }
            }
        });
        ui.weak("Passe par winget (gestionnaire de paquets de Windows). Certaines mises à jour demandent les droits admin ou de fermer le logiciel.");
        ui.separator();
        match &paquets {
            None => {
                ui.weak("Clique sur « Chercher les mises à jour ».");
            }
            Some(v) if v.is_empty() => {
                ui.label("✔ Tout est à jour.");
            }
            Some(v) => {
                egui::ScrollArea::vertical().id_salt("maj_log").max_height(ui.available_height() - 120.0).show(ui, |ui| {
                    egui::Grid::new("grille_maj").num_columns(4).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
                        ui.label("");
                        ui.strong("Logiciel");
                        ui.strong("Version");
                        ui.strong("Disponible");
                        ui.end_row();
                        for p in v {
                            if faits.contains(&p.id) {
                                ui.colored_label(Color32::LIGHT_GREEN, "✔");
                            } else {
                                let mut c = self.choisis.contains(&p.id);
                                if ui.checkbox(&mut c, "").changed() {
                                    if c {
                                        self.choisis.insert(p.id.clone());
                                    } else {
                                        self.choisis.remove(&p.id);
                                    }
                                }
                            }
                            ui.label(&p.nom).on_hover_text(&p.id);
                            ui.weak(&p.version);
                            ui.strong(&p.dispo);
                            ui.end_row();
                        }
                    });
                });
            }
        }
        if !journal.is_empty() {
            ui.separator();
            for l in journal.iter().take(8) {
                ui.label(l);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tableau() {
        let s = "   - \r   \\ \rNom                      ID                       Version       Disponible    Source\n\
-------------------------------------------------------------------------------------------\n\
Mozilla Firefox (x64 fr) Mozilla.Firefox          120.0         121.0         winget\n\
Discord                  Discord.Discord          1.0.9015      1.0.9020      winget\n\
2 mises à niveau disponibles.\n";
        let v = lire_tableau(s);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], Paquet { nom: "Mozilla Firefox (x64 fr)".into(), id: "Mozilla.Firefox".into(), version: "120.0".into(), dispo: "121.0".into() });
        assert_eq!(v[1].id, "Discord.Discord");
        assert!(lire_tableau("Aucune mise à jour disponible.").is_empty());
    }
}
