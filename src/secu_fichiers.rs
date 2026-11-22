//! Fichiers, liens et e-mails suspects (défense) : vrai type d'un fichier, macros, signature, lien
//! piégé (faux site, redirections, âge du domaine), en-têtes d'e-mail usurpés, visionneuse
//! hexadécimale avec extraction de texte, et métadonnées cachées à nettoyer avant de partager.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn hex_sha256(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

// ===================================================================================================
// 11 · Fichier suspect
// ===================================================================================================

/// Vrai type d'un fichier d'après ses premiers octets.
pub fn vrai_type(d: &[u8]) -> &'static str {
    let c = |s: &[u8]| d.starts_with(s);
    if c(b"MZ") {
        "programme Windows (exe/dll)"
    } else if c(b"%PDF") {
        "PDF"
    } else if c(b"PK\x03\x04") {
        if contient(d, b"word/") {
            "document Word"
        } else if contient(d, b"xl/") {
            "classeur Excel"
        } else if contient(d, b"ppt/") {
            "présentation PowerPoint"
        } else if contient(d, b"AndroidManifest") {
            "appli Android (apk)"
        } else {
            "archive zip"
        }
    } else if c(&[0xD0, 0xCF, 0x11, 0xE0]) {
        "ancien document Office (doc/xls) ou installeur msi"
    } else if c(&[0xFF, 0xD8, 0xFF]) {
        "image JPEG"
    } else if c(b"\x89PNG") {
        "image PNG"
    } else if c(b"GIF8") {
        "image GIF"
    } else if c(b"RIFF") && d.len() > 12 && &d[8..12] == b"WEBP" {
        "image WebP"
    } else if c(b"Rar!") {
        "archive RAR"
    } else if c(&[0x37, 0x7A, 0xBC, 0xAF]) {
        "archive 7z"
    } else if c(&[0x4C, 0x00, 0x00, 0x00, 0x01, 0x14, 0x02, 0x00]) {
        "raccourci Windows (lnk)"
    } else if c(b"\x7fELF") {
        "programme Linux"
    } else if d.len() > 8 && &d[4..8] == b"ftyp" {
        "vidéo MP4 / MOV"
    } else if c(b"ID3") || c(&[0xFF, 0xFB]) {
        "musique MP3"
    } else if d.iter().take(2000).all(|b| *b == b'\n' || *b == b'\r' || *b == b'\t' || (32..127).contains(b) || *b >= 0x80) {
        "texte"
    } else {
        "inconnu"
    }
}

fn contient(d: &[u8], motif: &[u8]) -> bool {
    memchr::memmem::find(d, motif).is_some()
}

/// Extensions attendues pour un type.
fn extensions_pour(t: &str) -> &'static [&'static str] {
    match t {
        "programme Windows (exe/dll)" => &["exe", "dll", "sys", "scr", "com", "cpl", "ocx", "efi", "msi"],
        "PDF" => &["pdf"],
        "document Word" => &["docx", "docm", "dotx", "dotm"],
        "classeur Excel" => &["xlsx", "xlsm", "xltx", "xltm"],
        "présentation PowerPoint" => &["pptx", "pptm"],
        "appli Android (apk)" => &["apk", "zip"],
        "archive zip" => &["zip", "jar", "odt", "ods", "epub", "xpi", "crx", "nupkg", "mcpack", "mrpack"],
        "ancien document Office (doc/xls) ou installeur msi" => &["doc", "xls", "ppt", "msi", "msg"],
        "image JPEG" => &["jpg", "jpeg", "jfif"],
        "image PNG" => &["png"],
        "image GIF" => &["gif"],
        "image WebP" => &["webp"],
        "archive RAR" => &["rar"],
        "archive 7z" => &["7z"],
        "raccourci Windows (lnk)" => &["lnk"],
        "vidéo MP4 / MOV" => &["mp4", "mov", "m4a", "m4v", "3gp"],
        "musique MP3" => &["mp3"],
        _ => &[],
    }
}

const DANGEREUSES: &[&str] = &["exe", "scr", "com", "bat", "cmd", "vbs", "vbe", "js", "jse", "wsf", "ps1", "hta", "msi", "lnk", "jar", "cpl", "reg", "iso", "img", "pif", "dll"];

#[derive(Clone, Default)]
pub struct Rapport {
    pub type_vrai: String,
    pub taille: u64,
    pub sha256: String,
    pub alertes: Vec<(u8, String)>, // 0 info, 1 attention, 2 danger
    pub origine: String,
}

