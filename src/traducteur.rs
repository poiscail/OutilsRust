//! Traducteur de sous-titres (.srt, .vtt, .ass, vidéos .mkv/.mp4) : port de TraducteurSousTitres (Python).
//! Mêmes fichiers : %APPDATA%\TraducteurSousTitres.json et TraducteurSousTitres_glossaire.txt.
use eframe::egui::{self, Color32, RichText};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, LazyLock};
use std::time::Duration;
use std::{fs, thread};

type R<T> = Result<T, String>;
type Trad<'a> = &'a mut dyn FnMut(&str) -> R<String>;
type Progres<'a> = &'a mut dyn FnMut(usize, usize) -> R<()>;

const ANNULE: &str = "Arrêté";

fn re(motif: &str) -> Regex {
    Regex::new(motif).unwrap()
}

// ---------------------------------------------------------------- lecture / écriture des sous-titres

static TIMING: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(?:\d{1,2}:)?\d{2}:\d{2}[,.]\d{1,3}\s*-->\s*(?:\d{1,2}:)?\d{2}:\d{2}[,.]\d{1,3}.*$")
});

#[derive(Clone, Debug, PartialEq)]
pub struct Bloc {
    numero: String,
    timing: String, // vide : bloc recopié tel quel (en-tête WEBVTT, NOTE, STYLE)
    lignes: Vec<String>,
}

fn bloc(numero: &str, timing: &str, lignes: &[&str]) -> Bloc {
    Bloc { numero: numero.into(), timing: timing.into(), lignes: lignes.iter().map(|s| s.to_string()).collect() }
}

/// Windows-1252 : comme Latin-1 sauf 0x80..0x9F.
const CP1252: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

/// Lit en UTF-8 (avec ou sans BOM), sinon en Windows-1252.
fn lire_fichier(chemin: &Path) -> R<String> {
    let brut = fs::read(chemin).map_err(|e| e.to_string())?;
    let sans_bom = brut.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&brut);
    Ok(match std::str::from_utf8(sans_bom) {
        Ok(s) => s.to_string(),
        Err(_) => brut.iter().map(|&b| if (0x80..0xA0).contains(&b) { CP1252[b as usize - 0x80] } else { b as char }).collect(),
    })
}

/// SRT ou WebVTT : le timing est en 1re ligne (VTT sans identifiant) ou en 2e.
fn parser(texte: &str) -> R<Vec<Bloc>> {
    let texte = texte.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocs: Vec<Bloc> = vec![];
    static VIDE: LazyLock<Regex> = LazyLock::new(|| re(r"\n\s*\n"));
    for morceau in VIDE.split(texte.trim_matches('\n')) {
        let lignes: Vec<&str> = morceau.split('\n').collect();
        if TIMING.is_match(lignes[0].trim()) {
            blocs.push(bloc("", lignes[0].trim(), &lignes[1..]));
        } else if lignes.len() > 1 && TIMING.is_match(lignes[1].trim()) {
            blocs.push(bloc(lignes[0].trim(), lignes[1].trim(), &lignes[2..]));
        } else if !blocs.is_empty() || texte.starts_with("WEBVTT") {
            blocs.push(bloc("", "", &lignes));
        } else {
            return Err(format!("Bloc de sous-titre invalide : {:?}", morceau.chars().take(60).collect::<String>()));
        }
    }
    if !blocs.iter().any(|b| !b.timing.is_empty()) {
        return Err("Aucun sous-titre trouvé dans le fichier.".into());
    }
    Ok(blocs)
}

fn ecrire(blocs: &[Bloc]) -> String {
    let parties: Vec<String> = blocs.iter().map(|b| {
        let mut l: Vec<&str> = [&b.numero, &b.timing].into_iter().filter(|x| !x.is_empty()).map(String::as_str).collect();
        l.extend(b.lignes.iter().map(String::as_str));
        l.join("\n")
    }).collect();
    parties.join("\n\n") + "\n"
}

fn ms(temps: &str) -> i64 {
    static T: LazyLock<Regex> = LazyLock::new(|| re(r"^([\d:]+)[,.](\d+)"));
    let Some(c) = T.captures(temps) else { return 0 };
    let mut hms: Vec<i64> = vec![0, 0];
    hms.extend(c[1].split(':').map(|x| x.parse::<i64>().unwrap_or(0)));
    let [h, m, s] = hms[hms.len() - 3..] else { unreachable!() };
    let frac: String = format!("{:0<3}", &c[2]).chars().take(3).collect();
    ((h * 60 + m) * 60 + s) * 1000 + frac.parse::<i64>().unwrap_or(0)
}

