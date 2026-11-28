//! Promos Steam : surveille les prix de ta liste de souhaits Steam (profil public) et des jeux ajoutés à la main,
//! prévient quand un jeu passe en promo ou sous le prix voulu. Vérifie toutes les 6 h.
//! Données dans %APPDATA%\Outils\promos.json.
use crate::systeme;
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct Prix {
    pub nom: String,
    pub initial: u64, // centimes
    pub final_: u64,
    pub remise: u32, // %
    pub texte: String,
    pub gratuit: bool,
    pub indispo: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Donnees {
    pub steamid: String,
    pub manuels: Vec<u32>,
    pub souhaits: Vec<u32>,
    pub prix: BTreeMap<u32, Prix>,
    pub cibles: BTreeMap<u32, u64>,    // prix voulu en centimes
    pub prevenus: BTreeMap<u32, u64>,  // dernier prix pour lequel on a prévenu
    pub derniere_maj: u64,             // secondes UTC
    pub pays: String,
    /// plus bas prix jamais vu (centimes), pour repérer les vraies baisses
    pub plus_bas: BTreeMap<u32, u64>,
    /// ne prévenir que pour une remise d'au moins … % (0 = toutes)
    pub remise_min: u32,
}

fn fichier() -> PathBuf {
    crate::outil::base().join("promos.json")
}

fn maintenant() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// SteamID du dernier compte connecté (loginusers.vdf).
pub fn steamid_dans(vdf: &str) -> Option<String> {
    let re = regex::Regex::new(r#""(7656\d{13})"\s*\{([^}]*)\}"#).ok()?;
    let recent = regex::Regex::new(r#""MostRecent"\s*"1""#).ok()?;
    let mut premier = None;
    for c in re.captures_iter(vdf) {
        let bloc = &c[2];
        if recent.is_match(bloc) {
            return Some(c[1].to_string());
        }
        premier.get_or_insert(c[1].to_string());
    }
    premier
}

pub fn steamid_local() -> Option<String> {
    let vdf = std::fs::read_to_string(crate::lanceur::steam_racine()?.join("config").join("loginusers.vdf")).ok()?;
    steamid_dans(&vdf)
}

/// Numéro de jeu depuis une adresse du magasin ou un nombre.
pub fn appid_de(texte: &str) -> Option<u32> {
    let t = texte.trim();
    if let Ok(n) = t.parse() {
        return Some(n);
    }
    let re = regex::Regex::new(r"/app/(\d+)").ok()?;
    re.captures(t)?[1].parse().ok()
}

pub fn lire_souhaits(json: &serde_json::Value) -> Vec<u32> {
    json["response"]["items"].as_array().map(|a| a.iter().filter_map(|x| x["appid"].as_u64().map(|n| n as u32)).collect()).unwrap_or_default()
}

pub fn lire_prix(appid: u32, json: &serde_json::Value) -> Option<Prix> {
    let e = &json[appid.to_string()];
    if e["success"] != true {
        return None;
    }
    let d = &e["data"];
    let nom = d["name"].as_str().unwrap_or("?").to_string();
    let p = &d["price_overview"];
    if p.is_object() {
        Some(Prix {
            nom, initial: p["initial"].as_u64().unwrap_or(0), final_: p["final"].as_u64().unwrap_or(0), remise: p["discount_percent"].as_u64().unwrap_or(0) as u32,
            texte: p["final_formatted"].as_str().unwrap_or("").to_string(), gratuit: false, indispo: false,
        })
    } else {
        let gratuit = d["is_free"] == true;
        Some(Prix { nom, gratuit, indispo: !gratuit, texte: if gratuit { "Gratuit".into() } else { "Pas en vente".into() }, ..Default::default() })
    }
}

/// Faut-il prévenir ? (nouvelle promo, prix plus bas qu'à la dernière alerte, ou sous la cible)
pub fn a_prevenir(p: &Prix, cible: Option<u64>, deja: Option<u64>) -> bool {
    if p.gratuit || p.indispo {
        return false;
    }
    let interessant = p.remise > 0 || cible.is_some_and(|c| p.final_ <= c);
    interessant && deja.is_none_or(|d| p.final_ < d)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("Outils").build()
}

/// Relit la liste de souhaits et tous les prix ; renvoie les alertes à afficher.
fn verifier(d: &Arc<Mutex<Donnees>>, etat: &Arc<Mutex<String>>) -> Vec<String> {
    let a = agent();
    let (steamid, pays) = {
        let x = d.lock().unwrap();
        (x.steamid.clone(), if x.pays.is_empty() { "fr".to_string() } else { x.pays.clone() })
    };
    if !steamid.is_empty() {
        *etat.lock().unwrap() = "Lecture de la liste de souhaits…".into();
        let url = format!("https://api.steampowered.com/IWishlistService/GetWishlist/v1/?steamid={steamid}");
        match a.get(&url).call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok()) {
            Some(j) => d.lock().unwrap().souhaits = lire_souhaits(&j),
            None => *etat.lock().unwrap() = "Liste de souhaits illisible (profil privé ?)".into(),
        }
    }
    let ids: Vec<u32> = {
        let x = d.lock().unwrap();
        let mut v = x.souhaits.clone();
        v.extend(x.manuels.iter().filter(|m| !x.souhaits.contains(m)));
        v
    };
    let mut alertes = vec![];
    for (i, id) in ids.iter().enumerate() {
        *etat.lock().unwrap() = format!("Prix {}/{}…", i + 1, ids.len());
        let url = format!("https://store.steampowered.com/api/appdetails?appids={id}&cc={pays}&l=french&filters=basic,price_overview");
        if let Some(p) = a.get(&url).call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok()).and_then(|j| lire_prix(*id, &j)) {
            let mut x = d.lock().unwrap();
            let (cible, deja) = (x.cibles.get(id).copied(), x.prevenus.get(id).copied());
            let vendu = !p.gratuit && !p.indispo && p.final_ > 0;
            let ancien_bas = x.plus_bas.get(id).copied();
            let record = vendu && ancien_bas.is_some_and(|b| p.final_ < b);
            if vendu {
                let b = x.plus_bas.entry(*id).or_insert(p.final_);
                *b = (*b).min(p.final_);
            }
            let assez = p.remise >= x.remise_min || cible.is_some_and(|c| p.final_ <= c) || record;
            if a_prevenir(&p, cible, deja) && assez {
                alertes.push(format!("{} : {} (-{} %){}", p.nom, p.texte, p.remise, if record { " · 📉 plus bas prix jamais vu" } else { "" }));
                x.prevenus.insert(*id, p.final_);
            } else if record && deja.is_none_or(|d| p.final_ < d) {
                alertes.push(format!("{} : baisse de prix, {} (📉 plus bas jamais vu)", p.nom, p.texte));
                x.prevenus.insert(*id, p.final_);
            } else if p.remise == 0 && cible.is_none_or(|c| p.final_ > c) {
                x.prevenus.remove(id); // fin de promo : on repréviendra à la prochaine
            }
            x.prix.insert(*id, p);
        }
        std::thread::sleep(Duration::from_millis(1600)); // Steam limite ~200 requêtes / 5 min
    }
    let mut x = d.lock().unwrap();
    x.prix.retain(|k, _| ids.contains(k));
    x.derniere_maj = maintenant();
    *etat.lock().unwrap() = format!("{} jeu(x) suivis, {} en promo.", ids.len(), x.prix.values().filter(|p| p.remise > 0).count());
    alertes
}

fn enregistrer(d: &Donnees) {
    let f = fichier();
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(f, serde_json::to_string_pretty(d).unwrap_or_default());
}

pub struct Promos {
    d: Arc<Mutex<Donnees>>,
    etat: Arc<Mutex<String>>,
    occupe: Arc<Mutex<bool>>,
    ajout: String,
    seulement_promos: bool,
    saisie_cible: BTreeMap<u32, String>,
}

fn lancer(d: Arc<Mutex<Donnees>>, etat: Arc<Mutex<String>>, occupe: Arc<Mutex<bool>>) {
    {
        let mut o = occupe.lock().unwrap();
        if *o {
            return;
        }
        *o = true;
    }
    std::thread::spawn(move || {
        let alertes = verifier(&d, &etat);
        enregistrer(&d.lock().unwrap());
        *occupe.lock().unwrap() = false;
        match alertes.len() {
            0 => {}
            1 => systeme::notifier_toujours("Promo Steam", &alertes[0]),
            n => systeme::notifier_toujours(&format!("{n} promos Steam"), &alertes.join(" · ")),
        }
    });
}

impl Promos {
    pub fn new() -> Self {
        let mut d: Donnees = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if d.steamid.is_empty() {
            d.steamid = steamid_local().unwrap_or_default();
        }
        let d = Arc::new(Mutex::new(d));
        let etat = Arc::new(Mutex::new(String::new()));
        let occupe = Arc::new(Mutex::new(false));
        let (d2, e2, o2) = (d.clone(), etat.clone(), occupe.clone());
        std::thread::spawn(move || loop {
            let (vide, derniere) = {
                let x = d2.lock().unwrap();
                (x.steamid.is_empty() && x.manuels.is_empty(), x.derniere_maj)
            };
            if !vide && maintenant().saturating_sub(derniere) >= 6 * 3600 {
                lancer(d2.clone(), e2.clone(), o2.clone());
            }
            std::thread::sleep(Duration::from_secs(600));
        });
        Promos { d, etat, occupe, ajout: String::new(), seulement_promos: false, saisie_cible: BTreeMap::new() }
    }

    pub fn resume(&self) -> String {
        let x = self.d.lock().unwrap();
        let n = x.prix.values().filter(|p| p.remise > 0).count();
        format!("{} jeu(x) suivis · {n} en promo", x.prix.len())
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Promos Steam");
        let avant = self.d.lock().unwrap().clone();
        let mut d = avant.clone();
        let occupe = *self.occupe.lock().unwrap();
        ui.horizontal(|ui| {
            ui.label("SteamID");
            ui.add(egui::TextEdit::singleline(&mut d.steamid).hint_text("7656…").desired_width(170.0));
            if ui.small_button("Détecter").clicked() {
                if let Some(s) = steamid_local() {
                    d.steamid = s;
                }
            }
            ui.label("Pays");
            ui.add(egui::TextEdit::singleline(&mut d.pays).hint_text("fr").desired_width(30.0));
            if occupe {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            } else if ui.button("⟳ Vérifier maintenant").clicked() {
                lancer(self.d.clone(), self.etat.clone(), self.occupe.clone());
            }
        });
        ui.weak("La liste de souhaits doit être visible publiquement (Steam → Profil → Confidentialité → Détails du jeu « Public »). Vérification auto toutes les 6 h.");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.ajout).hint_text("lien du magasin ou numéro du jeu").desired_width(300.0));
            if ui.button("➕ Suivre").clicked() {
                if let Some(id) = appid_de(&self.ajout) {
                    if !d.manuels.contains(&id) {
                        d.manuels.push(id);
                    }
                    self.ajout.clear();
                }
            }
            ui.checkbox(&mut self.seulement_promos, "En promo seulement");
        });
        ui.horizontal(|ui| {
            ui.label("Me prévenir à partir de");
            ui.add(egui::Slider::new(&mut d.remise_min, 0..=90).suffix(" % de remise"));
            ui.weak("(et toujours pour un prix plus bas que jamais, ou sous ton prix voulu). Les alertes arrivent aussi sur le téléphone associé.");
        });
        let e = self.etat.lock().unwrap().clone();
        if !e.is_empty() {
            ui.label(e);
        }
        ui.separator();
        let mut liste: Vec<(u32, Prix)> = d.prix.iter().map(|(k, v)| (*k, v.clone())).collect();
        liste.sort_by(|a, b| b.1.remise.cmp(&a.1.remise).then(a.1.nom.cmp(&b.1.nom)));
        let mut retirer = None;
        egui::ScrollArea::vertical().id_salt("promos").show(ui, |ui| {
            egui::Grid::new("grille_promos").num_columns(6).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
                ui.strong("Jeu");
                ui.strong("Prix");
                ui.strong("Remise");
                ui.strong("Plus bas vu");
                ui.strong("Me prévenir sous");
                ui.label("");
                ui.end_row();
                for (id, p) in liste.iter().filter(|(_, p)| !self.seulement_promos || p.remise > 0) {
                    if ui.link(&p.nom).on_hover_text("Ouvrir dans Steam").clicked() {
                        let _ = std::process::Command::new("explorer").arg(format!("steam://store/{id}")).spawn();
                    }
                    if p.remise > 0 {
                        ui.horizontal(|ui| {
                            ui.strong(&p.texte);
                            ui.weak(egui::RichText::new(format!("{:.2} €", p.initial as f64 / 100.0)).strikethrough());
                        });
                        ui.colored_label(Color32::from_rgb(120, 220, 120), format!("-{} %", p.remise));
                    } else {
                        ui.label(&p.texte);
                        ui.label("");
                    }
                    match d.plus_bas.get(id) {
                        Some(b) if *b >= p.final_ && p.final_ > 0 && !p.gratuit => {
                            ui.colored_label(Color32::from_rgb(120, 220, 120), format!("{:.2} € 📉", *b as f64 / 100.0)).on_hover_text("C'est le plus bas prix vu par Outils");
                        }
                        Some(b) => {
                            ui.weak(format!("{:.2} €", *b as f64 / 100.0));
                        }
                        None => {
                            ui.label("");
                        }
                    }
                    let s = self.saisie_cible.entry(*id).or_insert_with(|| d.cibles.get(id).map(|c| format!("{:.2}", *c as f64 / 100.0)).unwrap_or_default());
                    if ui.add(egui::TextEdit::singleline(s).hint_text("€").desired_width(60.0)).changed() {
                        match s.replace(',', ".").trim().parse::<f64>() {
                            Ok(v) if v > 0.0 => {
                                d.cibles.insert(*id, (v * 100.0).round() as u64);
                            }
                            _ => {
                                d.cibles.remove(id);
                            }
                        }
                    }
                    if d.manuels.contains(id) {
                        if ui.small_button("✖").on_hover_text("Ne plus suivre").clicked() {
                            retirer = Some(*id);
                        }
                    } else {
                        ui.weak("souhait");
                    }
                    ui.end_row();
                }
            });
            if liste.is_empty() {
                ui.weak("Aucun jeu pour l'instant : renseigne ton SteamID ou ajoute un lien, puis « Vérifier maintenant ».");
            }
        });
        if let Some(id) = retirer {
            d.manuels.retain(|m| *m != id);
            d.prix.remove(&id);
        }
        if d != avant {
            let mut x = self.d.lock().unwrap();
            // le fil a pu écrire entre-temps : on ne reprend que ce que l'interface modifie
            x.steamid = d.steamid;
            x.pays = d.pays;
            x.manuels = d.manuels;
            x.cibles = d.cibles;
            x.remise_min = d.remise_min;
            if retirer.is_some() {
                x.prix = d.prix;
            }
            enregistrer(&x);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steamid() {
        let vdf = r#""users" { "76561198000000001" { "AccountName" "a" "MostRecent" "0" } "76561198000000002" { "AccountName" "b" "MostRecent" "1" } }"#;
        assert_eq!(steamid_dans(vdf).as_deref(), Some("76561198000000002"));
    }

    #[test]
    fn appids() {
        assert_eq!(appid_de("https://store.steampowered.com/app/1245620/ELDEN_RING/"), Some(1245620));
        assert_eq!(appid_de(" 730 "), Some(730));
        assert_eq!(appid_de("rien"), None);
    }

    #[test]
    fn prix_et_alertes() {
        let j: serde_json::Value = serde_json::from_str(r#"{"1245620":{"success":true,"data":{"name":"ELDEN RING","price_overview":{"initial":5999,"final":2999,"discount_percent":50,"final_formatted":"29,99€"}}}}"#).unwrap();
        let p = lire_prix(1245620, &j).unwrap();
        assert_eq!((p.nom.as_str(), p.final_, p.remise), ("ELDEN RING", 2999, 50));
        assert!(a_prevenir(&p, None, None));
        assert!(!a_prevenir(&p, None, Some(2999))); // déjà prévenu à ce prix
        assert!(a_prevenir(&p, None, Some(3999)));  // encore moins cher
        let plein = Prix { final_: 2000, ..Default::default() };
        assert!(a_prevenir(&plein, Some(2500), None)); // sous la cible
        assert!(!a_prevenir(&plein, Some(1500), None));
        let souhaits: serde_json::Value = serde_json::from_str(r#"{"response":{"items":[{"appid":10},{"appid":20}]}}"#).unwrap();
        assert_eq!(lire_souhaits(&souhaits), vec![10, 20]);
    }
}