/// Analyse d'un fichier (sans l'ouvrir ni l'exécuter).
pub fn analyser_fichier(nom: &str, d: &[u8]) -> Rapport {
    let mut r = Rapport { type_vrai: vrai_type(d).to_string(), taille: d.len() as u64, sha256: hex_sha256(d), ..Default::default() };
    let bas = nom.to_lowercase();
    let ext = bas.rsplit('.').next().unwrap_or("").to_string();
    if nom.contains('\u{202E}') {
        r.alertes.push((2, "le nom contient un caractère qui inverse le texte (le vrai nom se lit à l'envers) : piège classique".into()));
    }
    let parties: Vec<&str> = bas.split('.').collect();
    if parties.len() >= 3 {
        let avant = parties[parties.len() - 2];
        if ["pdf", "doc", "docx", "jpg", "png", "mp4", "txt", "xlsx", "mp3"].contains(&avant) && DANGEREUSES.contains(&ext.as_str()) {
            r.alertes.push((2, format!("double extension « .{avant}.{ext} » : ça se fait passer pour un {avant}, mais c'est un {ext}")));
        }
    }
    let attendues = extensions_pour(&r.type_vrai);
    if !attendues.is_empty() && !ext.is_empty() && !attendues.contains(&ext.as_str()) {
        let g = if r.type_vrai.starts_with("programme") { 2 } else { 1 };
        r.alertes.push((g, format!("l'extension .{ext} ne correspond pas au contenu, qui est en réalité : {}", r.type_vrai)));
    }
    if r.type_vrai.starts_with("programme Windows") || DANGEREUSES.contains(&ext.as_str()) {
        r.alertes.push((1, "c'est un fichier qui peut s'exécuter : ne l'ouvre que s'il vient d'un site officiel".into()));
    }
    if contient(d, b"vbaProject.bin") || (d.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) && (contient(d, b"VBA") || contient(d, b"_VBA_PROJECT"))) {
        r.alertes.push((2, "document Office avec des macros : n'active JAMAIS « Activer le contenu » si tu ne sais pas pourquoi".into()));
    }
    if r.type_vrai == "PDF" {
        for (m, t) in [(&b"/JavaScript"[..], "du JavaScript"), (b"/Launch", "une commande qui lance un programme"), (b"/OpenAction", "une action automatique à l'ouverture"), (b"/EmbeddedFile", "un fichier caché à l'intérieur")] {
            if contient(d, m) {
                r.alertes.push((if m == b"/Launch" { 2 } else { 1 }, format!("ce PDF contient {t}")));
            }
        }
    }
    let texte = String::from_utf8_lossy(&d[..d.len().min(2_000_000)]).to_lowercase();
    for (m, t) in [("powershell", "lance PowerShell"), ("-encodedcommand", "contient une commande PowerShell cachée"), ("frombase64string", "décode des données cachées"), ("invoke-webrequest", "télécharge quelque chose"), ("downloadstring", "télécharge quelque chose"), ("wscript.shell", "exécute des commandes"), ("certutil -decode", "décode un fichier caché"), ("mshta", "lance du code HTML/JS")] {
        if texte.contains(m) && (r.type_vrai != "programme Windows (exe/dll)" || m.contains("encoded")) {
            r.alertes.push((2, format!("le contenu {t}")));
        }
    }
    r.alertes.dedup();
    r
}

/// Site d'où vient le fichier (marque « téléchargé depuis internet » de Windows).
fn origine(p: &Path) -> String {
    let flux = format!("{}:Zone.Identifier", p.display());
    let t = std::fs::read_to_string(flux).unwrap_or_default();
    let url = t.lines().find_map(|l| l.strip_prefix("HostUrl=")).or_else(|| t.lines().find_map(|l| l.strip_prefix("ReferrerUrl="))).unwrap_or("");
    if t.is_empty() {
        String::new()
    } else if url.is_empty() {
        "téléchargé depuis internet".into()
    } else {
        format!("téléchargé depuis {url}")
    }
}

fn signature(p: &Path) -> String {
    let s = format!("$s = Get-AuthenticodeSignature -LiteralPath '{}'; $s.Status.ToString() + '|' + $s.SignerCertificate.Subject", p.display().to_string().replace('\'', "''"));
    crate::optimiseur::ps(&s).unwrap_or_default().trim().to_string()
}

pub struct FichierSuspect {
    r: Arc<Mutex<Option<(PathBuf, Rapport, String)>>>,
}

impl FichierSuspect {
    pub fn new() -> Self {
        FichierSuspect { r: Arc::default() }
    }

    fn analyser(&self, p: PathBuf) {
        let r = self.r.clone();
        std::thread::spawn(move || {
            let mut d = vec![];
            if let Ok(f) = std::fs::File::open(&p) {
                let _ = f.take(300 << 20).read_to_end(&mut d);
            }
            let nom = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let mut rap = analyser_fichier(&nom, &d);
            rap.origine = origine(&p);
            let sig = if rap.type_vrai.starts_with("programme Windows") || rap.type_vrai.contains("msi") { signature(&p) } else { String::new() };
            *r.lock().unwrap() = Some((p, rap, sig));
        });
    }
}

impl Outil for FichierSuspect {
    fn nom(&self) -> &'static str {
        "🧪 Fichier suspect ?"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Fichier suspect ?");
        ui.weak("Choisis ou glisse un fichier : Outils regarde ce qu'il est vraiment (sans l'ouvrir), cherche les pièges classiques (fausse extension, macros, commandes cachées), sa signature et d'où il vient.");
        if ui.button("📂 Choisir un fichier…").clicked() {
            if let Some(p) = rfd::FileDialog::new().pick_file() {
                self.analyser(p);
            }
        }
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.analyser(p);
        }
        let Some((p, r, sig)) = self.r.lock().unwrap().clone() else { return };
        ui.separator();
        ui.strong(p.display().to_string());
        ui.label(format!("Contenu réel : {} · {:.1} Mo", r.type_vrai, r.taille as f64 / 1048576.0));
        if !r.origine.is_empty() {
            ui.label(format!("🌐 {}", r.origine));
        }
        if !sig.is_empty() {
            let (etat, qui) = sig.split_once('|').unwrap_or((&sig, ""));
            let qui = qui.split(',').find_map(|x| x.trim().strip_prefix("CN=")).unwrap_or(qui);
            match etat {
                "Valid" => ui.colored_label(Color32::LIGHT_GREEN, format!("✔ Signé par : {qui}")),
                "NotSigned" => ui.colored_label(Color32::from_rgb(255, 170, 80), "⚠ Programme non signé : on ne sait pas qui l'a fait"),
                e => ui.colored_label(Color32::LIGHT_RED, format!("❗ Signature invalide ({e}) : le programme a peut-être été modifié")),
            };
        }
        let pire = r.alertes.iter().map(|a| a.0).max().unwrap_or(0);
        ui.label(RichText::new(match pire {
            2 => "❗ Dangereux : ne l'ouvre pas",
            1 => "⚠ À manier avec prudence",
            _ => "✔ Rien de suspect repéré",
        }).size(18.0).strong().color([Color32::LIGHT_GREEN, Color32::from_rgb(255, 200, 100), Color32::LIGHT_RED][pire as usize]));
        for (g, t) in &r.alertes {
            ui.colored_label([Color32::GRAY, Color32::from_rgb(255, 200, 100), Color32::LIGHT_RED][*g as usize], format!("• {t}"));
        }
        ui.horizontal(|ui| {
            ui.monospace(format!("SHA-256 : {}", r.sha256));
            if ui.small_button("📋").clicked() {
                ui.ctx().copy_text(r.sha256.clone());
            }
        });
        if ui.button("🔎 Voir l'avis de 70 antivirus (VirusTotal)").on_hover_text("Seule l'empreinte part, pas le fichier").clicked() {
            let _ = std::process::Command::new("explorer").arg(format!("https://www.virustotal.com/gui/file/{}", r.sha256)).spawn();
        }
    }
}