fn hms(ms: i64, sep: char) -> String {
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// (début ms, fin ms, reste après la fin, séparateur des millisecondes)
fn temps(timing: &str) -> (i64, i64, String, char) {
    static T: LazyLock<Regex> = LazyLock::new(|| re(r"^(\S+)\s*-->\s*(\S+)(.*)$"));
    let c = T.captures(timing).expect("timing déjà validé par TIMING");
    (ms(&c[1]), ms(&c[2]), c[3].to_string(), if c[1].contains(',') { ',' } else { '.' })
}

/// Retire les sous-titres vides et les doublons qui se suivent, corrige les chevauchements
/// (fin après le début du suivant) et renumérote à partir de 1.
fn reparer(blocs: &[Bloc], numeroter: bool) -> Vec<Bloc> {
    let mut sortie: Vec<Bloc> = vec![];
    let mut n = 0;
    for b in blocs {
        if b.timing.is_empty() {
            sortie.push(b.clone());
            continue;
        }
        if !b.lignes.iter().any(|l| !l.trim().is_empty()) {
            continue;
        }
        let (debut, fin, _, _) = temps(&b.timing);
        if let Some(prec) = sortie.last_mut().filter(|p| !p.timing.is_empty()) {
            let (d, f, reste, sep) = temps(&prec.timing);
            if prec.lignes == b.lignes {
                // doublon : on prolonge le précédent
                prec.timing = format!("{} --> {}{reste}", hms(d, sep), hms(f.max(fin), sep));
                continue;
            }
            if f > debut {
                prec.timing = format!("{} --> {}{reste}", hms(d, sep), hms(d.max(debut - 1), sep));
            }
        }
        n += 1;
        sortie.push(Bloc { numero: if numeroter { n.to_string() } else { b.numero.clone() }, ..b.clone() });
    }
    sortie
}

/// ASS/SSA : chaque « Dialogue: » devient un bloc ; numero = n° de ligne, timing = début jusqu'au texte.
fn parser_ass(texte: &str) -> R<(Vec<String>, Vec<Bloc>)> {
    let lignes: Vec<String> = texte.replace("\r\n", "\n").split('\n').map(String::from).collect();
    let mut blocs = vec![];
    for (i, ligne) in lignes.iter().enumerate() {
        let parties: Vec<&str> = ligne.splitn(10, ',').collect();
        if ligne.starts_with("Dialogue:") && parties.len() == 10 {
            blocs.push(Bloc { numero: i.to_string(), timing: parties[..9].join(",") + ",",
                              lignes: parties[9].split("\\N").map(String::from).collect() });
        }
    }
    if blocs.is_empty() {
        return Err("Aucune ligne « Dialogue: » trouvée dans le fichier.".into());
    }
    Ok((lignes, blocs))
}

fn ecrire_ass(lignes: &[String], blocs: &[Bloc]) -> String {
    let mut lignes = lignes.to_vec();
    for b in blocs {
        lignes[b.numero.parse::<usize>().unwrap()] = b.timing.clone() + &b.lignes.join("\\N").replace('\n', "\\N");
    }
    lignes.join("\n")
}

fn ass_hms(ms: i64) -> String {
    format!("{}:{:02}:{:02}.{:02}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000 / 10)
}

/// Retarde (ms > 0) ou avance (ms < 0) tous les sous-titres, jamais avant 0.
fn decaler(blocs: &[Bloc], decalage: i64, ass: bool) -> Vec<Bloc> {
    blocs.iter().map(|b| {
        if b.timing.is_empty() {
            return b.clone();
        }
        let timing = if ass {
            // « Dialogue: 0,0:00:01.00,0:00:02.00,... »
            let mut p: Vec<String> = b.timing.split(',').map(String::from).collect();
            for x in &mut p[1..3] {
                *x = ass_hms((ms(x) + decalage).max(0));
            }
            p.join(",")
        } else {
            let (d, f, reste, sep) = temps(&b.timing);
            format!("{} --> {}{reste}", hms((d + decalage).max(0), sep), hms((f + decalage).max(0), sep))
        };
        Bloc { timing, ..b.clone() }
    }).collect()
}

/// {\i1}…{\i0} devient <i>…</i> ; les autres codes ASS sont retirés.
fn ass_vers_html(ligne: &str) -> String {
    const CODES: [(&str, &str); 6] = [(r"\i1", "<i>"), (r"\i0", "</i>"), (r"\b1", "<b>"), (r"\b0", "</b>"), (r"\u1", "<u>"), (r"\u0", "</u>")];
    static ACC: LazyLock<Regex> = LazyLock::new(|| re(r"\{[^}]*\}"));
    ACC.replace_all(ligne, |c: &regex::Captures| CODES.iter().filter(|(k, _)| c[0].contains(k)).map(|(_, v)| *v).collect::<String>())
        .into_owned()
}

fn html_vers_ass(ligne: &str) -> String {
    static B: LazyLock<Regex> = LazyLock::new(|| re(r"<[^>]+>"));
    B.replace_all(ligne, |c: &regex::Captures| match c[0].to_lowercase().as_str() {
        "<i>" => r"{\i1}", "</i>" => r"{\i0}", "<b>" => r"{\b1}", "</b>" => r"{\b0}", "<u>" => r"{\u1}", "</u>" => r"{\u0}",
        _ => "",
    }).into_owned()
}

type Cue = (i64, i64, Vec<String>);

/// Forme commune à tous les formats : (début ms, fin ms, lignes en balises <i>/<b>).
fn vers_cues(blocs: &[Bloc], ass: bool) -> Vec<Cue> {
    blocs.iter().filter(|b| !b.timing.is_empty()).map(|b| {
        let lignes: Vec<String> = b.lignes.iter().flat_map(|x| x.split('\n')).map(String::from).collect();
        if ass {
            let p: Vec<&str> = b.timing.split(',').collect();
            (ms(p[1]), ms(p[2]), lignes.iter().map(|l| ass_vers_html(l)).collect())
        } else {
            let (d, f, _, _) = temps(&b.timing);
            (d, f, lignes)
        }
    }).collect()
}

const ENTETE_ASS: &str = "[Script Info]
ScriptType: v4.00+
PlayResX: 384
PlayResY: 288

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, \
Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, \
MarginV, Encoding
Style: Default,Arial,16,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
";

/// Écrit les cues en srt, vtt ou ass.
fn ecrire_format(cues: &[Cue], fmt: &str) -> String {
    if fmt == "ass" {
        return ENTETE_ASS.to_string() + &cues.iter().map(|(d, f, l)| {
            format!("Dialogue: 0,{},{},Default,,0,0,0,,{}\n", ass_hms(*d), ass_hms(*f),
                    l.iter().map(|x| html_vers_ass(x)).collect::<Vec<_>>().join("\\N"))
        }).collect::<String>();
    }
    let sep = if fmt == "srt" { ',' } else { '.' };
    let corps: Vec<String> = cues.iter().enumerate().map(|(n, (d, f, l))| {
        let num = if fmt == "srt" { format!("{}\n", n + 1) } else { String::new() };
        format!("{num}{} --> {}\n{}", hms(*d, sep), hms(*f, sep), l.join("\n"))
    }).collect();
    (if fmt == "vtt" { "WEBVTT\n\n" } else { "" }).to_string() + &corps.join("\n\n") + "\n"
}

// ---------------------------------------------------------------- traduction en gardant les balises

const BALISE: &str = r"(?:<[^>]+>|\{[^}]*\})";
const TAILLE_LOT: usize = 4500; // limite ~5000 caractères du service gratuit
const LONGUEUR_MAX: usize = 42; // norme courante : 42 caractères par ligne, 2 lignes max

static RE_BALISE: LazyLock<Regex> = LazyLock::new(|| re(BALISE));
static MOT: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"(?:{BALISE}|\S)+"))); // balises collées comprises

fn longueur(s: &str) -> usize {
    s.chars().count()
}

/// (balises/espaces de début, cœur, balises/espaces de fin)
fn decouper(ligne: &str) -> (String, String, String) {
    static L: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"(?s)^((?:{BALISE}|\s)*)(.*?)((?:{BALISE}|\s)*)$")));
    let c = L.captures(ligne).unwrap();
    (c[1].to_string(), c[2].to_string(), c[3].to_string())
}

/// Garde les balises du milieu si la traduction les a rendues telles quelles, sinon les retire.
fn balises_intactes(avant: &str, apres: &str) -> String {
    let compter = |s: &str| {
        let mut v: Vec<&str> = RE_BALISE.find_iter(s).map(|m| m.as_str()).collect();
        v.sort();
        v.into_iter().map(String::from).collect::<Vec<_>>()
    };
    if compter(avant) == compter(apres) {
        return apres.to_string();
    }
    static ESP: LazyLock<Regex> = LazyLock::new(|| re(r"\s{2,}"));
    ESP.replace_all(&RE_BALISE.replace_all(apres, ""), " ").into_owned()
}

/// Coupe le texte traduit en n lignes de longueur proche.
fn repartir(texte: &str, n: usize) -> Vec<String> {
    let mut mots: Vec<String> = MOT.find_iter(texte).map(|m| m.as_str().to_string()).collect();
    let mut lignes = vec![];
    for reste in (1..=n).rev() {
        if reste == 1 {
            lignes.push(mots.join(" "));
            break;
        }
        let cible = longueur(&mots.join(" ")) as f64 / reste as f64;
        let mut ligne: Vec<String> = mots.drain(..mots.len().min(1)).collect();
        // ajoute un mot tant que ça rapproche de la longueur visée
        while mots.len() > reste - 1 {
            let avec = longueur(&ligne.join(" ")) + 1 + longueur(&mots[0]) - usize::from(ligne.is_empty());
            if (avec as f64 - cible).abs() >= (longueur(&ligne.join(" ")) as f64 - cible).abs() {
                break;
            }
            ligne.push(mots.remove(0));
        }
        lignes.push(ligne.join(" "));
    }
    lignes
}

/// Au moins n lignes ; passe à 2 si une ligne dépasserait LONGUEUR_MAX.
fn nb_lignes(texte: &str, n: usize) -> usize {
    if longueur(texte) > LONGUEUR_MAX * n && MOT.find_iter(texte).count() > 1 { n.max(2) } else { n }
}

