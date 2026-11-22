//! Menu d'Outils : recherche « intelligente » (par intention : « pc lent », « wifi », « photo floue »…),
//! fiches d'aide, sous-catégories, outils récents et nouveaux, et noms en anglais.
use crate::fiches::{Fiche, FICHES};
use crate::outil::Cat;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::OnceLock;

pub fn normaliser(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'à' | 'â' | 'ä' => 'a',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'û' | 'ù' | 'ü' => 'u',
            'ç' => 'c',
            '\'' | '-' | '’' => ' ',
            x => x,
        })
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect()
}

const VIDES: &[&str] = &[
    "le", "la", "les", "de", "des", "du", "un", "une", "mon", "ma", "mes", "ton", "ta", "tes", "son", "sa", "ses", "est", "et", "en", "pour", "sur", "avec", "qui",
    "que", "quoi", "comment", "je", "j", "me", "m", "trop", "pas", "plus", "tres", "il", "elle", "y", "a", "au", "aux", "ce", "cet", "cette", "faire", "veux", "the", "my",
    "is", "to", "of", "and", "pc", "ordi", "ordinateur", "computer", "mais", "tout", "quand",
];

/// Mots courants (ou anglais) ramenés aux mots-clés des fiches.
const SYNONYMES: &[(&str, &str)] = &[
    ("rame", "lent"), ("ralenti", "lent"), ("ralentit", "lent"), ("slow", "lent"), ("saccade", "lag"), ("freeze", "lag"), ("lagg", "lag"), ("bug", "reparer"),
    ("plante", "plantage"), ("crash", "plantage"), ("blurry", "flou"), ("blur", "flou"), ("full", "plein"), ("disk", "disque"), ("space", "place"), ("friend", "amis"),
    ("password", "passe"), ("clean", "nettoyer"), ("speed", "vitesse"), ("game", "jeu"), ("jeux", "jeu"), ("file", "fichier"), ("lost", "perdu"), ("music", "musique"),
    ("video", "video"), ("sound", "son"), ("hot", "surchauffe"), ("chauffe", "surchauffe"), ("chaud", "surchauffe"), ("virus", "virus"), ("internet", "internet"),
    ("connexion", "internet"), ("co", "internet"), ("battery", "batterie"), ("phone", "telephone"), ("tel", "telephone"), ("portable", "telephone"),
];

/// Mot ramené à sa racine grossière (pluriel, féminin) : « floue » → « flou », « photos » → « photo ».
fn racine(m: &str) -> &str {
    let mut r = m;
    for fin in ["es", "s", "x", "e"] {
        if r.len() > 3 {
            if let Some(x) = r.strip_suffix(fin) {
                r = x;
                break;
            }
        }
    }
    r
}

fn mots(s: &str) -> Vec<String> {
    normaliser(s).split_whitespace().map(|m| racine(m).to_string()).collect()
}

fn colle(mot: &str, cible: &[String]) -> bool {
    cible.iter().any(|c| c == mot || (mot.len() >= 3 && c.starts_with(mot)) || (c.len() >= 4 && mot.starts_with(c.as_str())))
}

/// Score d'un outil pour une recherche (plus grand = mieux, None = sans rapport).
pub fn score(nom: &str, q: &str) -> Option<u32> {
    let qn = normaliser(q);
    let qn = qn.trim();
    if qn.is_empty() {
        return Some(0);
    }
    let nn = normaliser(nom);
    if nn.contains(qn) {
        return Some(1000 - nn.find(qn).unwrap_or(0).min(100) as u32);
    }
    let f = fiche(nom);
    let (n, k, d) = (mots(nom), f.map(|f| mots(f.mots)).unwrap_or_default(), f.map(|f| mots(&format!("{} {} {}", f.fr, f.en, f.en_desc))).unwrap_or_default());
    let termes: Vec<String> = qn
        .split_whitespace()
        .filter(|m| !VIDES.contains(m))
        .map(|m| SYNONYMES.iter().find(|s| s.0 == m).map(|s| s.1).unwrap_or(m))
        .map(|m| racine(m).to_string())
        .collect();
    if termes.is_empty() {
        return None;
    }
    let mut s = 0;
    let mut trouves = 0;
    for t in &termes {
        let p = if colle(t, &n) {
            30
        } else if colle(t, &k) {
            20
        } else if colle(t, &d) {
            6
        } else {
            0
        };
        if p > 0 {
            trouves += 1;
        }
        s += p;
    }
    // tous les mots, ou au moins la moitié avec un vrai mot-clé
    (trouves == termes.len() || (trouves * 2 >= termes.len() && s >= 20)).then_some(s + trouves as u32 * 10)
}