// ===================================================================================================
// 12 · Vérificateur de lien
// ===================================================================================================

/// (marque, domaines officiels)
const MARQUES: &[(&str, &[&str])] = &[
    ("paypal", &["paypal.com", "paypal.fr", "paypal.me"]), ("amazon", &["amazon.fr", "amazon.com", "amazon.de", "amazon.co.uk", "amzn.to", "amazon.es", "amazon.it", "amazonaws.com", "media-amazon.com", "primevideo.com"]),
    ("apple", &["apple.com", "icloud.com"]), ("microsoft", &["microsoft.com", "live.com", "outlook.com", "office.com", "microsoftonline.com", "xbox.com", "windows.com", "bing.com", "msn.com", "azure.com"]),
    ("google", &["google.com", "google.fr", "gmail.com", "youtube.com", "goo.gl", "youtu.be", "googleusercontent.com", "googleapis.com", "gstatic.com", "googlevideo.com", "googlemail.com"]), ("netflix", &["netflix.com"]),
    ("steam", &["steampowered.com", "steamcommunity.com", "steamstatic.com", "s.team"]), ("discord", &["discord.com", "discord.gg", "discordapp.com", "discordapp.net", "discord.media"]),
    ("facebook", &["facebook.com", "fb.com", "fb.me", "messenger.com"]), ("instagram", &["instagram.com"]), ("laposte", &["laposte.fr", "laposte.net", "labanquepostale.fr"]),
    ("ameli", &["ameli.fr"]), ("impots", &["impots.gouv.fr"]), ("caf", &["caf.fr"]), ("antai", &["antai.gouv.fr"]), ("creditagricole", &["credit-agricole.fr", "ca-paris.fr"]),
    ("societegenerale", &["societegenerale.fr"]), ("bnpparibas", &["bnpparibas.net", "mabanque.bnpparibas"]), ("orange", &["orange.fr", "orange.com"]),
    ("free", &["free.fr", "freebox.fr", "free-mobile.fr"]), ("sfr", &["sfr.fr"]), ("bouygues", &["bouyguestelecom.fr"]), ("chronopost", &["chronopost.fr"]),
    ("colissimo", &["colissimo.fr", "laposte.fr"]), ("dhl", &["dhl.com", "dhl.fr"]), ("ups", &["ups.com"]), ("epicgames", &["epicgames.com", "unrealengine.com"]),
    ("roblox", &["roblox.com"]), ("twitch", &["twitch.tv"]), ("binance", &["binance.com"]), ("ledger", &["ledger.com"]), ("leboncoin", &["leboncoin.fr"]),
    ("vinted", &["vinted.fr", "vinted.com"]), ("mondialrelay", &["mondialrelay.fr"]), ("ants", &["ants.gouv.fr"]),
];

const RACCOURCISSEURS: &[&str] = &["bit.ly", "tinyurl.com", "t.co", "goo.gl", "ow.ly", "is.gd", "cutt.ly", "rebrand.ly", "shorturl.at", "tiny.cc", "rb.gy", "buff.ly"];

/// Homoglyphes courants ramenés à la lettre imitée (paypa1 → paypal, rn → m).
pub fn sans_deguisement(s: &str) -> String {
    s.to_lowercase().replace("rn", "m").replace("vv", "w").chars().map(|c| match c {
        '0' => 'o',
        '1' | 'l' | '|' => 'l',
        '3' => 'e',
        '5' => 's',
        'а' => 'a',
        'е' => 'e',
        'о' => 'o',
        'р' => 'p',
        'с' => 'c',
        x => x,
    }).filter(|c| c.is_alphanumeric()).collect()
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut p: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut c = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            c[j] = (p[j] + 1).min(c[j - 1] + 1).min(p[j - 1] + (a[i - 1] != b[j - 1]) as usize);
        }
        p = c;
    }
    p[b.len()]
}

/// Problèmes d'une adresse (sans la visiter) : (gravité, texte).
pub fn examiner_lien(url: &str) -> Vec<(u8, String)> {
    let mut v = vec![];
    let u = url.trim();
    let sans = u.trim_start_matches("https://").trim_start_matches("http://");
    let hote_complet = sans.split(['/', '?', '#']).next().unwrap_or("");
    if hote_complet.contains('@') {
        v.push((2, format!("l'adresse contient « @ » : le vrai site est ce qui vient après ({})", hote_complet.rsplit('@').next().unwrap_or(""))));
    }
    let hote = hote_complet.rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("").to_lowercase();
    if u.starts_with("http://") {
        v.push((1, "pas de chiffrement (http) : ne tape jamais de mot de passe ici".into()));
    }
    if hote.parse::<std::net::Ipv4Addr>().is_ok() {
        v.push((2, "une adresse IP au lieu d'un nom de site : les vrais services n'en utilisent pas".into()));
    }
    if hote.contains("xn--") {
        v.push((2, "nom de domaine avec des lettres d'autres alphabets (punycode) : peut imiter un vrai site à l'identique".into()));
    }
    if hote.chars().any(|c| !c.is_ascii()) {
        v.push((2, "lettres non latines dans le nom du site (une « а » cyrillique ressemble à un « a »)".into()));
    }
    let d = crate::secu_mdp::domaine(&hote);
    if hote.split('.').count() >= 5 {
        v.push((1, "beaucoup de sous-domaines : souvent utilisé pour cacher le vrai site à la fin".into()));
    }
    if RACCOURCISSEURS.contains(&d.as_str()) || RACCOURCISSEURS.contains(&hote.as_str()) {
        v.push((1, "lien raccourci : on ne voit pas où il mène (déplie-le avec le bouton)".into()));
    }
    let nom = sans_deguisement(d.split('.').next().unwrap_or(""));
    let complet = sans_deguisement(&hote);
    for (marque, officiels) in MARQUES {
        if officiels.contains(&d.as_str()) || officiels.iter().any(|o| hote.ends_with(&format!(".{o}"))) {
            v.push((0, format!("domaine officiel de {marque}")));
            return v;
        }
        let imite = nom == *marque || (marque.len() >= 5 && distance(&nom, marque) <= 1) || (marque.len() >= 5 && complet.contains(marque));
        if imite {
            v.push((2, format!("se fait passer pour {marque}, mais le vrai site est {} : c'est sans doute de l'hameçonnage", officiels[0])));
            break;
        }
    }
    for t in [".zip", ".mov", ".top", ".xyz", ".click", ".cfd", ".sbs", ".icu", ".rest", ".monster"] {
        if hote.ends_with(t) {
            v.push((1, format!("extension « {t} » très utilisée par les arnaques")));
        }
    }
    v
}