/// Envoie les textes par lots (un par ligne) ; repasse un par un si le lot revient mal découpé.
fn traduire_textes(textes: &[String], traduire: Trad, progres: Progres) -> R<Vec<String>> {
    let mut resultats: Vec<String> = vec![];
    let mut lot: Vec<String> = vec![];
    let mut vider = |lot: &mut Vec<String>, resultats: &mut Vec<String>| -> R<()> {
        if lot.is_empty() {
            return Ok(());
        }
        let sortie: Vec<String> = traduire(&lot.join("\n"))?.split('\n').map(String::from).collect();
        if sortie.len() == lot.len() {
            resultats.extend(sortie);
        } else {
            for t in lot.iter() {
                let r = traduire(t)?;
                resultats.push(if r.is_empty() { t.clone() } else { r });
            }
        }
        lot.clear();
        progres(resultats.len(), textes.len())
    };
    for t in textes {
        if !lot.is_empty() && longueur(&lot.join("\n")) + 1 + longueur(t) > TAILLE_LOT {
            vider(&mut lot, &mut resultats)?;
        }
        lot.push(t.clone());
    }
    vider(&mut lot, &mut resultats)?;
    Ok(resultats)
}

/// Une entrée par ligne : « terme » (jamais traduit) ou « terme = traduction ». # = commentaire.
fn lire_glossaire(texte: &str) -> Vec<(String, String)> {
    let mut entrees: Vec<(String, String)> = texte.lines().filter_map(|l| {
        let l = l.split('#').next().unwrap().trim();
        if l.is_empty() {
            return None;
        }
        let (terme, cible) = l.split_once('=').unwrap_or((l, ""));
        let cible = if cible.trim().is_empty() { terme } else { cible };
        Some((terme.trim().to_string(), cible.trim().to_string()))
    }).collect();
    entrees.sort_by_key(|e| std::cmp::Reverse(longueur(&e.0))); // « Jon Snow » avant « Jon »
    entrees
}

fn lettre(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// Remplace chaque terme du glossaire (mot entier) par un jeton <gN> que le moteur laisse tel quel.
fn proteger(texte: &str, glossaire: &[(String, String)]) -> String {
    let mut texte = texte.to_string();
    for (n, (terme, _)) in glossaire.iter().enumerate() {
        if terme.is_empty() {
            continue;
        }
        let (mut sortie, mut i) = (String::new(), 0);
        while let Some(p) = texte[i..].find(terme.as_str()) {
            let (a, b) = (i + p, i + p + terme.len());
            if !lettre(texte[..a].chars().next_back()) && !lettre(texte[b..].chars().next()) {
                sortie += &texte[i..a];
                sortie += &format!("<g{n}>");
                i = b;
            } else {
                let suivant = a + texte[a..].chars().next().unwrap().len_utf8();
                sortie += &texte[i..suivant];
                i = suivant;
            }
        }
        sortie += &texte[i..];
        texte = sortie;
    }
    texte
}

fn restaurer(texte: &str, glossaire: &[(String, String)]) -> String {
    static G: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)<\s*g\s*(\d+)\s*>"));
    G.replace_all(texte, |c: &regex::Captures| {
        c[1].parse::<usize>().ok().and_then(|n| glossaire.get(n)).map(|e| e.1.clone()).unwrap_or_default()
    }).into_owned()
}

/// Numéros et timings recopiés tels quels ; blocs sans timing (en-tête WEBVTT, NOTE, STYLE) non traduits.
fn traduire_blocs(blocs: &[Bloc], traduire: Trad, progres: Progres, glossaire: &[(String, String)]) -> R<Vec<Bloc>> {
    let morceaux: Vec<Vec<(String, String, String)>> = blocs.iter()
        .map(|b| if b.timing.is_empty() { vec![] } else { b.lignes.iter().map(|l| decouper(l)).collect() }).collect();
    // segment = (bloc, lignes concernées, texte, dialogue) ; une phrase sur 2 lignes est traduite d'un coup,
    // mais les dialogues ("- ...") ligne par ligne
    let mut segments: Vec<(usize, Vec<usize>, String, bool)> = vec![];
    for (i, m) in morceaux.iter().enumerate() {
        let pleines: Vec<usize> = (0..m.len()).filter(|&k| !RE_BALISE.replace_all(&m[k].1, "").trim().is_empty()).collect();
        if pleines.iter().any(|&k| m[k].1.trim_start().starts_with('-')) {
            segments.extend(pleines.iter().map(|&k| (i, vec![k], m[k].1.trim().to_string(), true)));
        } else if !pleines.is_empty() {
            let texte = pleines.iter().map(|&k| m[k].1.trim()).collect::<Vec<_>>().join(" ");
            segments.push((i, pleines, texte, false));
        }
    }
    let textes: Vec<String> = segments.iter().map(|s| proteger(&s.2, glossaire)).collect();
    let traduits = traduire_textes(&textes, traduire, progres)?;

    let mut nouvelles: Vec<HashMap<usize, String>> = vec![HashMap::new(); blocs.len()];
    for ((i, ks, original, dialogue), t) in segments.iter().zip(traduits) {
        let t = balises_intactes(original, &restaurer(t.trim(), glossaire));
        let lignes = repartir(&t, if *dialogue { ks.len() } else { nb_lignes(&t, ks.len()) });
        nouvelles[*i].extend(ks.iter().copied().zip(lignes.iter().cloned()));
        if lignes.len() > ks.len() {
            // ligne trop longue coupée en deux : la suite va avec la dernière
            nouvelles[*i].insert(*ks.last().unwrap(), lignes[ks.len() - 1..].join("\n"));
        }
    }
    Ok(blocs.iter().zip(&morceaux).zip(&nouvelles).map(|((b, m), n)| {
        if b.timing.is_empty() {
            return b.clone();
        }
        let lignes = m.iter().enumerate().map(|(k, (d, c, f))| format!("{d}{}{f}", n.get(&k).unwrap_or(c))).collect();
        Bloc { lignes, ..b.clone() }
    }).collect())
}

// ---------------------------------------------------------------- moteurs en ligne

#[derive(Clone, Copy, PartialEq)]
enum Moteur {
    Google,
    DeepL,
}

impl Moteur {
    fn nom(self) -> &'static str {
        match self {
            Moteur::Google => "Google (gratuit)",
            Moteur::DeepL => "DeepL (clé)",
        }
    }
}

fn agent() -> R<ureq::Agent> {
    let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    Ok(ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).timeout(Duration::from_secs(30)).build())
}

/// Réessaie avec attente croissante si le service limite (429) ou coupe.
fn envoyer(faire: impl Fn() -> Result<ureq::Response, ureq::Error>) -> R<Value> {
    for essai in 0..6 {
        match faire() {
            Ok(r) => return r.into_json().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(c, _)) if [429, 500, 502, 503].contains(&c) && essai < 5 => {}
            Err(ureq::Error::Transport(_)) if essai < 5 => {}
            Err(e) => return Err(e.to_string()),
        }
        thread::sleep(Duration::from_secs(1 << essai)); // 1, 2, 4, 8, 16 s
    }
    unreachable!()
}

/// Google : point d'accès public sans clé ; DeepL : clé gratuite sur deepl.com (500 000 caractères par mois).
/// Traduction d'un texte court avec Google (langue source détectée), pour la traduction instantanée.
pub fn traduire_google(texte: &str, langue: &str) -> R<String> {
    let mut t = moteur(Moteur::Google, langue, "auto", "")?;
    t(texte)
}