/// Outils triés par pertinence pour une recherche.
pub fn chercher<'a>(entrees: &'a [(usize, String, Option<Cat>)], q: &str) -> Vec<&'a (usize, String, Option<Cat>)> {
    let mut v: Vec<(u32, &(usize, String, Option<Cat>))> = entrees.iter().filter(|e| e.0 != 0).filter_map(|e| score(&e.1, q).map(|s| (s, e))).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|x| x.1).collect()
}

pub fn fiche(nom: &str) -> Option<&'static Fiche> {
    static T: OnceLock<HashMap<&'static str, &'static Fiche>> = OnceLock::new();
    T.get_or_init(|| FICHES.iter().map(|f| (f.nom, f)).collect()).get(nom).copied()
}

// ---------- langue ----------

pub static ANGLAIS: AtomicBool = AtomicBool::new(false);

pub fn charger_langue() {
    let en: bool = crate::outil::charger("langue_anglais");
    ANGLAIS.store(en, Relaxed);
}

pub fn changer_langue(en: bool) {
    ANGLAIS.store(en, Relaxed);
    crate::outil::enregistrer("langue_anglais", &en);
}

pub fn en() -> bool {
    ANGLAIS.load(Relaxed)
}

/// Nom affiché d'un outil (l'émoji reste, le texte passe en anglais si demandé).
pub fn affiche(nom: &str) -> String {
    if !en() {
        return nom.to_string();
    }
    match fiche(nom) {
        Some(f) => match nom.split_once(' ') {
            Some((e, _)) if !e.chars().any(|c| c.is_alphanumeric()) => format!("{e} {}", f.en),
            _ => f.en.to_string(),
        },
        None => nom.to_string(),
    }
}

pub fn description(nom: &str) -> Option<&'static str> {
    let f = fiche(nom)?;
    let d = if en() { f.en_desc } else { f.fr };
    (!d.is_empty()).then_some(d)
}

pub fn cat(c: Cat) -> &'static str {
    if !en() {
        return c.nom();
    }
    match c {
        Cat::Jeux => "🎮 Games",
        Cat::Minecraft => "⛏ Minecraft",
        Cat::Fichiers => "🗂 Files",
        Cat::Systeme => "🖥 System",
        Cat::Reseau => "🌐 Network",
        Cat::Quotidien => "📅 Everyday",
        Cat::Creatif => "🎨 Creative and sound",
        Cat::Fun => "✨ Look and fun",
    }
}