/// Suit les redirections une par une (sans charger les pages).
fn deplier(url: &str) -> Vec<String> {
    let a = ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(10)).user_agent("Mozilla/5.0").build();
    let mut v = vec![url.to_string()];
    let mut u = url.to_string();
    for _ in 0..10 {
        let r = match a.head(&u).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(_) => break,
        };
        match r.header("location") {
            Some(l) => {
                u = if l.starts_with("http") { l.to_string() } else { format!("{}{}", u.split('/').take(3).collect::<Vec<_>>().join("/"), l) };
                v.push(u.clone());
            }
            None => break,
        }
    }
    v
}

/// Date de création du domaine (RDAP).
fn age_domaine(d: &str) -> Option<String> {
    let j: serde_json::Value = ureq::get(&format!("https://rdap.org/domain/{d}")).timeout(Duration::from_secs(10)).call().ok()?.into_json().ok()?;
    j["events"].as_array()?.iter().find(|e| e["eventAction"] == "registration").and_then(|e| e["eventDate"].as_str()).map(|s| s[..10.min(s.len())].to_string())
}

pub fn jours_depuis(date: &str) -> Option<i64> {
    let p: Vec<i64> = date.split('-').filter_map(|x| x.parse().ok()).collect();
    if p.len() < 3 {
        return None;
    }
    let jours = |a: i64, m: i64, j: i64| {
        let (a, m) = if m <= 2 { (a - 1, m + 12) } else { (a, m) };
        365 * a + a / 4 - a / 100 + a / 400 + (153 * (m - 3) + 2) / 5 + j
    };
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64 / 86400;
    Some(t + jours(1970, 1, 1) - jours(p[0], p[1], p[2]))
}

pub struct Lien {
    url: String,
    chaine: Arc<Mutex<Option<(Vec<String>, Option<String>)>>>,
}

impl Lien {
    pub fn new() -> Self {
        Lien { url: String::new(), chaine: Arc::default() }
    }
}

impl Outil for Lien {
    fn nom(&self) -> &'static str {
        "🔗 Vérificateur de lien"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ce lien est-il piégé ?");
        ui.weak("Colle un lien reçu par SMS, e-mail ou Discord : Outils repère les faux sites (« paypa1 », lettres déguisées), les liens raccourcis, l'adresse réelle derrière les redirections et l'âge du site. Tu n'as pas besoin de cliquer.");
        if ui.add(egui::TextEdit::singleline(&mut self.url).hint_text("https://…").desired_width(f32::INFINITY)).changed() {
            *self.chaine.lock().unwrap() = None;
        }
        if self.url.trim().is_empty() {
            return;
        }
        let url = if self.url.contains("://") { self.url.trim().to_string() } else { format!("http://{}", self.url.trim()) };
        let mut v = examiner_lien(&url);
        if ui.button("🔍 Déplier et vérifier l'âge du site").on_hover_text("Outils contacte le site (sans ouvrir la page) pour suivre les redirections").clicked() {
            let (u, c) = (url.clone(), self.chaine.clone());
            std::thread::spawn(move || {
                let ch = deplier(&u);
                let fin = ch.last().cloned().unwrap_or(u);
                let d = crate::secu_mdp::domaine(fin.trim_start_matches("https://").trim_start_matches("http://"));
                *c.lock().unwrap() = Some((ch, age_domaine(&d)));
            });
        }
        if let Some((ch, age)) = self.chaine.lock().unwrap().clone() {
            if ch.len() > 1 {
                ui.strong("Chemin des redirections :");
                for (k, u) in ch.iter().enumerate() {
                    ui.label(format!("{} {u}", if k == 0 { "•" } else { "→" }));
                }
                if let Some(fin) = ch.last() {
                    for x in examiner_lien(fin) {
                        if !v.contains(&x) {
                            v.push(x);
                        }
                    }
                }
            }
            match age.as_deref().and_then(|a| jours_depuis(a).map(|j| (a.to_string(), j))) {
                Some((a, j)) if j < 60 => v.push((2, format!("site créé il y a {j} jours seulement ({a}) : les arnaques utilisent des sites tout neufs"))),
                Some((a, j)) => v.push((0, format!("site créé le {a} (il y a {} ans)", j / 365))),
                None => v.push((0, "âge du site inconnu".into())),
            }
        }
        let pire = v.iter().map(|x| x.0).max().unwrap_or(0);
        ui.label(RichText::new(match pire {
            2 => "❗ Très suspect : ne clique pas, ne tape rien",
            1 => "⚠ Prudence",
            _ => "✔ Rien de suspect repéré (reste méfiant si on te presse ou te demande de payer)",
        }).size(17.0).strong().color([Color32::LIGHT_GREEN, Color32::from_rgb(255, 200, 100), Color32::LIGHT_RED][pire as usize]));
        for (g, t) in &v {
            ui.colored_label([Color32::LIGHT_GREEN, Color32::from_rgb(255, 200, 100), Color32::LIGHT_RED][*g as usize], format!("• {t}"));
        }
    }
}

// ===================================================================================================
// 13 · E-mail suspect (en-têtes)
// ===================================================================================================