fn moteur(m: Moteur, langue: &str, source: &str, cle: &str) -> R<Box<dyn FnMut(&str) -> R<String> + Send>> {
    let agent = agent()?;
    let (langue, source, cle) = (langue.to_string(), source.to_string(), cle.to_string());
    match m {
        Moteur::Google => Ok(Box::new(move |texte: &str| {
            let v = envoyer(|| agent.post("https://translate.googleapis.com/translate_a/single")
                .query("client", "gtx").query("sl", &source).query("tl", &langue).query("dt", "t")
                .send_form(&[("q", texte)]))?;
            Ok(v[0].as_array().into_iter().flatten().filter_map(|m| m[0].as_str()).collect())
        })),
        Moteur::DeepL => {
            if cle.is_empty() {
                return Err("DeepL demande une clé (gratuite sur deepl.com/pro-api).".into());
            }
            let hote = if cle.ends_with(":fx") { "api-free.deepl.com" } else { "api.deepl.com" };
            let url = format!("https://{hote}/v2/translate");
            let cible = match langue.as_str() { "en" => "EN-US".into(), "pt" => "PT-PT".into(), "zh-CN" => "ZH-HANS".into(), l => l.to_uppercase() };
            let src = (source != "auto").then(|| source.split('-').next().unwrap().to_uppercase());
            Ok(Box::new(move |texte: &str| {
                let mut champs = vec![("text", texte), ("target_lang", &cible), ("tag_handling", "xml"), ("ignore_tags", "g")];
                if let Some(s) = &src {
                    champs.push(("source_lang", s));
                }
                let v = envoyer(|| agent.post(&url).set("Authorization", &format!("DeepL-Auth-Key {cle}")).send_form(&champs))?;
                v["translations"][0]["text"].as_str().map(String::from).ok_or_else(|| "Réponse DeepL inattendue.".to_string())
            }))
        }
    }
}

// ---------------------------------------------------------------- fichiers

const LANGUES: [(&str, &str); 14] = [
    ("Français", "fr"), ("Anglais", "en"), ("Espagnol", "es"), ("Allemand", "de"), ("Italien", "it"),
    ("Portugais", "pt"), ("Néerlandais", "nl"), ("Russe", "ru"), ("Arabe", "ar"), ("Japonais", "ja"),
    ("Chinois", "zh-CN"), ("Coréen", "ko"), ("Turc", "tr"), ("Polonais", "pl"),
];
const AUTO: &str = "Détection auto";
const EXTENSIONS: [&str; 4] = ["srt", "vtt", "ass", "ssa"];
const VIDEOS: [&str; 2] = ["mkv", "mp4"];
const FORMATS: [(&str, Option<&str>); 4] = [("Identique", None), ("SRT", Some("srt")), ("VTT", Some("vtt")), ("ASS", Some("ass"))];
/// codes ISO 639-2 attendus par les pistes de sous-titres des vidéos
const ISO3: [(&str, &str); 14] = [
    ("fr", "fre"), ("en", "eng"), ("es", "spa"), ("de", "ger"), ("it", "ita"), ("pt", "por"), ("nl", "dut"),
    ("ru", "rus"), ("ar", "ara"), ("ja", "jpn"), ("zh-CN", "chi"), ("ko", "kor"), ("tr", "tur"), ("pl", "pol"),
];
const MODELE_GLOSSAIRE: &str = "# Glossaire du traducteur de sous-titres
# Un terme par ligne :
#   Winterfell              -> jamais traduit
#   Hand of the King = Main du Roi   -> toujours traduit pareil
";

fn appdata() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default()
}

fn config_chemin() -> PathBuf {
    appdata().join("TraducteurSousTitres.json")
}

fn glossaire_chemin() -> PathBuf {
    appdata().join("TraducteurSousTitres_glossaire.txt")
}

fn ext(f: &Path) -> String {
    f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn suffixe(f: &Path) -> String {
    f.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()
}

fn stem(f: &Path) -> String {
    f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn nom(f: &Path) -> String {
    f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// film.fr.srt, ou film.fr (2).srt s'il existe déjà : on n'écrase jamais une traduction.
fn chemin_sortie(entree: &Path, langue: &str, suff: &str) -> PathBuf {
    let suff = if suff.is_empty() { suffixe(entree) } else { suff.to_string() };
    let base = if langue.is_empty() { stem(entree) } else { format!("{}.{langue}", stem(entree)) };
    let mut sortie = entree.with_file_name(format!("{base}{suff}"));
    let mut n = 2;
    while sortie.exists() {
        sortie = entree.with_file_name(format!("{base} ({n}){suff}"));
        n += 1;
    }
    sortie
}

/// film.fr.srt ou film.fr (2).srt : sortie d'une traduction précédente, à ne pas retraduire.
fn deja_traduit(f: &Path) -> bool {
    static L: LazyLock<Regex> = LazyLock::new(|| re(r"\.([\w-]+)(?: \(\d+\))?$"));
    L.captures(&stem(f)).is_some_and(|c| LANGUES.iter().any(|(_, code)| *code == &c[1]))
}

/// Fichiers de sous-titres (ou vidéos) donnés directement ou contenus dans les dossiers donnés.
fn lister(chemins: &[PathBuf]) -> Vec<PathBuf> {
    let mut fichiers = vec![];
    for c in chemins {
        if c.is_dir() {
            let mut dedans: Vec<PathBuf> = fs::read_dir(c).into_iter().flatten().flatten().map(|e| e.path())
                .filter(|f| f.is_file() && {
                    let e = ext(f);
                    EXTENSIONS.contains(&e.as_str()) && !deja_traduit(f)
                        || VIDEOS.contains(&e.as_str()) && !f.with_extension("srt").exists()
                }).collect();
            dedans.sort_by_key(|f| f.to_string_lossy().to_lowercase());
            fichiers.extend(dedans);
        } else if EXTENSIONS.contains(&ext(c).as_str()) || VIDEOS.contains(&ext(c).as_str()) {
            fichiers.push(c.clone());
        }
    }
    fichiers
}

/// Lance ffmpeg (téléchargé tout seul au besoin) sans ouvrir de console ; Err(stderr) s'il échoue.
fn ffmpeg(args: &[&std::ffi::OsStr]) -> R<()> {
    let r = crate::dependances::commande("ffmpeg")?.args(["-y", "-v", "error"]).args(args).output().map_err(|e| e.to_string())?;
    if r.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&r.stderr).trim().to_string()) }
}

/// Sort la piste de sous-titres d'une vidéo en film.srt.
fn extraire_sous_titres(video: &Path) -> R<PathBuf> {
    let sortie = video.with_extension("srt");
    if sortie.exists() {
        return Ok(sortie);
    }
    // shortcut: 1re piste de sous-titres seulement ; ajouter un choix de piste si on en a besoin
    if ffmpeg(&["-i".as_ref(), video.as_os_str(), "-map".as_ref(), "0:s:0".as_ref(), sortie.as_os_str()]).is_err() || !sortie.exists() {
        let _ = fs::remove_file(&sortie);
        return Err("Pas de piste de sous-titres texte dans la vidéo (les sous-titres en image ne sont pas lisibles).".into());
    }
    Ok(sortie)
}