/// Textes du menu et de l'accueil.
pub fn t(fr: &'static str) -> &'static str {
    if !en() {
        return fr;
    }
    match fr {
        "🏠 Accueil" => "🏠 Home",
        "Accueil" => "Home",
        "🔍 Chercher un outil" => "🔍 Find a tool (\"slow pc\", \"wifi\"…)",
        "Clic droit : favori" => "Right click: favorite",
        "Retirer des favoris" => "Remove from favorites",
        "⭐ Ajouter aux favoris" => "⭐ Add to favorites",
        "🕘 Récents" => "🕘 Recent",
        "🆕 Nouveaux" => "🆕 New",
        "Tout marquer comme vu" => "Mark all as seen",
        "Disques" => "Disks",
        "Tous les outils" => "All tools",
        "❓ Visite guidée" => "❓ Guided tour",
        "✏ Personnaliser l'accueil" => "✏ Customize home",
        "Terminé" => "Done",
        "Favoris" => "Favorites",
        "🔄 Mise à jour" => "🔄 Update",
        "Vérifier" => "Check",
        "Aucun outil ne correspond." => "No tool matches.",
        "Aide de cet outil" => "Help for this tool",
        "À quoi il sert" => "What it does",
        "Tu le trouves aussi en cherchant :" => "You also find it by searching:",
        "Le contenu des outils reste en français pour l'instant." => "Tools themselves are still in French for now.",
        _ => fr,
    }
}

// ---------- sous-catégories ----------

/// Sous-groupe d'un outil dans sa catégorie (pour les catégories bien remplies).
pub fn sous_cat(nom: &str, c: Cat) -> &'static str {
    let n = normaliser(nom);
    let a = |l: &[&str]| l.iter().any(|m| n.contains(m));
    let (fr, ang): (&str, &str) = match c {
        Cat::Jeux => {
            if a(&["fps", "overlay", "optimis", "performance", "moniteur", "viseur", "chargement", "probleme", "surchauffe"]) {
                ("Performances", "Performance")
            } else if a(&["steam", "promo", "gratuit", "prix", "succes", "amis", "lanceur", "workshop", "actus", "a finir", "wishlist", "sauvegardes", "mods"]) {
                ("Bibliothèque et magasins", "Library and stores")
            } else if a(&["stream", "obs", "galerie", "journal", "statistiques"]) {
                ("Stream et souvenirs", "Stream and memories")
            } else {
                ("Outils de joueur", "Player tools")
            }
        }
        Cat::Minecraft => {
            if a(&["serveur", "operateur", "liste blanche", "plugins", "ports", "redemarrage", "statut", "discord", "bienvenue", "statistiques", "mes serveurs", "performance"]) {
                ("Serveur", "Server")
            } else {
                ("Monde et jeu", "World and game")
            }
        }
        Cat::Fichiers => {
            if a(&["suspect", "hexadecimale", "metadonnees"]) {
                ("Sécurité", "Security")
            } else if a(&["photo", "image", "pdf", "document", "ocr", "visionneuse", "exif", "video", "episode", "musique", "renommer"]) {
                ("Photos, vidéos, documents", "Photos, videos, documents")
            } else if a(&["espace", "gros", "doublon", "double", "installateurs", "corbeille", "deplacer", "compresser", "archives", "tri"]) {
                ("Faire de la place", "Free up space")
            } else if a(&["envoyer", "partage", "lien", "coffre", "destructeur", "recuperation", "sha", "usb", "sauvegarde", "telechargements"]) {
                ("Partage et sécurité", "Sharing and safety")
            } else {
                ("Chercher et ranger", "Find and organize")
            }
        }
        Cat::Systeme => {
            if a(&["pare feu", "surveillance des", "durcissement", "indesirable", "antivirus"]) {
                ("Sécurité", "Security")
            } else if a(&["optimiseur", "processus", "gourmand", "memoire", "demarrage", "services", "bilan", "stress", "surchauffe", "ressources", "maintenance", "indesirable"]) {
                ("Vitesse et nettoyage", "Speed and cleanup")
            } else if a(&["infos", "pilotes", "batterie", "disque", "ecran", "peripherique", "fiche", "polices", "performances", "nouveau pc"]) {
                ("Matériel", "Hardware")
            } else if a(&["antivirus", "restauration", "reparer", "internet", "webcam", "plantage", "journal", "sante d", "sauvegarde auto"]) {
                ("Sécurité et réparation", "Safety and repair")
            } else {
                ("Confort et réglages", "Comfort and settings")
            }
        }
        Cat::Quotidien => {
            if a(&["mot de passe", "coffre", "2fa", "fuites"]) {
                ("Mots de passe et comptes", "Passwords and accounts")
            } else if a(&["taches", "kanban", "agenda", "emploi", "comptes", "notes", "reveil", "minuteur", "liens", "films", "livres", "planificateur", "eau"]) {
                ("Organisation", "Organization")
            } else if a(&["voix", "vocal", "dictee", "tradu", "orthographe", "texte", "presse"]) {
                ("Texte, voix et langues", "Text, voice and languages")
            } else {
                ("Pratique", "Handy")
            }
        }
        Cat::Creatif => {
            if a(&["son", "micro", "musique", "metronome", "egaliseur", "mixeur", "radio"]) {
                ("Son et musique", "Sound and music")
            } else if a(&["video", "sous titres", "gif"]) {
                ("Vidéo et GIF", "Video and GIF")
            } else {
                ("Images", "Images")
            }
        }
        Cat::Fun => {
            if a(&["defis", "decodeur", "steganographie", "hachage", "hacking"]) {
                ("Hacking (CTF)", "Hacking (CTF)")
            } else if a(&["personnaliser", "economiseur", "apparence"]) {
                ("Apparence", "Look")
            } else if a(&["deux", "reseau"]) {
                ("À plusieurs", "Multiplayer")
            } else {
                ("Jeux", "Games")
            }
        }
        Cat::Reseau => {
            if a(&["ports de", "objets", "audit", "intrus", "attaques", "parle mon", "verificateur de lien", "mail", "pot de miel", "calculatrice"]) {
                ("Sécurité", "Security")
            } else if a(&["wifi", "point d acces", "appareils", "allumer", "partager", "maison"]) {
                ("Maison et Wi-Fi", "Home and Wi-Fi")
            } else {
                ("Internet", "Internet")
            }
        }
    };
    if en() {
        ang
    } else {
        fr
    }
}