/// En-têtes « déroulés » (les lignes qui commencent par un espace continuent la précédente).
pub fn entetes(brut: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = vec![];
    for l in brut.lines() {
        if l.is_empty() {
            break;
        }
        if l.starts_with([' ', '\t']) {
            if let Some(d) = v.last_mut() {
                d.1.push(' ');
                d.1.push_str(l.trim());
            }
        } else if let Some((k, val)) = l.split_once(':') {
            v.push((k.trim().to_lowercase(), val.trim().to_string()));
        }
    }
    v
}

fn adresse(v: &str) -> String {
    let a = v.rsplit('<').next().unwrap_or(v).trim_end_matches('>').trim();
    a.to_lowercase()
}

/// Verdict sur un e-mail d'après ses en-têtes : (gravité, texte).
pub fn examiner_mail(brut: &str) -> Vec<(u8, String)> {
    let h = entetes(brut);
    let get = |k: &str| h.iter().find(|x| x.0 == k).map(|x| x.1.clone());
    let mut v = vec![];
    let from = get("from").unwrap_or_default();
    let de = adresse(&from);
    let dom = de.rsplit('@').next().unwrap_or("").to_string();
    if from.is_empty() {
        v.push((1, "pas d'en-tête « From » trouvé : colle bien les en-têtes complets".into()));
        return v;
    }
    v.push((0, format!("expéditeur affiché : {from}")));
    let auth = h.iter().filter(|x| x.0 == "authentication-results").map(|x| x.1.to_lowercase()).collect::<Vec<_>>().join(" ");
    for (test, nom) in [("spf", "SPF (le serveur a-t-il le droit d'envoyer pour ce domaine ?)"), ("dkim", "DKIM (signature du domaine)"), ("dmarc", "DMARC (politique du domaine)")] {
        if let Some(p) = auth.find(&format!("{test}=")) {
            let r: String = auth[p + test.len() + 1..].chars().take_while(|c| c.is_alphanumeric()).collect();
            match r.as_str() {
                "pass" => v.push((0, format!("✔ {nom} : réussi"))),
                "fail" | "softfail" | "permerror" => v.push((2, format!("✖ {nom} : ÉCHEC ({r}) — l'expéditeur est probablement usurpé"))),
                x => v.push((1, format!("{nom} : {x}"))),
            }
        }
    }
    if auth.is_empty() {
        v.push((1, "pas de résultat d'authentification (SPF/DKIM/DMARC) dans les en-têtes".into()));
    }
    if let Some(rp) = get("return-path") {
        let d2 = adresse(&rp).rsplit('@').next().unwrap_or("").to_string();
        if !d2.is_empty() && crate::secu_mdp::domaine(&d2) != crate::secu_mdp::domaine(&dom) {
            v.push((1, format!("l'adresse de retour ({d2}) n'est pas du même domaine que l'expéditeur ({dom})")));
        }
    }
    if let Some(rt) = get("reply-to") {
        let d3 = adresse(&rt).rsplit('@').next().unwrap_or("").to_string();
        if !d3.is_empty() && crate::secu_mdp::domaine(&d3) != crate::secu_mdp::domaine(&dom) {
            v.push((2, format!("si tu réponds, ta réponse part chez {d3}, pas chez l'expéditeur affiché : classique des arnaques")));
        }
    }
    let nom_affiche = sans_deguisement(from.split('<').next().unwrap_or(""));
    for (marque, officiels) in MARQUES {
        if nom_affiche.contains(marque) && !officiels.iter().any(|o| dom == *o || dom.ends_with(&format!(".{o}"))) {
            v.push((2, format!("le nom affiché parle de {marque}, mais l'adresse vient de {dom}")));
        }
    }
    for l in examiner_lien(&format!("https://{dom}")) {
        if l.0 == 2 {
            v.push((2, format!("domaine de l'expéditeur : {}", l.1)));
        }
    }
    let recus: Vec<&String> = h.iter().filter(|x| x.0 == "received").map(|x| &x.1).collect();
    if let Some(premier) = recus.last() {
        let ip: String = premier.split(['[', ']']).nth(1).unwrap_or("").to_string();
        if !ip.is_empty() {
            v.push((0, format!("premier serveur d'envoi : {ip} ({} relais)", recus.len())));
        }
    }
    v
}

pub struct Mail {
    texte: String,
}

impl Mail {
    pub fn new() -> Self {
        Mail { texte: String::new() }
    }
}

impl Outil for Mail {
    fn nom(&self) -> &'static str {
        "📧 E-mail suspect ?"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Cet e-mail est-il un faux ?");
        ui.weak("Colle les en-têtes de l'e-mail : Outils dit si l'expéditeur est usurpé (SPF, DKIM, DMARC), où partirait ta réponse, et d'où il vient vraiment.");
        ui.weak("Gmail : ⋮ › « Afficher l'original ». Outlook : … › Afficher › « Afficher la source du message ». Thunderbird : Ctrl+U.");
        ui.add(egui::TextEdit::multiline(&mut self.texte).desired_rows(8).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("Received: from …\nFrom: …"));
        if self.texte.trim().is_empty() {
            return;
        }
        let v = examiner_mail(&self.texte);
        let pire = v.iter().map(|x| x.0).max().unwrap_or(0);
        ui.label(RichText::new(match pire {
            2 => "❗ Très probablement une arnaque",
            1 => "⚠ Douteux : vérifie par un autre moyen (appelle, va sur le site toi-même)",
            _ => "✔ L'expéditeur semble authentique",
        }).size(17.0).strong().color([Color32::LIGHT_GREEN, Color32::from_rgb(255, 200, 100), Color32::LIGHT_RED][pire as usize]));
        for (g, t) in &v {
            ui.colored_label([Color32::LIGHT_GREEN, Color32::from_rgb(255, 200, 100), Color32::LIGHT_RED][*g as usize], format!("• {t}"));
        }
    }
}

// ===================================================================================================
// 14 · Visionneuse hexadécimale et chaînes
// ===================================================================================================