/// Copie la vidéo avec les sous-titres en 1re piste (film.fr.mkv), sans réencoder image ni son.
fn integrer_video(video: &Path, sous_titres: &Path, langue: Option<&str>) -> R<PathBuf> {
    let sortie = chemin_sortie(video, langue.unwrap_or("sous-titres"), "");
    let mp4 = ext(video) == "mp4";
    let iso = ISO3.iter().find(|(c, _)| Some(*c) == langue).map_or("und", |(_, i)| i);
    let meta = format!("language={iso}");
    let mut a: Vec<&std::ffi::OsStr> = ["-i".as_ref(), video.as_os_str(), "-i".as_ref(), sous_titres.as_os_str()].to_vec();
    a.extend(["-map", "0:v?", "-map", "0:a?", "-map", "1", "-map", "0:s?"].map(std::ffi::OsStr::new));
    if !mp4 {
        a.extend(["-map", "0:t?"].map(std::ffi::OsStr::new));
    }
    a.extend(["-c", "copy"].map(std::ffi::OsStr::new));
    if mp4 {
        a.extend(["-c:s", "mov_text"].map(std::ffi::OsStr::new));
    }
    a.extend(["-metadata:s:s:0", &meta, "-disposition:s:0", "default"].map(std::ffi::OsStr::new));
    a.push(sortie.as_os_str());
    match ffmpeg(&a) {
        Ok(()) if sortie.exists() => Ok(sortie),
        r => {
            let _ = fs::remove_file(&sortie);
            let e = r.err().unwrap_or_default();
            let fin: String = e.chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect();
            Err(format!("Intégration dans la vidéo impossible : {fin}"))
        }
    }
}

fn glossaire() -> Vec<(String, String)> {
    fs::read_to_string(glossaire_chemin()).map(|t| lire_glossaire(&t)).unwrap_or_default()
}

#[derive(Clone)]
struct Options {
    source: String, // "auto" ou code langue
    moteur: Moteur,
    cle: String,
    glossaire: Vec<(String, String)>,
    reparer: bool,
    decalage: f64, // secondes
    fmt: Option<&'static str>,
    integrer: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { source: "auto".into(), moteur: Moteur::Google, cle: String::new(), glossaire: vec![], reparer: false,
                  decalage: 0.0, fmt: None, integrer: false }
    }
}