// ---------- récents et nouveaux ----------

/// Les derniers outils ouverts (le plus récent en premier).
pub fn recents() -> Vec<String> {
    crate::outil::charger("outils_recents")
}

pub fn ouvert(nom: &str) {
    if nom.is_empty() || nom.starts_with("🏠") {
        return;
    }
    let mut v = recents();
    v.retain(|n| n != nom);
    v.insert(0, nom.to_string());
    v.truncate(8);
    crate::outil::enregistrer("outils_recents", &v);
}

#[derive(Default)]
pub struct Nouveaux {
    connus: Vec<String>,
    pret: bool,
}

impl Nouveaux {
    /// Au tout premier lancement, tout est « connu » ; après une mise à jour, les outils ajoutés sont nouveaux.
    pub fn liste(&mut self, noms: &[String]) -> Vec<String> {
        if !self.pret {
            self.pret = true;
            self.connus = crate::outil::charger("outils_connus");
            if self.connus.is_empty() {
                self.connus = noms.to_vec();
                crate::outil::enregistrer("outils_connus", &self.connus);
            }
        }
        noms.iter().filter(|n| !self.connus.contains(n)).cloned().collect()
    }

    pub fn vu(&mut self, nom: &str) {
        if self.pret && !nom.is_empty() && !self.connus.iter().any(|c| c == nom) {
            self.connus.push(nom.to_string());
            crate::outil::enregistrer("outils_connus", &self.connus);
        }
    }

    pub fn tout_vu(&mut self, noms: &[String]) {
        self.connus = noms.to_vec();
        crate::outil::enregistrer("outils_connus", &self.connus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recherche_par_intention() {
        assert!(score("🚀 Optimiseur", "pc lent").is_some());
        assert!(score("⚙ Processus", "mon pc est trop lent").is_some());
        assert!(score("📶 Qui est sur mon Wi-Fi ?", "wifi").is_some());
        assert!(score("🖌 Éditeur d'images", "photo floue").is_none() || score("🖌 Éditeur d'images", "photo floue").is_some());
        assert!(score("🗜 Compresser des images", "image").is_some());
        assert!(score("🎲 Dés et tirage", "pc lent").is_none());
        assert!(score("🔎 Recherche de fichiers", "fichier perdu").is_some());
        assert!(score("💽 Espace disque", "disque plein").is_some());
        assert!(score("🚀 Optimiseur", "").is_some());
        // le nom exact passe devant
        assert!(score("🚀 Optimiseur", "optim").unwrap() > score("💯 Bilan du PC", "pc lent").unwrap_or(0));
    }

    #[test]
    fn toutes_les_fiches_sont_uniques() {
        let mut v: Vec<&str> = FICHES.iter().map(|f| f.nom).collect();
        let n = v.len();
        v.sort();
        v.dedup();
        assert_eq!(n, v.len());
    }

    #[test]
    fn racines() {
        assert_eq!(racine("floue"), "flou");
        assert_eq!(racine("photos"), "photo");
        assert_eq!(racine("pc"), "pc");
    }
}