/// Morceaux de texte lisible (ASCII et UTF-16) d'au moins `min` caractères : (position, texte).
pub fn chaines(d: &[u8], min: usize) -> Vec<(usize, String)> {
    let mut v = vec![];
    let mut debut = 0;
    let mut cur = String::new();
    for (i, b) in d.iter().enumerate() {
        if (32..127).contains(b) {
            if cur.is_empty() {
                debut = i;
            }
            cur.push(*b as char);
        } else {
            if cur.len() >= min {
                v.push((debut, std::mem::take(&mut cur)));
            }
            cur.clear();
        }
    }
    if cur.len() >= min {
        v.push((debut, cur));
    }
    // UTF-16 (texte des programmes Windows)
    let mut cur = String::new();
    let mut i = 0;
    while i + 1 < d.len() {
        if (32..127).contains(&d[i]) && d[i + 1] == 0 {
            if cur.is_empty() {
                debut = i;
            }
            cur.push(d[i] as char);
        } else {
            if cur.len() >= min {
                v.push((debut, std::mem::take(&mut cur)));
            }
            cur.clear();
        }
        i += 2;
    }
    if cur.len() >= min {
        v.push((debut, cur));
    }
    v.sort_by_key(|x| x.0);
    v
}

pub fn interessant(s: &str) -> bool {
    let b = s.to_lowercase();
    b.contains("http") || b.contains("\\") && b.contains(':') || b.contains(".exe") || b.contains("password") || b.contains("passwd") || b.contains("cmd") || b.contains("powershell") || b.split('.').filter(|x| x.parse::<u8>().is_ok()).count() == 4
}

pub struct Hexa {
    chemin: Option<PathBuf>,
    d: Vec<u8>,
    aller: String,
    cherche: String,
    ligne: Option<usize>,
    onglet: usize,
    filtre: String,
    seulement_utiles: bool,
}

impl Hexa {
    pub fn new() -> Self {
        Hexa { chemin: None, d: vec![], aller: String::new(), cherche: String::new(), ligne: None, onglet: 0, filtre: String::new(), seulement_utiles: false }
    }
}

impl Outil for Hexa {
    fn nom(&self) -> &'static str {
        "🔬 Visionneuse hexadécimale"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Visionneuse hexadécimale et chaînes");
        ui.horizontal(|ui| {
            if ui.button("📂 Ouvrir un fichier…").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    let mut d = vec![];
                    if let Ok(f) = std::fs::File::open(&p) {
                        let _ = f.take(64 << 20).read_to_end(&mut d);
                    }
                    self.d = d;
                    self.chemin = Some(p);
                    self.ligne = None;
                }
            }
            if let Some(p) = &self.chemin {
                ui.label(format!("{} · {} octets{}", p.display(), self.d.len(), if self.d.len() >= 64 << 20 { " (64 premiers Mo)" } else { "" }));
            }
        });
        if self.d.is_empty() {
            ui.weak("Ouvre n'importe quel fichier pour voir ses octets, et le texte caché dedans (adresses, chemins, messages).");
            return;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.onglet, 0, "Octets");
            ui.selectable_value(&mut self.onglet, 1, "Texte trouvé dedans");
            ui.label(format!("Type : {}", vrai_type(&self.d)));
        });
        if self.onglet == 1 {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("filtrer").desired_width(200.0));
                ui.checkbox(&mut self.seulement_utiles, "seulement adresses, chemins, commandes");
            });
            let f = self.filtre.to_lowercase();
            let c: Vec<(usize, String)> = chaines(&self.d, 5).into_iter().filter(|(_, s)| (f.is_empty() || s.to_lowercase().contains(&f)) && (!self.seulement_utiles || interessant(s))).collect();
            ui.label(format!("{} morceau(x) de texte", c.len()));
            egui::ScrollArea::vertical().id_salt("chaines").show_rows(ui, 18.0, c.len(), |ui, r| {
                for (p, s) in &c[r] {
                    let t = format!("{p:08X}  {s}");
                    if interessant(s) {
                        ui.colored_label(Color32::from_rgb(255, 200, 100), RichText::new(t).monospace());
                    } else {
                        ui.monospace(t);
                    }
                }
            });
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Aller à");
            ui.add(egui::TextEdit::singleline(&mut self.aller).hint_text("1A2B (hexa)").desired_width(90.0));
            if ui.button("↵").clicked() {
                if let Ok(o) = usize::from_str_radix(self.aller.trim().trim_start_matches("0x"), 16) {
                    self.ligne = Some(o / 16);
                }
            }
            ui.label("Chercher");
            ui.add(egui::TextEdit::singleline(&mut self.cherche).hint_text("texte ou « hex: 4D 5A »").desired_width(160.0));
            if ui.button("Suivant").clicked() {
                let motif: Vec<u8> = match self.cherche.strip_prefix("hex:") {
                    Some(h) => h.split_whitespace().filter_map(|x| u8::from_str_radix(x, 16).ok()).collect(),
                    None => self.cherche.as_bytes().to_vec(),
                };
                let depart = self.ligne.map(|l| l * 16 + 1).unwrap_or(0).min(self.d.len());
                if !motif.is_empty() {
                    if let Some(p) = memchr::memmem::find(&self.d[depart..], &motif).map(|p| p + depart).or_else(|| memchr::memmem::find(&self.d, &motif)) {
                        self.ligne = Some(p / 16);
                    }
                }
            }
        });
        let lignes = self.d.len().div_ceil(16);
        let mut sa = egui::ScrollArea::vertical().id_salt("hexa");
        if let Some(l) = self.ligne.take() {
            sa = sa.vertical_scroll_offset(l as f32 * 18.0);
        }
        sa.show_rows(ui, 18.0, lignes, |ui, r| {
            for l in r {
                let b = &self.d[l * 16..(l * 16 + 16).min(self.d.len())];
                let hex: String = b.iter().enumerate().map(|(i, x)| if i == 8 { format!(" {x:02X}") } else { format!("{x:02X}") }).collect::<Vec<_>>().join(" ");
                let txt: String = b.iter().map(|x| if (32..127).contains(x) { *x as char } else { '·' }).collect();
                ui.monospace(format!("{:08X}  {hex:<49}  {txt}", l * 16));
            }
        });
    }
}

// ===================================================================================================
// 15 · Métadonnées cachées
// ===================================================================================================