enum Rendu {
    Brut,
    Ass(Vec<String>),
    Format(&'static str, bool), // format de sortie, entrée en ASS
}

impl Rendu {
    fn rendre(&self, blocs: &[Bloc]) -> String {
        match self {
            Rendu::Brut => ecrire(blocs),
            Rendu::Ass(lignes) => ecrire_ass(lignes, blocs),
            Rendu::Format(f, ass) => ecrire_format(&vers_cues(blocs, *ass), f),
        }
    }
}

struct Contenu {
    lu: PathBuf, // fichier de sous-titres lu (extrait de la vidéo le cas échéant)
    blocs: Vec<Bloc>,
    finaux: Vec<Bloc>,
    rendu: Rendu,
    suffixe: String,
}

/// Sans langue : seulement réparation, décalage et conversion de format.
fn traduire_contenu(entree: &Path, langue: Option<&str>, o: &Options, traduire: Option<Trad>, progres: Progres) -> R<Contenu> {
    let lu = if VIDEOS.contains(&ext(entree).as_str()) { extraire_sous_titres(entree)? } else { entree.to_path_buf() };
    let texte = lire_fichier(&lu)?;
    let ass = ["ass", "ssa"].contains(&ext(&lu).as_str());
    let (mut blocs, mut rendu) = if ass {
        let (lignes, blocs) = parser_ass(&texte)?;
        (blocs, Rendu::Ass(lignes))
    } else {
        let mut blocs = parser(&texte)?;
        if o.reparer {
            blocs = reparer(&blocs, ext(&lu) == "srt");
        }
        (blocs, Rendu::Brut)
    };
    if o.decalage != 0.0 {
        blocs = decaler(&blocs, (o.decalage * 1000.0).round() as i64, ass);
    }
    let mut suff = suffixe(&lu);
    if let Some(f) = o.fmt.filter(|f| *f != if ass { "ass" } else { ext(&lu).leak() }) {
        rendu = Rendu::Format(f, ass);
        suff = format!(".{f}");
    }
    let finaux = match langue {
        Some(l) => match traduire {
            Some(t) => traduire_blocs(&blocs, t, progres, &o.glossaire)?,
            None => traduire_blocs(&blocs, &mut moteur(o.moteur, l, &o.source, &o.cle)?, progres, &o.glossaire)?,
        },
        None => blocs.clone(),
    };
    Ok(Contenu { lu, blocs, finaux, rendu, suffixe: suff })
}

/// Écrit film.fr.srt (film.vtt si seulement converti, film.modifie.srt si seulement décalé ou réparé),
/// puis l'intègre à la vidéo d'origine si demandé.
fn enregistrer(origine: &Path, c: &Contenu, langue: Option<&str>, integrer: bool) -> R<PathBuf> {
    let etiquette = langue.unwrap_or(if c.suffixe.to_lowercase() != suffixe(&c.lu).to_lowercase() { "" } else { "modifie" });
    let sortie = chemin_sortie(&c.lu, etiquette, &c.suffixe);
    fs::write(&sortie, c.rendu.rendre(&c.finaux)).map_err(|e| e.to_string())?;
    if integrer && VIDEOS.contains(&ext(origine).as_str()) {
        return integrer_video(origine, &sortie, langue);
    }
    Ok(sortie)
}

fn traduire_fichier(entree: &Path, langue: Option<&str>, o: &Options, traduire: Option<Trad>, progres: Progres) -> R<PathBuf> {
    let c = traduire_contenu(entree, langue, o, traduire, progres)?;
    enregistrer(entree, &c, langue, o.integrer)
}

fn lire_config() -> serde_json::Map<String, Value> {
    fs::read_to_string(config_chemin()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn ecrire_config(valeurs: Value) {
    let mut c = lire_config();
    if let Value::Object(v) = valeurs {
        c.extend(v);
    }
    let _ = fs::write(config_chemin(), Value::Object(c).to_string()); // préférence non retenue : pas grave
}

// ---------------------------------------------------------------- interface

enum Msg {
    Etat(String),
    Progres(f32),
    Apercu(Box<Apercu>),
    Fin(String, Vec<String>),
}

struct Apercu {
    code: Option<String>,
    origine: PathBuf,
    c: Contenu,
    edition: Option<(usize, String)>,
}

pub struct Traducteur {
    fichiers: Vec<PathBuf>,
    langues: Vec<bool>,
    source: usize, // 0 = détection auto, sinon LANGUES[source - 1]
    moteur: Moteur,
    cle: String,
    fmt: usize,
    decalage: String,
    reparer: bool,
    apercu: bool,
    integrer: bool,
    etat: String,
    progres: f32,
    stop: Arc<AtomicBool>,
    rx: Option<mpsc::Receiver<Msg>>,
    erreurs: Option<String>,
    vue: Option<Box<Apercu>>,
}

fn une_ligne(b: &Bloc) -> String {
    b.lignes.join(" | ").replace('\n', " | ")
}

impl Traducteur {
    pub fn new() -> Self {
        let c = lire_config();
        let texte = |k: &str| c.get(k).and_then(Value::as_str).map(String::from);
        let vrai = |k: &str| c.get(k).and_then(Value::as_bool).unwrap_or(false);
        let deja: Vec<String> = match c.get("langues").and_then(Value::as_array) {
            Some(l) if !l.is_empty() => l.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => vec![texte("langue").unwrap_or("Français".into())],
        };
        let source = texte("source").and_then(|s| LANGUES.iter().position(|(n, _)| *n == s)).map_or(0, |i| i + 1);
        Traducteur {
            fichiers: vec![],
            langues: LANGUES.iter().map(|(n, _)| deja.iter().any(|d| d == n)).collect(),
            source,
            moteur: if texte("moteur").as_deref() == Some(Moteur::DeepL.nom()) { Moteur::DeepL } else { Moteur::Google },
            cle: texte("cle_deepl").or_else(|| std::env::var("DEEPL_API_KEY").ok()).unwrap_or_default(),
            fmt: texte("format").and_then(|f| FORMATS.iter().position(|(n, _)| *n == f)).unwrap_or(0),
            decalage: "0".into(),
            reparer: vrai("reparer"),
            apercu: vrai("apercu"),
            integrer: vrai("integrer"),
            etat: "Choisis des sous-titres (.srt, .vtt, .ass, .mkv) et une ou plusieurs langues. Glisse-les ici.".into(),
            progres: 0.0,
            stop: Arc::new(AtomicBool::new(false)),
            rx: None,
            erreurs: None,
            vue: None,
        }
    }

    fn relever(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Etat(e) => self.etat = e,
                Msg::Progres(p) => self.progres = p,
                Msg::Apercu(a) => self.vue = Some(a),
                Msg::Fin(e, erreurs) => {
                    self.etat = e;
                    self.rx = None;
                    if !erreurs.is_empty() {
                        self.erreurs = Some(erreurs.join("\n"));
                    }
                    return;
                }
            }
        }
    }

    fn lancer(&mut self, ctx: &egui::Context) {
        let cibles: Vec<usize> = (0..LANGUES.len()).filter(|&i| self.langues[i]).collect();
        let Ok(secondes) = self.decalage.trim().replace(',', ".").parse::<f64>().or_else(|e| if self.decalage.trim().is_empty() { Ok(0.0) } else { Err(e) }) else {
            self.erreurs = Some("Décalage invalide : un nombre de secondes, par ex. 1.5 ou -2.".into());
            return;
        };
        let fmt = FORMATS[self.fmt].1;
        let modifier_seul = secondes != 0.0 || fmt.is_some() || self.reparer;
        if self.fichiers.is_empty() || (cibles.is_empty() && !modifier_seul) {
            self.erreurs = Some("Choisis au moins un fichier et une langue cible (ou un décalage, un format, la réparation).".into());
            return;
        }
        ecrire_config(json!({
            "langues": cibles.iter().map(|&i| LANGUES[i].0).collect::<Vec<_>>(),
            "source": if self.source == 0 { AUTO } else { LANGUES[self.source - 1].0 },
            "moteur": self.moteur.nom(), "cle_deepl": self.cle.trim(), "reparer": self.reparer, "apercu": self.apercu,
            "integrer": self.integrer, "format": FORMATS[self.fmt].0,
        }));
        let o = Options {
            source: if self.source == 0 { "auto".into() } else { LANGUES[self.source - 1].1.into() },
            moteur: self.moteur, cle: self.cle.trim().into(), glossaire: glossaire(), reparer: self.reparer,
            decalage: secondes, fmt, integrer: self.integrer,
        };
        let mut taches: Vec<(PathBuf, Option<String>)> =
            self.fichiers.iter().flat_map(|f| cibles.iter().map(move |&i| (f.clone(), Some(LANGUES[i].1.to_string())))).collect();
        if taches.is_empty() {
            taches = self.fichiers.iter().map(|f| (f.clone(), None)).collect();
        }
        let voir = self.apercu && taches.len() == 1;
        self.stop = Arc::new(AtomicBool::new(false));
        self.progres = 0.0;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let (stop, ctx) = (self.stop.clone(), ctx.clone());
        thread::spawn(move || {
            let (mut erreurs, mut faits, total) = (vec![], 0, taches.len());
            for (n, (f, code)) in taches.iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let vers = code.as_deref().unwrap_or("modifié");
                let _ = tx.send(Msg::Etat(format!("Traitement {}/{total} : {} → {vers}", n + 1, nom(f))));
                ctx.request_repaint();
                let mut progres = |fait: usize, tot: usize| -> R<()> {
                    if stop.load(Ordering::Relaxed) {
                        return Err(ANNULE.into());
                    }
                    let _ = tx.send(Msg::Progres((n as f32 + fait as f32 / tot.max(1) as f32) / total as f32));
                    ctx.request_repaint();
                    Ok(())
                };
                let r = if voir {
                    traduire_contenu(f, code.as_deref(), &o, None, &mut progres).map(|c| {
                        let _ = tx.send(Msg::Apercu(Box::new(Apercu { code: code.clone(), origine: f.clone(), c, edition: None })));
                    })
                } else {
                    traduire_fichier(f, code.as_deref(), &o, None, &mut progres).map(|_| ())
                };
                match r {
                    Ok(()) => faits += 1,
                    Err(e) if e == ANNULE => break,
                    Err(e) => erreurs.push(format!("{} → {vers} : {e}", nom(f))),
                }
            }
            let arrete = stop.load(Ordering::Relaxed);
            let _ = tx.send(Msg::Progres(if arrete { 0.0 } else { 1.0 }));
            let fin = if arrete { "Arrêté" } else { "Terminé" };
            let _ = tx.send(Msg::Fin(format!("{fin} : {faits}/{total} fichier(s)."), erreurs));
            ctx.request_repaint();
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.relever();
        let deposes: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !deposes.is_empty() {
            self.fichiers = lister(&deposes);
        }
        let en_cours = self.rx.is_some();
        ui.heading("Traducteur de sous-titres");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Fichiers :");
            let resume = match self.fichiers.len() {
                0 => "aucun".to_string(),
                1 => self.fichiers[0].display().to_string(),
                n => format!("{n} fichiers"),
            };
            ui.label(RichText::new(resume).color(Color32::GRAY));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Dossier…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.fichiers = lister(&[d]);
                    }
                }
                if ui.button("Fichiers…").clicked() {
                    if let Some(f) = rfd::FileDialog::new()
                        .add_filter("Sous-titres ou vidéos", &["srt", "vtt", "ass", "ssa", "mkv", "mp4"]).pick_files() {
                        self.fichiers = lister(&f);
                    }
                }
            });
        });
        ui.add_space(6.0);
        egui::Grid::new("tr_reglages").num_columns(6).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("De :");
            let source = if self.source == 0 { AUTO } else { LANGUES[self.source - 1].0 };
            egui::ComboBox::from_id_salt("tr_source").selected_text(source).width(140.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.source, 0, AUTO);
                for (i, (n, _)) in LANGUES.iter().enumerate() {
                    ui.selectable_value(&mut self.source, i + 1, *n);
                }
            });
            ui.label("Moteur :");
            egui::ComboBox::from_id_salt("tr_moteur").selected_text(self.moteur.nom()).width(140.0).show_ui(ui, |ui| {
                for m in [Moteur::Google, Moteur::DeepL] {
                    ui.selectable_value(&mut self.moteur, m, m.nom());
                }
            });
            ui.label("Clé DeepL :");
            ui.add(egui::TextEdit::singleline(&mut self.cle).password(true).desired_width(160.0));
            ui.end_row();
            ui.label("Format :");
            egui::ComboBox::from_id_salt("tr_format").selected_text(FORMATS[self.fmt].0).width(140.0).show_ui(ui, |ui| {
                for (i, (n, _)) in FORMATS.iter().enumerate() {
                    ui.selectable_value(&mut self.fmt, i, *n);
                }
            });
            ui.label("Décalage (s) :");
            ui.add(egui::TextEdit::singleline(&mut self.decalage).desired_width(60.0));
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label("Vers :");
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        ui.set_width(180.0);
                        for (i, (n, _)) in LANGUES.iter().enumerate() {
                            ui.checkbox(&mut self.langues[i], *n);
                        }
                    });
                });
            });
            ui.add_space(20.0);
            ui.vertical(|ui| {
                ui.add_space(20.0);
                ui.checkbox(&mut self.reparer, "Réparer le fichier");
                ui.checkbox(&mut self.apercu, "Aperçu avant d'enregistrer");
                ui.checkbox(&mut self.integrer, "Intégrer à la vidéo");
                ui.add_space(6.0);
                if ui.button("Glossaire…").clicked() {
                    let g = glossaire_chemin();
                    if !g.exists() {
                        let _ = fs::write(&g, MODELE_GLOSSAIRE);
                    }
                    let _ = Command::new("explorer").arg(&g).spawn();
                }
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!en_cours, egui::Button::new("Traduire")).clicked() {
                self.lancer(ui.ctx());
            }
            if ui.add_enabled(en_cours, egui::Button::new("Arrêter")).clicked() {
                self.stop.store(true, Ordering::Relaxed);
            }
        });
        ui.add_space(8.0);
        ui.add(egui::ProgressBar::new(self.progres));
        ui.label(&self.etat);
        self.fenetres(ui.ctx());
    }

    fn fenetres(&mut self, ctx: &egui::Context) {
        if let Some(e) = &self.erreurs {
            let mut ouvert = true;
            let mut ok = false;
            egui::Window::new("Erreur").id(egui::Id::new("tr_erreur")).collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).open(&mut ouvert).show(ctx, |ui| {
                    ui.label(e.as_str());
                    ok = ui.button("OK").clicked();
                });
            if !ouvert || ok {
                self.erreurs = None;
            }
        }
        let Some(v) = &mut self.vue else { return };
        let (mut ouvert, mut action) = (true, None);
        let titre = format!("Aperçu : {} → {}", nom(&v.c.lu), v.code.clone().unwrap_or(v.c.suffixe.clone()));
        egui::Window::new(titre).id(egui::Id::new("tr_apercu")).default_size([900.0, 500.0]).open(&mut ouvert).show(ctx, |ui| {
            let Apercu { c, edition, .. } = &mut **v;
            ui.label(RichText::new("Double-clic sur une traduction pour la corriger.").color(Color32::GRAY));
            let mut valider = None;
            egui::ScrollArea::vertical().max_height(ui.available_height() - 36.0).show(ui, |ui| {
                egui::Grid::new("tr_grille").striped(true).num_columns(3).show(ui, |ui| {
                    ui.strong("Début");
                    ui.strong("Original");
                    ui.strong("Traduction");
                    ui.end_row();
                    for (i, (avant, apres)) in c.blocs.iter().zip(&c.finaux).enumerate() {
                        if apres.timing.is_empty() {
                            continue;
                        }
                        ui.label(apres.timing.split_whitespace().next().unwrap_or(""));
                        ui.scope(|ui| {
                            ui.set_width(380.0);
                            ui.add(egui::Label::new(une_ligne(avant)).wrap());
                        });
                        ui.scope(|ui| {
                            ui.set_width(380.0);
                            match edition {
                                Some((j, texte)) if *j == i => {
                                    ui.add(egui::TextEdit::multiline(texte).desired_rows(3).desired_width(380.0));
                                    if ui.button("OK").clicked() {
                                        valider = Some((i, texte.clone()));
                                    }
                                }
                                _ => {
                                    if ui.add(egui::Label::new(une_ligne(apres)).wrap().sense(egui::Sense::click())).double_clicked() {
                                        *edition = Some((i, apres.lignes.join("\n")));
                                    }
                                }
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            if let Some((i, texte)) = valider {
                c.finaux[i].lignes = texte.split('\n').map(String::from).collect();
                *edition = None;
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Enregistrer").clicked() {
                    action = Some(true);
                }
                if ui.button("Annuler").clicked() {
                    action = Some(false);
                }
            });
        });
        if action == Some(true) {
            match enregistrer(&v.origine, &v.c, v.code.as_deref(), self.integrer) {
                Ok(s) => {
                    self.etat = format!("Enregistré : {}", nom(&s));
                    self.vue = None;
                }
                Err(e) => self.erreurs = Some(e),
            }
        } else if !ouvert || action == Some(false) {
            self.vue = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXEMPLE: &str = "1\n00:00:01,000 --> 00:00:03,500\nHello, how are you today?\n\n\
2\n00:00:04,000 --> 00:00:07,250\n<i>I think we should leave\nbefore it gets dark.</i>\n\n\
3\n00:00:08,100 --> 00:00:10,000\n- Where is the car?\n- Behind the house.\n\n\
4\n00:01:02,500 --> 00:01:05,900\n{\\an8}Thank you very much, my friend.\n";

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("trad_test_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn maj(t: &str) -> R<String> {
        Ok(t.to_uppercase())
    }

    fn rien(_: usize, _: usize) -> R<()> {
        Ok(())
    }

    fn trad(f: &Path, langue: Option<&str>, o: &Options) -> PathBuf {
        traduire_fichier(f, langue, o, Some(&mut maj), &mut rien).unwrap()
    }

    fn blocs_maj(b: &[Bloc], g: &[(String, String)], t: Trad) -> Vec<Bloc> {
        traduire_blocs(b, t, &mut rien, g).unwrap()
    }

    #[test]
    fn aller_retour_crlf_bom_et_cp1252() {
        assert_eq!(ecrire(&parser(EXEMPLE).unwrap()), EXEMPLE);
        let d = dossier("lire");
        let f = d.join("a.srt");
        fs::write(&f, "\u{feff}1\r\n00:00:01,000 --> 00:00:02,000\r\nSalut\r\n").unwrap();
        let b = parser(&lire_fichier(&f).unwrap()).unwrap();
        assert_eq!((b[0].numero.as_str(), b[0].lignes.clone()), ("1", vec!["Salut".to_string()]));
        fs::write(&f, b"1\n00:00:01,000 --> 00:00:02,000\nCaf\xe9 \xe0 c\xf4t\xe9 \x80\n").unwrap();
        assert_eq!(parser(&lire_fichier(&f).unwrap()).unwrap()[0].lignes, ["Café à côté €"]);
    }

    #[test]
    fn traduction_garde_numeros_timings_et_balises() {
        let d = dossier("garde");
        let f = d.join("film.srt");
        fs::write(&f, EXEMPLE).unwrap();
        let sortie = trad(&f, Some("fr"), &Options::default());
        assert_eq!(nom(&sortie), "film.fr.srt");
        let (avant, apres) = (parser(EXEMPLE).unwrap(), parser(&fs::read_to_string(&sortie).unwrap()).unwrap());
        let forme = |b: &[Bloc]| b.iter().map(|b| (b.numero.clone(), b.timing.clone(), b.lignes.len())).collect::<Vec<_>>();
        assert_eq!(forme(&avant), forme(&apres));
        assert!(apres[1].lignes[0].starts_with("<i>") && apres[1].lignes.last().unwrap().ends_with("</i>"));
        assert_eq!(apres[3].lignes[0], "{\\an8}THANK YOU VERY MUCH, MY FRIEND.");
        assert_eq!(apres[2].lignes, ["- WHERE IS THE CAR?", "- BEHIND THE HOUSE."]);
    }

    #[test]
    fn lots_progression_et_arret() {
        let mut faux = |t: &str| -> R<String> { Ok(if t.contains('\n') { t.replace('\n', " ") } else { format!("{t}!") }) };
        let t = ["a".to_string(), "b".to_string()];
        assert_eq!(traduire_textes(&t, &mut faux, &mut rien).unwrap(), ["a!", "b!"]);
        let mut vu = vec![];
        traduire_textes(&t, &mut maj, &mut |f, n| { vu.push((f, n)); Ok(()) }).unwrap();
        assert_eq!(vu.last(), Some(&(2, 2)));
        assert_eq!(traduire_textes(&t, &mut maj, &mut |_, _| Err(ANNULE.into())), Err(ANNULE.to_string()));
    }

    #[test]
    fn repartir_et_balises() {
        assert_eq!(repartir("un deux trois quatre", 2), ["un deux", "trois quatre"]);
        assert_eq!(repartir("seul", 1), ["seul"]);
        let b = vec![bloc("1", "00:00:01,000 --> 00:00:02,000", &["Il est <i>très</i> tard"])];
        assert_eq!(blocs_maj(&b, &[], &mut |t| Ok(t.replace("tard", "late")))[0].lignes, ["Il est <i>très</i> late"]);
        // balise abîmée par le moteur : retirée plutôt que laissée cassée
        assert_eq!(blocs_maj(&b, &[], &mut |t| Ok(t.replace("</i>", "< / i>")))[0].lignes, ["Il est très tard"]);
    }

    #[test]
    fn ligne_trop_longue_coupee_en_deux() {
        let b = vec![bloc("1", "00:00:01,000 --> 00:00:02,000", &["<i>court</i>"])];
        let l = blocs_maj(&b, &[], &mut |_| Ok("un deux trois quatre cinq six sept huit neuf dix onze douze".into()));
        let l = &l[0].lignes[0];
        assert!(l.starts_with("<i>un") && l.ends_with("</i>") && l.matches('\n').count() == 1);
        assert!(l.split('\n').all(|x| longueur(x) <= 42));
    }

    #[test]
    fn vtt_et_ass() {
        let d = dossier("formats");
        let f = d.join("a.vtt");
        fs::write(&f, "WEBVTT\n\nNOTE commentaire\n\n00:01.000 --> 00:02.000 align:start\nHello\n\n2\n00:00:03.000 --> 00:00:04.000\nBye\n").unwrap();
        let s = trad(&f, Some("fr"), &Options::default());
        assert_eq!(nom(&s), "a.fr.vtt");
        assert_eq!(fs::read_to_string(&s).unwrap(),
                   "WEBVTT\n\nNOTE commentaire\n\n00:01.000 --> 00:02.000 align:start\nHELLO\n\n2\n00:00:03.000 --> 00:00:04.000\nBYE\n");
        let f = d.join("a.ass");
        fs::write(&f, "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1}Hello, you\\Nthere{\\i0}\n").unwrap();
        let s = fs::read_to_string(trad(&f, Some("fr"), &Options::default())).unwrap();
        assert_eq!(s.lines().nth(2).unwrap(), r"Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\i1}HELLO,\NYOU THERE{\i0}");
    }

    #[test]
    fn dossier_et_noms() {
        let d = dossier("noms");
        for n in ["a.srt", "a.fr.srt", "b.vtt", "notes.txt"] {
            fs::write(d.join(n), "").unwrap();
        }
        assert_eq!(lister(&[d.clone()]).iter().map(|f| nom(f)).collect::<Vec<_>>(), ["a.srt", "b.vtt"]);
        let e = d.join("film.srt");
        assert_eq!(nom(&chemin_sortie(&e, "fr", "")), "film.fr.srt");
        fs::write(d.join("film.fr.srt"), "").unwrap();
        assert_eq!(nom(&chemin_sortie(&e, "fr", "")), "film.fr (2).srt");
        assert!(deja_traduit(&d.join("film.fr (2).srt")));
    }

    #[test]
    fn glossaire_protege_les_termes() {
        let g = lire_glossaire("# commentaire\nWinterfell\nHand of the King = Main du Roi\n");
        let b = vec![bloc("1", "00:00:01,000 --> 00:00:02,000", &["The Hand of the King is in Winterfell"])];
        assert_eq!(blocs_maj(&b, &g, &mut maj)[0].lignes, ["THE Main du Roi IS IN Winterfell"]);
        assert_eq!(proteger("Winterfells Winterfell", &g), "Winterfells <g1>");
    }

    #[test]
    fn reparer_decaler_convertir() {
        let b = parser("5\n00:00:01,000 --> 00:00:03,000\nA\n\n9\n00:00:02,000 --> 00:00:04,000\nB\n\n\
10\n00:00:04,000 --> 00:00:05,000\nB\n\n11\n00:00:06,000 --> 00:00:07,000\n \n").unwrap();
        assert_eq!(reparer(&b, true), [bloc("1", "00:00:01,000 --> 00:00:01,999", &["A"]),
                                       bloc("2", "00:00:02,000 --> 00:00:05,000", &["B"])]);
        let b = parser("1\n00:00:01,000 --> 00:00:02,500\nA\n").unwrap();
        assert_eq!(decaler(&b, 1500, false)[0].timing, "00:00:02,500 --> 00:00:04,000");
        assert_eq!(decaler(&b, -5000, false)[0].timing, "00:00:00,000 --> 00:00:00,000");
        let (_, a) = parser_ass("Dialogue: 0,0:00:01.00,0:00:02.50,Default,,0,0,0,,Salut").unwrap();
        assert_eq!(decaler(&a, 1000, true)[0].timing, "Dialogue: 0,0:00:02.00,0:00:03.50,Default,,0,0,0,,");

        let cues = vers_cues(&parser("1\n00:00:01,000 --> 00:00:02,000\n<i>Salut</i>\ntoi\n").unwrap(), false);
        assert_eq!(ecrire_format(&cues, "vtt"), "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<i>Salut</i>\ntoi\n");
        let ass = ecrire_format(&cues, "ass");
        assert!(ass.ends_with("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1}Salut{\\i0}\\Ntoi\n"));
        let (_, b) = parser_ass(&ass).unwrap();
        assert_eq!(ecrire_format(&vers_cues(&b, true), "srt"), "1\n00:00:01,000 --> 00:00:02,000\n<i>Salut</i>\ntoi\n");
    }

    #[test]
    #[ignore] // réseau : cargo test -- --ignored
    fn google_en_ligne() {
        let mut t = moteur(Moteur::Google, "fr", "en", "").unwrap();
        assert_eq!(t("Hello
Thank you").unwrap().to_lowercase(), "bonjour
merci");
    }

    #[test]
    fn decaler_et_convertir_sans_traduire() {
        let d = dossier("sans");
        let f = d.join("a.srt");
        fs::write(&f, "1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
        let o = Options { decalage: 2.0, fmt: Some("vtt"), ..Options::default() };
        assert_eq!(fs::read_to_string(trad(&f, None, &o)).unwrap(), "WEBVTT\n\n00:00:03.000 --> 00:00:04.000\nHi\n");
        assert_eq!(nom(&trad(&f, None, &Options { decalage: -0.5, ..Options::default() })), "a.modifie.srt");
    }
}
