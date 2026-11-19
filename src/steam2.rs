//! Steam : succès (55), jeux « à finir » (56), alertes de sortie (57), jeux gratuits de la semaine (58).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("Outils").build()
}

/// (appid, nom) des jeux Steam installés.
fn jeux_installes() -> Vec<(u32, String)> {
    let mut v: Vec<(u32, String)> = crate::lanceur::scan_steam(crate::lanceur::steam_racine().as_deref()).into_iter()
        .filter_map(|j| Some((j.id.strip_prefix("steam:")?.parse().ok()?, j.nom))).filter(|(_, n)| !n.contains("Redistributable") && !n.contains("Steamworks")).collect();
    v.sort_by_key(|x| x.1.to_lowercase());
    v.dedup();
    v
}

// ---------------------------------------------------------------------------------------------------
// 55 · Succès
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Succes {
    pub id: String,
    pub nom: String,
    pub description: String,
    pub obtenu: bool,
    pub global: Option<f64>, // % des joueurs qui l'ont
}

/// Lit le XML public des succès d'un profil.
pub fn lire_succes_xml(x: &str) -> Vec<Succes> {
    let re = regex::Regex::new(r#"(?s)<achievement closed="(\d)">(.*?)</achievement>"#).unwrap();
    let champ = |b: &str, t: &str| -> String {
        regex::Regex::new(&format!(r"(?s)<{t}>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</{t}>")).unwrap().captures(b).map(|c| c[1].trim().to_string()).unwrap_or_default()
    };
    re.captures_iter(x).map(|c| Succes { id: champ(&c[2], "apiname"), nom: champ(&c[2], "name"), description: champ(&c[2], "description"), obtenu: &c[1] == "1", global: None }).collect()
}

pub struct SuccesUi {
    jeux: Vec<(u32, String)>,
    choisi: Option<u32>,
    res: Arc<Mutex<Option<Result<Vec<Succes>, String>>>>,
}

impl SuccesUi {
    pub fn new() -> Self {
        SuccesUi { jeux: vec![], choisi: None, res: Arc::default() }
    }
}

impl Outil for SuccesUi {
    fn nom(&self) -> &'static str {
        "🏆 Succès Steam"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Succès Steam");
        let Some(sid) = crate::promos::steamid_local() else {
            ui.weak("Steam introuvable ou jamais connecté sur ce PC.");
            return;
        };
        if self.jeux.is_empty() {
            self.jeux = jeux_installes();
        }
        ui.weak("Ton profil Steam doit avoir les « détails des jeux » publics (Confidentialité).");
        let nom = self.choisi.and_then(|c| self.jeux.iter().find(|j| j.0 == c)).map(|j| j.1.clone()).unwrap_or_else(|| "Choisis un jeu".into());
        egui::ComboBox::from_id_salt("jeu_succes").selected_text(nom).width(320.0).show_ui(ui, |ui| {
            for (id, n) in &self.jeux {
                if ui.selectable_label(self.choisi == Some(*id), n).clicked() {
                    self.choisi = Some(*id);
                    let (r, sid, id) = (self.res.clone(), sid.clone(), *id);
                    *r.lock().unwrap() = None;
                    std::thread::spawn(move || {
                        let res = (|| {
                            let x = agent().get(&format!("https://steamcommunity.com/profiles/{sid}/stats/{id}/achievements/")).query("xml", "1").query("l", "french")
                                .call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
                            let mut s = lire_succes_xml(&x);
                            if s.is_empty() {
                                return Err("pas de succès, ou profil privé".to_string());
                            }
                            if let Ok(g) = agent().get("https://api.steampowered.com/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/").query("gameid", &id.to_string()).call() {
                                if let Ok(j) = g.into_json::<serde_json::Value>() {
                                    let p: BTreeMap<String, f64> = j["achievementpercentages"]["achievements"].as_array().into_iter().flatten()
                                        .filter_map(|a| Some((a["name"].as_str()?.to_lowercase(), a["percent"].as_f64().or_else(|| a["percent"].as_str()?.parse().ok())?))).collect();
                                    for x in &mut s {
                                        x.global = p.get(&x.id.to_lowercase()).copied();
                                    }
                                }
                            }
                            Ok(s)
                        })();
                        *r.lock().unwrap() = Some(res);
                    });
                }
            }
        });
        if self.choisi.is_none() {
            return;
        }
        let res = self.res.lock().unwrap().clone();
        match res {
            None => {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            Some(Ok(s)) => {
                let n = s.iter().filter(|x| x.obtenu).count();
                ui.add(egui::ProgressBar::new(n as f32 / s.len().max(1) as f32).text(format!("{n} / {} succès", s.len())));
                let mut restants: Vec<&Succes> = s.iter().filter(|x| !x.obtenu).collect();
                restants.sort_by(|a, b| b.global.unwrap_or(0.0).total_cmp(&a.global.unwrap_or(0.0)));
                ui.strong("Les plus faciles qu'il te reste (obtenus par le plus de joueurs)");
                egui::ScrollArea::vertical().id_salt("succes").show(ui, |ui| {
                    for x in restants {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&x.nom).strong());
                            if let Some(g) = x.global {
                                ui.colored_label(Color32::from_rgb(255, 170, 200), format!("{g:.1} %"));
                            }
                            ui.weak(&x.description);
                        });
                    }
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 56 · Jeux à finir (installés mais jamais ou peu lancés)
// ---------------------------------------------------------------------------------------------------

/// Temps de jeu (minutes) par appid, lus dans localconfig.vdf.
pub fn temps_de_jeu(vdf: &str) -> BTreeMap<u32, u64> {
    let re = regex::Regex::new(r#"(?s)"(\d+)"\s*\{([^{}]*)\}"#).unwrap();
    let pt = regex::Regex::new(r#""Playtime"\s*"(\d+)""#).unwrap();
    re.captures_iter(vdf).filter_map(|c| Some((c[1].parse().ok()?, pt.captures(&c[2])?[1].parse().ok()?))).collect()
}

fn localconfig() -> Option<String> {
    let r = crate::lanceur::steam_racine()?;
    let sid: u64 = crate::promos::steamid_local()?.parse().ok()?;
    let compte = sid.checked_sub(76561197960265728)?;
    std::fs::read_to_string(r.join("userdata").join(compte.to_string()).join("config").join("localconfig.vdf")).ok()
}

pub struct AFinir {
    liste: Option<Vec<(u32, String, u64)>>,
    seuil_h: u64,
}

impl AFinir {
    pub fn new() -> Self {
        AFinir { liste: None, seuil_h: 2 }
    }
}

impl Outil for AFinir {
    fn nom(&self) -> &'static str {
        "📋 Jeux à finir"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Jeux à finir");
        ui.weak("Tes jeux Steam installés, du moins joué au plus joué : de quoi vider la pile de la honte.");
        if self.liste.is_none() {
            let t = localconfig().map(|v| temps_de_jeu(&v)).unwrap_or_default();
            let mut l: Vec<(u32, String, u64)> = jeux_installes().into_iter().map(|(id, n)| (id, n, t.get(&id).copied().unwrap_or(0))).collect();
            l.sort_by_key(|x| x.2);
            self.liste = Some(l);
        }
        ui.horizontal(|ui| {
            ui.label("Moins de");
            ui.add(egui::DragValue::new(&mut self.seuil_h).range(0..=100).suffix(" h"));
            if ui.button("⟳").clicked() {
                self.liste = None;
            }
        });
        let l = self.liste.clone().unwrap_or_default();
        let jamais = l.iter().filter(|x| x.2 == 0).count();
        ui.label(RichText::new(format!("{jamais} jeu(x) jamais lancé(s) sur {}", l.len())).strong());
        egui::ScrollArea::vertical().id_salt("afinir").show(ui, |ui| {
            for (id, n, m) in l.iter().filter(|x| x.2 <= self.seuil_h * 60) {
                ui.horizontal(|ui| {
                    if ui.small_button("▶").on_hover_text("Lancer").clicked() {
                        let _ = std::process::Command::new("explorer").arg(format!("steam://rungameid/{id}")).spawn();
                    }
                    ui.label(n);
                    if *m == 0 { ui.colored_label(Color32::from_rgb(255, 170, 200), "jamais lancé") } else { ui.weak(format!("{} h {:02}", m / 60, m % 60)) };
                    if ui.small_button("⏱").on_hover_text("Durée de vie sur HowLongToBeat").clicked() {
                        let _ = std::process::Command::new("explorer").arg(format!("https://howlongtobeat.com/?q={}", n.replace(' ', "%20"))).spawn();
                    }
                });
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 57 · Alertes de sortie / 58 · Jeux gratuits
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Attendu {
    pub appid: u32,
    pub nom: String,
    pub date: String,
    pub sorti: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Gratuit {
    pub id: u64,
    pub titre: String,
    pub plateformes: String,
    pub valeur: String,
    pub fin: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct DonneesSorties {
    attendus: Vec<Attendu>,
    gratuits: Vec<Gratuit>,
    vus: BTreeSet<u64>,
    derniere: i64,
}

pub fn lire_sortie(appid: u32, j: &serde_json::Value) -> Option<(String, String, bool)> {
    let d = &j[appid.to_string()]["data"];
    Some((d["name"].as_str()?.to_string(), d["release_date"]["date"].as_str().unwrap_or("").to_string(), d["release_date"]["coming_soon"] == false))
}

pub fn lire_gratuits(j: &serde_json::Value) -> Vec<Gratuit> {
    j.as_array().into_iter().flatten().filter_map(|g| Some(Gratuit {
        id: g["id"].as_u64()?, titre: g["title"].as_str()?.to_string(), plateformes: g["platforms"].as_str().unwrap_or("").to_string(),
        valeur: g["worth"].as_str().unwrap_or("").to_string(), fin: g["end_date"].as_str().unwrap_or("").to_string(), url: g["open_giveaway_url"].as_str().unwrap_or("").to_string(),
    })).collect()
}

fn verifier(d: &Mutex<DonneesSorties>) {
    let a = agent();
    let attendus = d.lock().unwrap().attendus.clone();
    for x in attendus {
        let url = format!("https://store.steampowered.com/api/appdetails?appids={}&l=french&filters=basic,release_date", x.appid);
        let Some((nom, date, sorti)) = a.get(&url).call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok()).and_then(|j| lire_sortie(x.appid, &j)) else { continue };
        let mut g = d.lock().unwrap();
        if let Some(y) = g.attendus.iter_mut().find(|y| y.appid == x.appid) {
            if sorti && !y.sorti && !y.nom.is_empty() {
                crate::systeme::notifier_toujours("🎉 Sortie !", &format!("{nom} est disponible sur Steam."));
            }
            *y = Attendu { appid: x.appid, nom, date, sorti };
        }
        drop(g);
        std::thread::sleep(Duration::from_millis(1500));
    }
    if let Some(l) = a.get("https://www.gamerpower.com/api/giveaways").query("type", "game").query("platform", "epic-games-store.steam.gog").call().ok()
        .and_then(|r| r.into_json::<serde_json::Value>().ok()).map(|j| lire_gratuits(&j))
    {
        let mut g = d.lock().unwrap();
        let nouveaux: Vec<String> = l.iter().filter(|x| !g.vus.contains(&x.id)).map(|x| x.titre.clone()).collect();
        if !nouveaux.is_empty() && !g.vus.is_empty() {
            crate::systeme::notifier_toujours("🎁 Jeux gratuits", &nouveaux.join(" · "));
        }
        g.vus.extend(l.iter().map(|x| x.id));
        g.gratuits = l;
    }
    let mut g = d.lock().unwrap();
    g.derniere = crate::pressepapiers::maintenant_utc();
    enregistrer("sorties", &*g);
}

pub struct Sorties {
    d: Arc<Mutex<DonneesSorties>>,
    ajout: String,
}

impl Sorties {
    pub fn new() -> Self {
        let d: Arc<Mutex<DonneesSorties>> = Arc::new(Mutex::new(charger("sorties")));
        let d2 = d.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(120));
            if crate::pressepapiers::maintenant_utc() - d2.lock().unwrap().derniere > 6 * 3600 {
                verifier(&d2);
            }
            std::thread::sleep(Duration::from_secs(1800));
        });
        Sorties { d, ajout: String::new() }
    }
}

impl Outil for Sorties {
    fn nom(&self) -> &'static str {
        "🎁 Jeux gratuits et sorties"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Jeux gratuits et sorties attendues");
        let x = self.d.lock().unwrap().clone();
        ui.horizontal(|ui| {
            if ui.button("⟳ Vérifier maintenant").clicked() {
                let d = self.d.clone();
                std::thread::spawn(move || verifier(&d));
            }
            ui.weak("Vérification automatique toutes les 6 h, notification pour chaque nouveauté.");
        });
        ui.separator();
        ui.strong("🎁 Gratuits en ce moment (Epic, Steam, GOG)");
        if x.gratuits.is_empty() {
            ui.weak("Rien pour l'instant (ou pas encore vérifié).");
        }
        for g in &x.gratuits {
            ui.horizontal_wrapped(|ui| {
                if ui.link(RichText::new(&g.titre).strong()).clicked() {
                    let _ = std::process::Command::new("explorer").arg(&g.url).spawn();
                }
                ui.weak(&g.plateformes);
                if !g.valeur.is_empty() && g.valeur != "N/A" {
                    ui.colored_label(Color32::LIGHT_GREEN, format!("au lieu de {}", g.valeur));
                }
                if !g.fin.is_empty() && g.fin != "N/A" {
                    ui.weak(format!("jusqu'au {}", g.fin));
                }
            });
        }
        ui.separator();
        ui.strong("📅 Sorties attendues (Steam)");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.ajout).hint_text("lien Steam ou numéro du jeu").desired_width(300.0));
            if ui.button("➕").clicked() {
                if let Some(id) = crate::promos::appid_de(&self.ajout) {
                    let mut d = self.d.lock().unwrap();
                    if !d.attendus.iter().any(|a| a.appid == id) {
                        d.attendus.push(Attendu { appid: id, ..Default::default() });
                        enregistrer("sorties", &*d);
                    }
                    self.ajout.clear();
                    let d2 = self.d.clone();
                    std::thread::spawn(move || verifier(&d2));
                }
            }
        });
        let mut retirer = None;
        for a in &x.attendus {
            ui.horizontal(|ui| {
                ui.label(RichText::new(if a.nom.is_empty() { format!("jeu {}", a.appid) } else { a.nom.clone() }).strong());
                if a.sorti { ui.colored_label(Color32::LIGHT_GREEN, format!("sorti ({})", a.date)) } else { ui.weak(if a.date.is_empty() { "date inconnue".to_string() } else { a.date.clone() }) };
                if ui.small_button("🗑").clicked() {
                    retirer = Some(a.appid);
                }
            });
        }
        if let Some(id) = retirer {
            let mut d = self.d.lock().unwrap();
            d.attendus.retain(|a| a.appid != id);
            enregistrer("sorties", &*d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lectures() {
        let x = r#"<achievements><achievement closed="1"><apiname><![CDATA[ACH_A]]></apiname><name><![CDATA[Premier pas]]></name><description><![CDATA[Finir le tuto]]></description></achievement>
            <achievement closed="0"><apiname>ACH_B</apiname><name>Boss</name><description>Battre le boss</description></achievement></achievements>"#;
        let s = lire_succes_xml(x);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].id.as_str(), s[0].nom.as_str(), s[0].obtenu), ("ACH_A", "Premier pas", true));
        assert!(!s[1].obtenu);
        let vdf = r#""apps" { "730" { "LastPlayed" "1" "Playtime" "1234" } "570" { "Playtime" "0" } }"#;
        let t = temps_de_jeu(vdf);
        assert_eq!(t.get(&730), Some(&1234));
        assert_eq!(t.get(&570), Some(&0));
        let j: serde_json::Value = serde_json::from_str(r#"{"10":{"success":true,"data":{"name":"Jeu","release_date":{"coming_soon":false,"date":"1 nov. 2026"}}}}"#).unwrap();
        assert_eq!(lire_sortie(10, &j), Some(("Jeu".into(), "1 nov. 2026".into(), true)));
        let g: serde_json::Value = serde_json::from_str(r#"[{"id":5,"title":"X","platforms":"Epic Games Store","worth":"$19.99","end_date":"N/A","open_giveaway_url":"u"}]"#).unwrap();
        assert_eq!(lire_gratuits(&g)[0].titre, "X");
    }
}