/// Retire les blocs de métadonnées d'un JPEG (EXIF, XMP, commentaires) sans toucher à l'image.
pub fn jpeg_sans_meta(d: &[u8]) -> Option<Vec<u8>> {
    if !d.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut o = vec![0xFF, 0xD8];
    let mut i = 2;
    while i + 4 <= d.len() {
        if d[i] != 0xFF {
            return None;
        }
        let m = d[i + 1];
        if m == 0xDA {
            o.extend_from_slice(&d[i..]); // début de l'image : on recopie tout le reste
            return Some(o);
        }
        let n = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        let fin = (i + 2 + n).min(d.len());
        // APP1 (EXIF/XMP), APP12/13 (Photoshop, IPTC), COM (commentaire) ; on garde APP0 (JFIF), APP2 (couleurs), APP14 (Adobe)
        if !matches!(m, 0xE1 | 0xEC | 0xED | 0xFE | 0xE3..=0xEB | 0xEF) {
            o.extend_from_slice(&d[i..fin]);
        }
        i = fin;
    }
    Some(o)
}

/// Retire les blocs texte et EXIF d'un PNG.
pub fn png_sans_meta(d: &[u8]) -> Option<Vec<u8>> {
    if !d.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut o = d[..8].to_vec();
    let mut i = 8;
    while i + 12 <= d.len() {
        let n = u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
        let t = &d[i + 4..i + 8];
        let fin = (i + 12 + n).min(d.len());
        if !matches!(t, b"tEXt" | b"iTXt" | b"zTXt" | b"eXIf" | b"tIME") {
            o.extend_from_slice(&d[i..fin]);
        }
        i = fin;
    }
    Some(o)
}

fn infos_photo(p: &Path) -> Vec<String> {
    let mut v = vec![];
    let Ok(f) = std::fs::File::open(p) else { return v };
    let Ok(e) = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(f)) else { return v };
    for (t, n) in [(exif::Tag::Make, "Marque"), (exif::Tag::Model, "Appareil"), (exif::Tag::DateTimeOriginal, "Prise le"), (exif::Tag::Software, "Logiciel"), (exif::Tag::Artist, "Auteur"), (exif::Tag::GPSLatitude, "📍 Latitude"), (exif::Tag::GPSLongitude, "📍 Longitude"), (exif::Tag::BodySerialNumber, "N° de série")] {
        if let Some(c) = e.get_field(t, exif::In::PRIMARY) {
            v.push(format!("{n} : {}", c.display_value().with_unit(&e)));
        }
    }
    v
}

/// Office : auteur, dernière modification, société (docProps).
fn infos_office(d: &[u8]) -> Vec<String> {
    let mut v = vec![];
    let Ok(mut z) = zip::ZipArchive::new(std::io::Cursor::new(d)) else { return v };
    for (f, balises) in [("docProps/core.xml", &["dc:creator", "cp:lastModifiedBy", "dc:title", "dcterms:created", "dcterms:modified"][..]), ("docProps/app.xml", &["Company", "Manager", "Application"][..])] {
        let Ok(mut e) = z.by_name(f) else { continue };
        let mut t = String::new();
        let _ = e.read_to_string(&mut t);
        for b in balises {
            if let Some(i) = t.find(&format!("<{b}")) {
                let deb = t[i..].find('>').map(|k| i + k + 1).unwrap_or(i);
                let fin = t[deb..].find('<').map(|k| deb + k).unwrap_or(deb);
                let val = &t[deb..fin];
                if !val.trim().is_empty() {
                    v.push(format!("{b} : {val}"));
                }
            }
        }
    }
    v
}

/// Copie du document Office sans auteur ni société.
fn office_sans_meta(d: &[u8]) -> Option<Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(d)).ok()?;
    let mut sortie = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..z.len() {
        let mut e = z.by_index(i).ok()?;
        let nom = e.name().to_string();
        let mut b = vec![];
        e.read_to_end(&mut b).ok()?;
        if nom == "docProps/core.xml" || nom == "docProps/app.xml" {
            let mut t = String::from_utf8_lossy(&b).to_string();
            for bal in ["dc:creator", "cp:lastModifiedBy", "Company", "Manager"] {
                if let Some(i) = t.find(&format!("<{bal}")) {
                    if let (Some(a), Some(f)) = (t[i..].find('>').map(|k| i + k + 1), t.find(&format!("</{bal}>"))) {
                        if a <= f {
                            t.replace_range(a..f, "");
                        }
                    }
                }
            }
            b = t.into_bytes();
        }
        sortie.start_file(nom, opt).ok()?;
        std::io::Write::write_all(&mut sortie, &b).ok()?;
    }
    Some(sortie.finish().ok()?.into_inner())
}

fn infos_pdf(p: &Path) -> (Vec<String>, Option<lopdf::Document>) {
    let Ok(doc) = lopdf::Document::load(p) else { return (vec![], None) };
    let mut v = vec![];
    if let Ok(info) = doc.trailer.get(b"Info").and_then(|o| o.as_reference()).and_then(|r| doc.get_dictionary(r)) {
        for (k, val) in info.iter() {
            let t = match val {
                lopdf::Object::String(s, _) => String::from_utf8_lossy(s).trim_start_matches('\u{feff}').to_string(),
                o => format!("{o:?}"),
            };
            v.push(format!("{} : {t}", String::from_utf8_lossy(k)));
        }
    }
    (v, Some(doc))
}

pub struct Metadonnees {
    chemin: Option<PathBuf>,
    infos: Vec<String>,
    message: String,
}

impl Metadonnees {
    pub fn new() -> Self {
        Metadonnees { chemin: None, infos: vec![], message: String::new() }
    }

    fn lire(&mut self, p: PathBuf) {
        let d = std::fs::read(&p).unwrap_or_default();
        self.infos = match vrai_type(&d) {
            "image JPEG" | "image PNG" => infos_photo(&p),
            "PDF" => infos_pdf(&p).0,
            "document Word" | "classeur Excel" | "présentation PowerPoint" => infos_office(&d),
            _ => vec![],
        };
        if vrai_type(&d) == "image PNG" && (contient(&d, b"tEXt") || contient(&d, b"iTXt")) {
            for (_, s) in chaines(&d[..d.len().min(200_000)], 6).into_iter().take(6) {
                self.infos.push(format!("texte caché : {s}"));
            }
        }
        self.chemin = Some(p);
        self.message.clear();
    }

    fn nettoyer(&mut self) {
        let Some(p) = self.chemin.clone() else { return };
        let d = std::fs::read(&p).unwrap_or_default();
        let t = vrai_type(&d);
        let ext = p.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        let dest = p.with_file_name(format!("{} (nettoyé).{ext}", p.file_stem().unwrap_or_default().to_string_lossy()));
        let r: Result<(), String> = match t {
            "image JPEG" => jpeg_sans_meta(&d).ok_or("JPEG illisible".to_string()).and_then(|o| std::fs::write(&dest, o).map_err(|e| e.to_string())),
            "image PNG" => png_sans_meta(&d).ok_or("PNG illisible".to_string()).and_then(|o| std::fs::write(&dest, o).map_err(|e| e.to_string())),
            "document Word" | "classeur Excel" | "présentation PowerPoint" => office_sans_meta(&d).ok_or("document illisible".to_string()).and_then(|o| std::fs::write(&dest, o).map_err(|e| e.to_string())),
            "PDF" => match infos_pdf(&p).1 {
                Some(mut doc) => {
                    doc.trailer.remove(b"Info");
                    doc.save(&dest).map(drop).map_err(|e| e.to_string())
                }
                None => Err("PDF illisible".into()),
            },
            _ => Err("type de fichier non pris en charge (photos JPEG/PNG, PDF, Word, Excel, PowerPoint)".into()),
        };
        self.message = match r {
            Ok(()) => format!("✔ Copie nettoyée : {}", dest.display()),
            Err(e) => format!("✖ {e}"),
        };
    }
}

impl Outil for Metadonnees {
    fn nom(&self) -> &'static str {
        "🧹 Métadonnées cachées"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Métadonnées cachées");
        ui.weak("Une photo peut dire où elle a été prise (GPS) et avec quel téléphone ; un document Word ou PDF peut contenir ton nom complet et celui de ton école ou entreprise. Vérifie et nettoie avant de partager.");
        if ui.button("📂 Choisir un fichier…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Photos et documents", &["jpg", "jpeg", "png", "pdf", "docx", "xlsx", "pptx"]).pick_file() {
                self.lire(p);
            }
        }
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.lire(p);
        }
        let Some(p) = self.chemin.clone() else { return };
        ui.strong(p.display().to_string());
        if self.infos.is_empty() {
            ui.colored_label(Color32::LIGHT_GREEN, "✔ Aucune information personnelle trouvée.");
        } else {
            for i in &self.infos {
                ui.colored_label(if i.contains('📍') || i.to_lowercase().contains("creator") || i.contains("Author") { Color32::from_rgb(255, 170, 80) } else { ui.visuals().text_color() }, format!("• {i}"));
            }
            if ui.button("🧹 Créer une copie nettoyée").clicked() {
                self.nettoyer();
            }
        }
        ui.label(&self.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichiers_liens_mails() {
        assert_eq!(vrai_type(b"MZ\x90\x00"), "programme Windows (exe/dll)");
        assert_eq!(vrai_type(b"%PDF-1.7"), "PDF");
        let r = analyser_fichier("facture.pdf.exe", b"MZ\x90\x00rest");
        assert!(r.alertes.iter().any(|a| a.0 == 2 && a.1.contains("double extension")));
        let r = analyser_fichier("photo.jpg", b"MZ\x90\x00");
        assert!(r.alertes.iter().any(|a| a.1.contains("ne correspond pas")));
        let r = analyser_fichier("x.txt", b"powershell -EncodedCommand SQBFAFgA");
        assert!(r.alertes.iter().any(|a| a.0 == 2));
        assert!(analyser_fichier("ok.png", b"\x89PNG\r\n\x1a\nxxxx").alertes.is_empty());
        let l = examiner_lien("https://paypa1-securite.com/login");
        assert!(l.iter().any(|x| x.0 == 2 && x.1.contains("paypal")), "{l:?}");
        assert!(examiner_lien("https://www.paypal.com/fr").iter().all(|x| x.0 == 0));
        assert!(examiner_lien("http://192.168.0.1/x").iter().any(|x| x.0 == 2));
        assert!(examiner_lien("https://xn--pypal-4ve.com").iter().any(|x| x.0 == 2));
        assert!(examiner_lien("https://bit.ly/abc").iter().any(|x| x.1.contains("raccourci")));
        assert!(examiner_lien("https://steamcommunity.com/id/x").iter().all(|x| x.0 == 0));
        assert!(examiner_lien("https://steamcommunlty.com/gift").iter().any(|x| x.0 == 2));
        assert_eq!(distance("paypal", "paypai"), 1);
        let m = "Received: from mail.evil.ru ([45.1.2.3])\nAuthentication-Results: mx.google.com;\n spf=fail smtp.mailfrom=evil.ru;\n dkim=none\nFrom: \"Service PayPal\" <securite@paypal-alerte.ru>\nReply-To: arnaque@gmail.com\nSubject: Votre compte\n\ncorps";
        let v = examiner_mail(m);
        assert!(v.iter().any(|x| x.0 == 2 && x.1.contains("SPF")));
        assert!(v.iter().any(|x| x.1.contains("paypal")));
        assert!(v.iter().any(|x| x.1.contains("45.1.2.3")));
        let c = chaines(b"\x00\x01http://exemple.com\x00\x02a\x00b\x00c\x00d\x00e\x00\x00", 5);
        assert!(c.iter().any(|x| x.1 == "http://exemple.com"));
        assert!(c.iter().any(|x| x.1 == "abcde"));
        assert!(interessant("C:\\Windows\\cmd.exe"));
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x04, 0xAA, 0xBB, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22, 0xFF, 0xD9];
        assert_eq!(jpeg_sans_meta(&jpeg).unwrap(), vec![0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22, 0xFF, 0xD9]);
        assert!(jours_depuis("2000-01-01").unwrap() > 9000);
    }
}
