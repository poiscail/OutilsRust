//! Papiers : lettres toutes prêtes (résiliation, réclamation…) et CV + lettre de motivation.
//! Les PDF sont de vrais PDF texte (Helvetica) : nets, légers, et le texte se sélectionne.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------------------------------
// Petit moteur de mise en page → PDF
// ---------------------------------------------------------------------------------------------------

pub const A4: (f32, f32) = (595.0, 842.0);

/// Coordonnées en points depuis le coin haut-gauche ; `y` d'un texte = sa ligne de base.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Texte { x: f32, y: f32, taille: f32, gras: bool, c: [u8; 3], s: String },
    Rect { x: f32, y: f32, l: f32, h: f32, c: [u8; 3] },
    Ligne { x1: f32, y1: f32, x2: f32, y2: f32, e: f32, c: [u8; 3] },
    Photo { x: f32, y: f32, d: f32 },
}

/// Caractère → octet WinAnsi (les accents français, œ, €, « », …).
fn winansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '\u{202f}' => 0xA0,
        '€' => 0x80,
        '‚' => 0x82,
        '„' => 0x84,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        'Œ' => 0x8C,
        'œ' => 0x9C,
        'Ÿ' => 0x9F,
        '\t' => b' ',
        _ => b'?',
    }
}

const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_GRAS_MIN: [u16; 26] = [556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500];
const HELV_GRAS_MAJ: [u16; 26] = [722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611];
const LATIN: &[u8; 64] = b"AAAAAAACEEEEIIIIDNOOOOOxOUUUUYPsaaaaaaaceeeeiiiidnooooo/ouuuuypy";

fn chasse(b: u8, gras: bool) -> f32 {
    let b = if b >= 0xC0 { LATIN[(b - 0xC0) as usize] } else { b };
    let w = match b {
        b'a'..=b'z' if gras => HELV_GRAS_MIN[(b - b'a') as usize],
        b'A'..=b'Z' if gras => HELV_GRAS_MAJ[(b - b'A') as usize],
        0x20..=0x7E => HELV[(b - 0x20) as usize] + if gras && !b.is_ascii_alphanumeric() { 30 } else { 0 },
        0x85 | 0x97 | 0x8C => 1000,
        0x9C => 944,
        0x91 | 0x92 => 222,
        0x93 | 0x94 => 333,
        0xA0 => 278,
        0xB0 => 400,
        _ => 556,
    };
    w as f32 / 1000.0
}

pub fn largeur(s: &str, taille: f32, gras: bool) -> f32 {
    s.chars().map(|c| chasse(winansi(c), gras)).sum::<f32>() * taille
}

/// Coupe le texte en lignes de largeur `max` (les retours à la ligne sont gardés).
pub fn couper(s: &str, taille: f32, gras: bool, max: f32) -> Vec<String> {
    let mut lignes = Vec::new();
    for para in s.split('\n') {
        let mut l = String::new();
        for mot in para.split_whitespace() {
            let essai = if l.is_empty() { mot.to_string() } else { format!("{l} {mot}") };
            if largeur(&essai, taille, gras) > max && !l.is_empty() {
                lignes.push(std::mem::replace(&mut l, mot.to_string()));
            } else {
                l = essai;
            }
        }
        lignes.push(l);
    }
    lignes
}

/// Ajoute des lignes de texte les unes sous les autres, avec saut de page automatique.
pub struct Mise {
    pub pages: Vec<Vec<Op>>,
    pub y: f32,
    haut: f32,
    bas: f32,
    fond: Vec<Op>,
}

impl Mise {
    pub fn new(haut: f32, bas: f32, fond: Vec<Op>) -> Self {
        Mise { pages: vec![fond.clone()], y: haut, haut, bas, fond }
    }
    pub fn op(&mut self, o: Op) {
        self.pages.last_mut().unwrap().push(o);
    }
    /// Passe à la page suivante s'il ne reste pas `h` points.
    pub fn place(&mut self, h: f32) {
        if self.y + h > self.bas && self.y > self.haut {
            self.pages.push(self.fond.clone());
            self.y = self.haut;
        }
    }
    pub fn texte(&mut self, x: f32, larg: f32, s: &str, taille: f32, gras: bool, c: [u8; 3]) {
        for l in couper(s, taille, gras, larg) {
            let h = taille * 1.38;
            self.place(h);
            if !l.is_empty() {
                self.op(Op::Texte { x, y: self.y + taille, taille, gras, c, s: l });
            }
            self.y += h;
        }
    }
    /// Texte aligné à droite (une ligne).
    pub fn texte_droite(&mut self, droite: f32, s: &str, taille: f32, gras: bool, c: [u8; 3]) {
        let h = taille * 1.38;
        self.place(h);
        self.op(Op::Texte { x: droite - largeur(s, taille, gras), y: self.y + taille, taille, gras, c, s: s.into() });
        self.y += h;
    }
}

fn echapper(s: &str) -> Vec<u8> {
    let mut o = Vec::new();
    for c in s.chars() {
        let b = winansi(c);
        if matches!(b, b'(' | b')' | b'\\') {
            o.push(b'\\');
        }
        o.push(b);
    }
    o
}

fn rgb(c: [u8; 3]) -> String {
    format!("{:.3} {:.3} {:.3}", c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0)
}

/// Fabrique le PDF (A4). `photo` : image carrée affichée en rond par les `Op::Photo`.
pub fn pdf(pages: &[Vec<Op>], photo: Option<&RgbaImage>) -> Vec<u8> {
    let h = A4.1;
    let mut objets: Vec<Vec<u8>> = vec![
        Vec::new(), // catalogue (rempli à la fin)
        Vec::new(), // pages
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    let mut ressources = "/Font << /F1 3 0 R /F2 4 0 R >>".to_string();
    if let Some(p) = photo {
        let mut jpg = Vec::new();
        let rgb = image::DynamicImage::ImageRgba8(p.clone()).to_rgb8();
        let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(&rgb);
        let mut o = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", p.width(), p.height(), jpg.len()).into_bytes();
        o.extend_from_slice(&jpg);
        o.extend_from_slice(b"\nendstream");
        objets.push(o);
        ressources += " /XObject << /Im1 5 0 R >>";
    }
    let mut kids = Vec::new();
    for page in pages {
        let mut f: Vec<u8> = Vec::new();
        for o in page {
            match o {
                Op::Rect { x, y, l, h: ht, c } => f.extend(format!("{} rg {x:.2} {:.2} {l:.2} {ht:.2} re f\n", rgb(*c), h - y - ht).bytes()),
                Op::Ligne { x1, y1, x2, y2, e, c } => f.extend(format!("{} RG {e:.2} w {x1:.2} {:.2} m {x2:.2} {:.2} l S\n", rgb(*c), h - y1, h - y2).bytes()),
                Op::Texte { x, y, taille, gras, c, s } => {
                    f.extend(format!("BT /F{} {taille:.2} Tf {} rg {x:.2} {:.2} Td (", if *gras { 2 } else { 1 }, rgb(*c), h - y).bytes());
                    f.extend(echapper(s));
                    f.extend(b") Tj ET\n");
                }
                Op::Photo { x, y, d } if photo.is_some() => {
                    let r = d / 2.0;
                    let (cx, cy) = (x + r, h - y - r);
                    let k = 0.5523 * r;
                    f.extend(format!(
                        "q {:.2} {cy:.2} m {:.2} {:.2} {:.2} {:.2} {cx:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {cy:.2} c {:.2} {:.2} {:.2} {:.2} {cx:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {cy:.2} c h W n {d:.2} 0 0 {d:.2} {x:.2} {:.2} cm /Im1 Do Q\n",
                        cx + r, cx + r, cy + k, cx + k, cy + r, cy + r,
                        cx - k, cy + r, cx - r, cy + k, cx - r,
                        cx - r, cy - k, cx - k, cy - r, cy - r,
                        cx + k, cy - r, cx + r, cy - k, cx + r,
                        h - y - d
                    ).bytes());
                }
                Op::Photo { .. } => {}
            }
        }
        let n = objets.len();
        kids.push(n + 1);
        objets.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.0} {:.0}] /Contents {} 0 R /Resources << {ressources} >> >>", A4.0, A4.1, n + 2).into_bytes());
        let mut c = format!("<< /Length {} >>\nstream\n", f.len()).into_bytes();
        c.extend(f);
        c.extend(b"\nendstream");
        objets.push(c);
    }
    objets[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    objets[1] = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.iter().map(|k| format!("{k} 0 R")).collect::<Vec<_>>().join(" "), kids.len()).into_bytes();
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut pos = Vec::new();
    for (k, o) in objets.iter().enumerate() {
        pos.push(out.len());
        out.extend(format!("{} 0 obj\n", k + 1).bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objets.len() + 1).bytes());
    for p in pos {
        out.extend(format!("{p:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objets.len() + 1).bytes());
    out
}

/// Aperçu d'une page dans l'interface.
fn apercu(ui: &mut egui::Ui, page: &[Op], largeur_px: f32, photo: Option<&egui::TextureHandle>) {
    let e = largeur_px / A4.0;
    let (zone, _) = ui.allocate_exact_size(egui::vec2(largeur_px, A4.1 * e), egui::Sense::hover());
    let p = ui.painter_at(zone);
    p.rect_filled(zone, 2.0, Color32::WHITE);
    let pt = |x: f32, y: f32| zone.min + egui::vec2(x * e, y * e);
    let col = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    for o in page {
        match o {
            Op::Rect { x, y, l, h, c } => {
                p.rect_filled(egui::Rect::from_min_size(pt(*x, *y), egui::vec2(l * e, h * e)), 0.0, col(*c));
            }
            Op::Ligne { x1, y1, x2, y2, e: ep, c } => {
                p.line_segment([pt(*x1, *y1), pt(*x2, *y2)], egui::Stroke::new((ep * e).max(0.5), col(*c)));
            }
            Op::Texte { x, y, taille, gras, c, s } => {
                let t = p.text(pt(*x, *y + taille * 0.22), egui::Align2::LEFT_BOTTOM, s, egui::FontId::proportional((taille * e * 1.04).max(3.0)), col(*c));
                if *gras {
                    p.text(pt(*x, *y + taille * 0.22) + egui::vec2(0.35, 0.0), egui::Align2::LEFT_BOTTOM, s, egui::FontId::proportional((taille * e * 1.04).max(3.0)), col(*c));
                }
                let _ = t;
            }
            Op::Photo { x, y, d } => {
                let r = egui::Rect::from_min_size(pt(*x, *y), egui::vec2(d * e, d * e));
                match photo {
                    Some(t) => egui::Image::new(t).corner_radius(d * e / 2.0).paint_at(ui, r),
                    None => {
                        p.circle_filled(r.center(), d * e / 2.0, Color32::from_gray(200));
                    }
                }
            }
        }
    }
}

/// Aperçu de toutes les pages, côte à côte si la place le permet.
fn apercu_pages(ui: &mut egui::Ui, pages: &[Vec<Op>], photo: Option<&egui::TextureHandle>) {
    let l = (ui.available_width() - 10.0).clamp(240.0, 520.0);
    ui.horizontal_wrapped(|ui| {
        for p in pages {
            apercu(ui, p, l, photo);
        }
    });
}

/// Date du jour en toutes lettres : « 27 novembre 2026 ».
pub fn date_du_jour() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let z = s / 86400 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let j = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let a = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    const MOIS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
    format!("{}{} {} {a}", j, if j == 1 { "er" } else { "" }, MOIS[(m - 1) as usize])
}

fn enregistrer_pdf(nom: &str, octets: Vec<u8>) -> String {
    let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(nom).save_file() else { return String::new() };
    match std::fs::write(&p, octets) {
        Ok(_) => {
            crate::outil::ouvrir(&p);
            format!("✔ Enregistré : {}", p.display())
        }
        Err(e) => format!("✖ {e}"),
    }
}

fn nom_fichier(s: &str) -> String {
    let n: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    n.trim_matches('_').chars().take(60).collect()
}

// ---------------------------------------------------------------------------------------------------
// Lettre (mise en page française classique)
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Personne {
    pub nom: String,
    pub adresse: String,
    pub telephone: String,
    pub email: String,
    pub ville: String,
}

pub struct Lettre<'a> {
    pub de: &'a Personne,
    pub a: &'a str,
    pub objet: &'a str,
    pub reference: &'a str,
    pub recommande: bool,
    pub corps: &'a str,
    pub pj: &'a str,
}

const NOIR: [u8; 3] = [25, 25, 30];
const GRIS: [u8; 3] = [90, 90, 100];

pub fn mise_lettre(l: &Lettre) -> Vec<Vec<Op>> {
    let (g, d) = (70.0, A4.0 - 70.0);
    let mut m = Mise::new(60.0, A4.1 - 60.0, vec![]);
    // expéditeur
    m.texte(g, 250.0, &l.de.nom, 11.0, true, NOIR);
    for t in [l.de.adresse.as_str(), &l.de.telephone, &l.de.email] {
        if !t.trim().is_empty() {
            m.texte(g, 250.0, t.trim(), 10.0, false, GRIS);
        }
    }
    // destinataire
    m.y = m.y.max(150.0);
    let mut premiere = true;
    for t in l.a.trim().lines() {
        m.texte(320.0, d - 320.0, t.trim(), 11.0, premiere, NOIR);
        premiere = false;
    }
    m.y += 24.0;
    let ville = l.de.ville.trim();
    m.texte_droite(d, &format!("{}le {}", if ville.is_empty() { String::new() } else { format!("{ville}, ") }, date_du_jour()), 10.5, false, NOIR);
    m.y += 18.0;
    if l.recommande {
        m.texte(g, d - g, "Lettre recommandée avec accusé de réception", 10.0, true, NOIR);
    }
    if !l.reference.trim().is_empty() {
        m.texte(g, d - g, &format!("Réf. : {}", l.reference.trim()), 10.0, false, GRIS);
    }
    if !l.objet.trim().is_empty() {
        m.texte(g, d - g, &format!("Objet : {}", l.objet.trim()), 11.0, true, NOIR);
    }
    m.y += 16.0;
    for para in l.corps.trim().split("\n\n") {
        m.texte(g, d - g, para.trim(), 11.0, false, NOIR);
        m.y += 8.0;
    }
    m.y += 16.0;
    m.place(40.0);
    let signe = if l.de.nom.trim().is_empty() { "Signature" } else { l.de.nom.trim() };
    m.texte(380.0, d - 380.0, signe, 11.0, false, NOIR);
    if !l.pj.trim().is_empty() {
        m.y += 30.0;
        m.texte(g, d - g, &format!("Pièce(s) jointe(s) : {}", l.pj.trim()), 9.5, false, GRIS);
    }
    m.pages
}

// ---------------------------------------------------------------------------------------------------
// 5 · Lettres toutes prêtes
// ---------------------------------------------------------------------------------------------------

/// (clé, ce qu'on demande, texte si vide)
type Champ = (&'static str, &'static str, &'static str);

pub struct Modele {
    pub nom: &'static str,
    pub explication: &'static str,
    pub objet: &'static str,
    pub champs: &'static [Champ],
    pub corps: &'static str,
    pub recommande: bool,
}

const POLITESSE: &str = "Je vous prie d'agréer, Madame, Monsieur, l'expression de mes salutations distinguées.";

pub const MODELES: &[Modele] = &[
    Modele {
        nom: "✂ Résilier un abonnement",
        explication: "Box internet, forfait, salle de sport, magazine, assurance…",
        objet: "Résiliation de mon abonnement {service}",
        champs: &[("service", "Le service (ex. : box internet, salle de sport)", ""), ("numero", "Numéro de client ou de contrat", ""), ("date", "Date de fin souhaitée", "dès que possible, en respectant le préavis prévu par mon contrat")],
        corps: "Madame, Monsieur,\n\nPar la présente, je vous informe de ma décision de résilier mon abonnement {service} (client ou contrat n° {numero}), avec effet {date}.\n\nJe vous remercie de bien vouloir prendre en compte cette résiliation, d'arrêter tout prélèvement à partir de cette date et de m'adresser une confirmation écrite. Le cas échéant, merci de m'indiquer comment restituer le matériel.\n\n{politesse}",
        recommande: true,
    },
    Modele {
        nom: "🛠 Produit en panne ou défectueux",
        explication: "Garantie légale de conformité : 2 ans pour un produit neuf acheté à un professionnel.",
        objet: "Produit défectueux — commande n° {commande}",
        champs: &[("produit", "Le produit", ""), ("commande", "Numéro de commande ou de facture", ""), ("date", "Date d'achat", ""), ("probleme", "Le problème", ""), ("demande", "Ce que tu demandes", "la réparation ou au remplacement du produit")],
        corps: "Madame, Monsieur,\n\nLe {date}, j'ai acheté auprès de vous le produit suivant : {produit} (commande n° {commande}). Ce produit présente le défaut suivant : {probleme}.\n\nEn application de la garantie légale de conformité (articles L217-3 et suivants du Code de la consommation), je vous demande de procéder à {demande}, sans frais pour moi.\n\nVous trouverez ci-joint une copie de ma facture. Je vous remercie de me répondre dans les meilleurs délais.\n\n{politesse}",
        recommande: false,
    },
    Modele {
        nom: "↩ Annuler un achat en ligne (14 jours)",
        explication: "Droit de rétractation : 14 jours après la réception du colis, sans avoir à se justifier.",
        objet: "Exercice de mon droit de rétractation — commande n° {commande}",
        champs: &[("produit", "Le produit", ""), ("commande", "Numéro de commande", ""), ("date", "Date de réception", "")],
        corps: "Madame, Monsieur,\n\nJe vous informe par la présente de ma rétractation concernant l'achat du produit suivant : {produit} (commande n° {commande}), reçu le {date}.\n\nConformément à l'article L221-18 du Code de la consommation, je dispose d'un délai de quatorze jours pour exercer ce droit. Je vous remercie de me rembourser la totalité des sommes versées, frais de livraison initiaux compris, au plus tard quatorze jours après réception de ce courrier.\n\n{politesse}",
        recommande: false,
    },
    Modele {
        nom: "💶 Demander un remboursement",
        explication: "Commande annulée, double prélèvement, trop-perçu, service non rendu…",
        objet: "Demande de remboursement",
        champs: &[("motif", "Pourquoi tu dois être remboursé (ex. : ma commande annulée le 3 mars)", ""), ("reference", "Référence (commande, facture, dossier)", ""), ("montant", "Montant", "")],
        corps: "Madame, Monsieur,\n\nJe me permets de vous contacter au sujet de {motif} (référence : {reference}).\n\nJe vous demande en conséquence de bien vouloir me rembourser la somme de {montant}, par virement ou sur le moyen de paiement utilisé lors de mon règlement.\n\nDans l'attente de votre réponse, {politesse_min}",
        recommande: false,
    },
    Modele {
        nom: "⚠ Relance ferme (mise en demeure)",
        explication: "Quand une première demande est restée sans réponse. À envoyer en recommandé.",
        objet: "Mise en demeure",
        champs: &[("date", "Date de ton premier courrier", ""), ("sujet", "Ce que tu avais demandé (ex. : le remboursement de ma commande n° 123)", ""), ("delai", "Délai accordé", "quinze jours")],
        corps: "Madame, Monsieur,\n\nPar courrier du {date}, je vous ai demandé {sujet}. À ce jour, je n'ai reçu aucune réponse satisfaisante de votre part.\n\nPar la présente, je vous mets en demeure d'y donner suite dans un délai de {delai} à compter de la réception de cette lettre.\n\nÀ défaut, je me réserve le droit de saisir le conciliateur de justice ou le tribunal compétent afin de faire valoir mes droits.\n\n{politesse}",
        recommande: true,
    },
    Modele {
        nom: "🏠 Changement d'adresse",
        explication: "Pour ta banque, ton assurance, ton fournisseur d'énergie…",
        objet: "Changement d'adresse",
        champs: &[("numero", "Numéro de client", ""), ("date", "Date du déménagement", ""), ("ancienne", "Ancienne adresse", "")],
        corps: "Madame, Monsieur,\n\nJe vous informe de mon changement d'adresse à compter du {date}. Mon ancienne adresse était : {ancienne}. Ma nouvelle adresse est celle indiquée en haut de ce courrier.\n\nJe vous remercie de bien vouloir mettre à jour mon dossier (client n° {numero}) et de m'adresser désormais tout courrier à cette nouvelle adresse.\n\n{politesse}",
        recommande: false,
    },
    Modele {
        nom: "📝 Lettre libre",
        explication: "Une lettre bien présentée, avec ton propre texte.",
        objet: "",
        champs: &[],
        corps: "Madame, Monsieur,\n\n\n\n{politesse}",
        recommande: false,
    },
];

/// Remplit un modèle ; un champ vide devient sa valeur par défaut ou « [ce qu'on demande] ».
pub fn remplir(texte: &str, champs: &[Champ], valeurs: &[String]) -> String {
    let mut t = texte.replace("{politesse}", POLITESSE).replace("{politesse_min}", "je vous prie d'agréer, Madame, Monsieur, l'expression de mes salutations distinguées.");
    for (k, (cle, question, defaut)) in champs.iter().enumerate() {
        let v = valeurs.get(k).map(|s| s.trim()).unwrap_or("");
        let v = if !v.is_empty() {
            v.to_string()
        } else if !defaut.is_empty() {
            defaut.to_string()
        } else {
            format!("[{}]", question.split(" (").next().unwrap_or(question).to_lowercase())
        };
        t = t.replace(&format!("{{{cle}}}"), &v);
    }
    t
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
struct EtatLettres {
    moi: Personne,
    modele: usize,
    valeurs: Vec<String>,
    destinataire: String,
    objet: String,
    corps: String,
    a_la_main: bool,
    recommande: bool,
    pj: String,
}

pub struct Lettres {
    e: EtatLettres,
    message: String,
}

impl Lettres {
    pub fn new() -> Self {
        let mut l = Lettres { e: charger("lettres"), message: String::new() };
        if l.e.corps.is_empty() {
            l.choisir(0);
        }
        l
    }
    fn choisir(&mut self, k: usize) {
        self.e.modele = k;
        self.e.valeurs = vec![String::new(); MODELES[k].champs.len()];
        self.e.recommande = MODELES[k].recommande;
        self.e.a_la_main = false;
        self.refaire();
    }
    fn refaire(&mut self) {
        let m = &MODELES[self.e.modele.min(MODELES.len() - 1)];
        self.e.objet = remplir(m.objet, m.champs, &self.e.valeurs);
        self.e.corps = remplir(m.corps, m.champs, &self.e.valeurs);
    }
    fn pages(&self) -> Vec<Vec<Op>> {
        mise_lettre(&Lettre { de: &self.e.moi, a: &self.e.destinataire, objet: &self.e.objet, reference: "", recommande: self.e.recommande, corps: &self.e.corps, pj: &self.e.pj })
    }
}

fn champ_personne(ui: &mut egui::Ui, p: &mut Personne, avec_ville: bool) {
    egui::Grid::new(ui.next_auto_id()).num_columns(2).show(ui, |ui| {
        ui.label("Nom et prénom");
        ui.text_edit_singleline(&mut p.nom);
        ui.end_row();
        ui.label("Adresse");
        ui.add(egui::TextEdit::multiline(&mut p.adresse).desired_rows(2).hint_text("12 rue des Lilas\n75000 Paris"));
        ui.end_row();
        ui.label("Téléphone");
        ui.text_edit_singleline(&mut p.telephone);
        ui.end_row();
        ui.label("E-mail");
        ui.text_edit_singleline(&mut p.email);
        ui.end_row();
        if avec_ville {
            ui.label("Ville (pour la date)");
            ui.text_edit_singleline(&mut p.ville);
            ui.end_row();
        }
    });
}

impl Outil for Lettres {
    fn nom(&self) -> &'static str {
        "✉ Lettres toutes prêtes"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.e.clone();
        ui.heading("Lettres toutes prêtes");
        ui.weak("Choisis une lettre, remplis quelques cases : Outils écrit une lettre bien présentée, prête à imprimer, envoyer par la poste ou joindre à un e-mail.");
        ui.horizontal_wrapped(|ui| {
            for (k, m) in MODELES.iter().enumerate() {
                if ui.selectable_label(self.e.modele == k, m.nom).clicked() && self.e.modele != k {
                    self.choisir(k);
                }
            }
        });
        let m = &MODELES[self.e.modele.min(MODELES.len() - 1)];
        ui.label(RichText::new(m.explication).italics());
        ui.separator();
        let large = ui.available_width() > 900.0;
        let gauche = |s: &mut Self, ui: &mut egui::Ui| {
            egui::CollapsingHeader::new("🙂 Moi (gardé pour les prochaines lettres)").default_open(s.e.moi.nom.is_empty()).show(ui, |ui| champ_personne(ui, &mut s.e.moi, true));
            ui.label(RichText::new("À qui ?").strong());
            ui.add(egui::TextEdit::multiline(&mut s.e.destinataire).desired_rows(3).desired_width(f32::INFINITY).hint_text("Nom de l'entreprise ou de l'organisme\nService client\nAdresse\nCode postal et ville"));
            let mut change = false;
            if !m.champs.is_empty() {
                ui.label(RichText::new("Les détails").strong());
                s.e.valeurs.resize(m.champs.len(), String::new());
                egui::Grid::new("champs_lettre").num_columns(2).show(ui, |ui| {
                    for (k, (_, q, _)) in m.champs.iter().enumerate() {
                        ui.label(*q);
                        change |= ui.add(egui::TextEdit::singleline(&mut s.e.valeurs[k]).desired_width(260.0)).changed();
                        ui.end_row();
                    }
                });
            }
            ui.checkbox(&mut s.e.recommande, "Envoi en recommandé avec accusé de réception (conseillé pour une résiliation ou une relance)");
            ui.horizontal(|ui| {
                ui.label("Pièces jointes");
                ui.add(egui::TextEdit::singleline(&mut s.e.pj).hint_text("ex. : copie de la facture").desired_width(260.0));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Le texte").strong());
                if s.e.a_la_main && ui.small_button("↺ Refaire le texte depuis les cases").clicked() {
                    s.e.a_la_main = false;
                    change = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Objet :");
                if ui.add(egui::TextEdit::singleline(&mut s.e.objet).desired_width(f32::INFINITY)).changed() {
                    s.e.a_la_main = true;
                }
            });
            if ui.add(egui::TextEdit::multiline(&mut s.e.corps).desired_rows(10).desired_width(f32::INFINITY)).changed() {
                s.e.a_la_main = true;
            }
            if change && !s.e.a_la_main {
                s.refaire();
            }
            if s.e.corps.contains('[') {
                ui.colored_label(Color32::from_rgb(230, 160, 60), "Il reste des [cases] à remplir dans le texte.");
            }
            ui.horizontal(|ui| {
                if ui.button(RichText::new("📄 Enregistrer en PDF").strong()).clicked() {
                    let nom = format!("lettre_{}.pdf", nom_fichier(&s.e.objet));
                    s.message = enregistrer_pdf(&nom, pdf(&s.pages(), None));
                }
                if ui.button("📋 Copier le texte (pour un e-mail)").clicked() {
                    ui.ctx().copy_text(format!("Objet : {}\n\n{}\n\n{}", s.e.objet, s.e.corps.trim(), s.e.moi.nom));
                    s.message = "Texte copié.".into();
                }
            });
            ui.label(&s.message);
        };
        if large {
            ui.columns(2, |c| {
                egui::ScrollArea::vertical().id_salt("lettre_g").show(&mut c[0], |ui| gauche(self, ui));
                let pages = self.pages();
                egui::ScrollArea::vertical().id_salt("lettre_d").show(&mut c[1], |ui| apercu_pages(ui, &pages, None));
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                gauche(self, ui);
                ui.separator();
                let pages = self.pages();
                apercu_pages(ui, &pages, None);
            });
        }
        if self.e != avant {
            enregistrer("lettres", &self.e);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 6 · CV et lettre de motivation
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Bloc {
    pub dates: String,
    pub titre: String,
    pub lieu: String,
    pub details: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Cv {
    pub moi: Personne,
    pub titre: String,
    pub lien: String,
    pub photo: Option<PathBuf>,
    pub profil: String,
    pub experiences: Vec<Bloc>,
    pub formations: Vec<Bloc>,
    pub competences: String,
    pub langues: String,
    pub interets: String,
    pub modele: usize,
    pub couleur: [u8; 3],
    // lettre de motivation
    pub entreprise: String,
    pub adresse_entreprise: String,
    pub poste: String,
    pub annonce: String,
    pub situation: String,
    pub pourquoi: String,
    pub atouts: String,
    pub motivation: String,
    pub motivation_main: bool,
}

impl Default for Cv {
    fn default() -> Self {
        Cv {
            moi: Personne::default(),
            titre: String::new(),
            lien: String::new(),
            photo: None,
            profil: String::new(),
            experiences: vec![Bloc::default()],
            formations: vec![Bloc::default()],
            competences: String::new(),
            langues: String::new(),
            interets: String::new(),
            modele: 0,
            couleur: [36, 99, 170],
            entreprise: String::new(),
            adresse_entreprise: String::new(),
            poste: String::new(),
            annonce: String::new(),
            situation: String::new(),
            pourquoi: String::new(),
            atouts: String::new(),
            motivation: String::new(),
            motivation_main: false,
        }
    }
}

pub const MODELES_CV: [&str; 3] = ["Moderne (colonne colorée)", "Classique", "Sobre"];

fn liste(s: &str) -> Vec<String> {
    s.split(['\n', ',', ';']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

fn blocs_pleins(v: &[Bloc]) -> Vec<&Bloc> {
    v.iter().filter(|b| !(b.titre.trim().is_empty() && b.lieu.trim().is_empty() && b.details.trim().is_empty())).collect()
}

/// Section « titre » + blocs dans une colonne.
fn section(m: &mut Mise, x: f32, l: f32, titre: &str, coul: [u8; 3], trait_: bool) {
    m.y += 10.0;
    m.place(60.0);
    m.texte(x, l, &titre.to_uppercase(), 11.5, true, coul);
    if trait_ {
        m.op(Op::Ligne { x1: x, y1: m.y - 2.0, x2: x + l, y2: m.y - 2.0, e: 0.8, c: coul });
        m.y += 4.0;
    }
}

fn blocs(m: &mut Mise, x: f32, l: f32, v: &[&Bloc], coul: [u8; 3]) {
    for b in v {
        m.place(40.0);
        let haut = m.y;
        if !b.dates.trim().is_empty() {
            m.texte_droite(x + l, b.dates.trim(), 9.5, false, GRIS);
            m.y = haut;
        }
        let ld = largeur(b.dates.trim(), 9.5, false) + 12.0;
        m.texte(x, l - ld, b.titre.trim(), 11.0, true, NOIR);
        if !b.lieu.trim().is_empty() {
            m.texte(x, l, b.lieu.trim(), 10.0, false, coul);
        }
        for ligne in b.details.trim().lines().filter(|s| !s.trim().is_empty()) {
            let t = ligne.trim().trim_start_matches(['-', '•', '*']).trim();
            if ligne.trim().starts_with(['-', '•', '*']) {
                let y0 = m.y;
                m.place(14.0);
                m.op(Op::Texte { x, y: m.y.max(y0) + 9.8, taille: 9.8, gras: false, c: coul, s: "•".into() });
                m.texte(x + 10.0, l - 10.0, t, 9.8, false, NOIR);
            } else {
                m.texte(x, l, t, 9.8, false, NOIR);
            }
        }
        m.y += 6.0;
    }
}

pub fn mise_cv(cv: &Cv, photo: bool) -> Vec<Vec<Op>> {
    let coul = if cv.modele == 2 { [40, 40, 45] } else { cv.couleur };
    let contact: Vec<String> = [cv.moi.telephone.trim(), cv.moi.email.trim(), cv.moi.ville.trim(), cv.lien.trim()].iter().filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
    let exp = blocs_pleins(&cv.experiences);
    let form = blocs_pleins(&cv.formations);
    let comp = liste(&cv.competences);
    let lang = liste(&cv.langues);
    let inter = liste(&cv.interets);
    if cv.modele == 0 {
        // colonne colorée à gauche
        let lc = 185.0;
        let fond = vec![Op::Rect { x: 0.0, y: 0.0, l: lc, h: A4.1, c: coul }];
        let mut c = Mise::new(40.0, A4.1 - 30.0, vec![]);
        if photo {
            c.op(Op::Photo { x: (lc - 110.0) / 2.0, y: 40.0, d: 110.0 });
            c.y = 170.0;
        }
        let blanc = [255, 255, 255];
        let pale = [225, 232, 245];
        let (x, l) = (22.0, lc - 40.0);
        let bloc = |c: &mut Mise, titre: &str, v: &[String]| {
            if v.is_empty() {
                return;
            }
            c.y += 12.0;
            c.texte(x, l, &titre.to_uppercase(), 10.5, true, blanc);
            c.op(Op::Ligne { x1: x, y1: c.y - 2.0, x2: x + l, y2: c.y - 2.0, e: 0.6, c: pale });
            c.y += 4.0;
            for s in v {
                c.texte(x, l, s, 9.5, false, pale);
                c.y += 1.5;
            }
        };
        bloc(&mut c, "Contact", &contact);
        bloc(&mut c, "Compétences", &comp);
        bloc(&mut c, "Langues", &lang);
        bloc(&mut c, "Centres d'intérêt", &inter);
        let colonne = c.pages.swap_remove(0);
        let mut m = Mise::new(45.0, A4.1 - 45.0, fond.clone());
        m.pages[0].extend(colonne);
        let (x, l) = (lc + 30.0, A4.0 - lc - 60.0);
        m.texte(x, l, cv.moi.nom.trim(), 26.0, true, NOIR);
        if !cv.titre.trim().is_empty() {
            m.texte(x, l, cv.titre.trim(), 13.0, false, coul);
        }
        if !cv.profil.trim().is_empty() {
            section(&mut m, x, l, "Profil", coul, true);
            m.texte(x, l, cv.profil.trim(), 10.0, false, NOIR);
        }
        if !exp.is_empty() {
            section(&mut m, x, l, "Expériences", coul, true);
            blocs(&mut m, x, l, &exp, coul);
        }
        if !form.is_empty() {
            section(&mut m, x, l, "Formation", coul, true);
            blocs(&mut m, x, l, &form, coul);
        }
        return m.pages;
    }
    // classique (centré, en couleur) ou sobre (aligné à gauche, gris)
    let (x, l) = (55.0, A4.0 - 110.0);
    let mut m = Mise::new(45.0, A4.1 - 45.0, vec![]);
    let centre = cv.modele == 1;
    let ecrire = |m: &mut Mise, s: &str, t: f32, g: bool, c: [u8; 3]| {
        if s.trim().is_empty() {
            return;
        }
        if centre {
            for li in couper(s.trim(), t, g, l) {
                let w = largeur(&li, t, g);
                m.texte(x + (l - w) / 2.0, l, &li, t, g, c);
            }
        } else {
            m.texte(x, l, s.trim(), t, g, c);
        }
    };
    let haut = m.y;
    if photo {
        let d = 85.0;
        m.op(Op::Photo { x: x + l - d, y: haut, d });
    }
    ecrire(&mut m, &cv.moi.nom, 24.0, true, NOIR);
    ecrire(&mut m, &cv.titre, 13.0, false, coul);
    ecrire(&mut m, &contact.join("  ·  "), 9.5, false, GRIS);
    if photo {
        m.y = m.y.max(haut + 92.0);
    }
    m.y += 4.0;
    m.op(Op::Rect { x, y: m.y, l, h: if centre { 2.0 } else { 0.8 }, c: coul });
    m.y += 6.0;
    if !cv.profil.trim().is_empty() {
        section(&mut m, x, l, "Profil", coul, !centre);
        m.texte(x, l, cv.profil.trim(), 10.0, false, NOIR);
    }
    if !exp.is_empty() {
        section(&mut m, x, l, "Expériences", coul, !centre);
        blocs(&mut m, x, l, &exp, coul);
    }
    if !form.is_empty() {
        section(&mut m, x, l, "Formation", coul, !centre);
        blocs(&mut m, x, l, &form, coul);
    }
    for (t, v) in [("Compétences", &comp), ("Langues", &lang), ("Centres d'intérêt", &inter)] {
        if !v.is_empty() {
            section(&mut m, x, l, t, coul, !centre);
            m.texte(x, l, &v.join("  ·  "), 10.0, false, NOIR);
        }
    }
    m.pages
}

pub fn texte_motivation(cv: &Cv) -> String {
    let v = |s: &str, d: &str| if s.trim().is_empty() { format!("[{d}]") } else { s.trim().trim_end_matches('.').to_string() };
    let mut t = String::from("Madame, Monsieur,\n\n");
    let annonce = if cv.annonce.trim().is_empty() { String::new() } else { format!(", que j'ai découvert sur {}", cv.annonce.trim()) };
    t += &format!(
        "{}, je souhaite rejoindre {} au poste de {}{annonce}.",
        if cv.situation.trim().is_empty() { "[Ta situation actuelle, ex. : Actuellement en BTS commerce]".to_string() } else { cv.situation.trim().trim_end_matches('.').to_string() },
        v(&cv.entreprise, "l'entreprise"),
        v(&cv.poste, "le poste")
    );
    t += "\n\n";
    t += &format!("Votre entreprise m'attire particulièrement : {}.", v(&cv.pourquoi, "ce qui te plaît chez eux"));
    t += "\n\n";
    t += &format!(
        "Au fil de mon parcours, j'ai développé {}. J'ai la conviction que ces qualités me permettront de m'investir pleinement dans ce poste et d'apporter rapidement ma contribution à votre équipe.",
        v(&cv.atouts, "tes qualités et ce que tu sais faire")
    );
    t += "\n\nJ'aimerais beaucoup vous rencontrer pour vous présenter ma motivation lors d'un entretien, à la date qui vous conviendra.\n\n";
    t += POLITESSE;
    t
}

pub struct CvOutil {
    cv: Cv,
    onglet: usize,
    photo: Option<(PathBuf, RgbaImage, egui::TextureHandle)>,
    message: String,
}

fn charger_photo(p: &std::path::Path) -> Option<RgbaImage> {
    let img = image::open(p).ok()?.to_rgba8();
    let c = img.width().min(img.height());
    let carre = image::imageops::crop_imm(&img, (img.width() - c) / 2, (img.height() - c) / 4, c, c).to_image();
    Some(image::imageops::resize(&carre, 400, 400, image::imageops::FilterType::Lanczos3))
}

impl CvOutil {
    pub fn new() -> Self {
        CvOutil { cv: charger("cv"), onglet: 0, photo: None, message: String::new() }
    }
    fn lettre(&self) -> Vec<Vec<Op>> {
        let a = format!("{}\n{}", self.cv.entreprise.trim(), self.cv.adresse_entreprise.trim());
        let objet = format!("Candidature au poste de {}", if self.cv.poste.trim().is_empty() { "[poste]" } else { self.cv.poste.trim() });
        mise_lettre(&Lettre { de: &self.cv.moi, a: &a, objet: &objet, reference: "", recommande: false, corps: &self.cv.motivation, pj: "curriculum vitae" })
    }
}

fn editer_blocs(ui: &mut egui::Ui, v: &mut Vec<Bloc>, id: &str, titre: &str, lieu: &str) {
    let mut suppr = None;
    let n = v.len();
    for (k, b) in v.iter_mut().enumerate() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            egui::Grid::new((id, k)).num_columns(2).show(ui, |ui| {
                ui.label(titre);
                ui.add(egui::TextEdit::singleline(&mut b.titre).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label(lieu);
                ui.add(egui::TextEdit::singleline(&mut b.lieu).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("Dates");
                ui.add(egui::TextEdit::singleline(&mut b.dates).hint_text("2023 – 2025").desired_width(160.0));
                ui.end_row();
                ui.label("Détails");
                ui.add(egui::TextEdit::multiline(&mut b.details).desired_rows(2).desired_width(f32::INFINITY).hint_text("- une ligne commençant par un tiret = une puce"));
                ui.end_row();
            });
            if n > 1 && ui.small_button("🗑 Retirer").clicked() {
                suppr = Some(k);
            }
        });
    }
    if let Some(k) = suppr {
        v.remove(k);
    }
    if ui.small_button("➕ Ajouter").clicked() {
        v.push(Bloc::default());
    }
}

impl Outil for CvOutil {
    fn nom(&self) -> &'static str {
        "📝 CV et lettre de motivation"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.cv.clone();
        // photo (chargée une fois)
        if self.cv.photo.as_ref() != self.photo.as_ref().map(|p| &p.0) {
            self.photo = self.cv.photo.as_ref().and_then(|p| charger_photo(p).map(|img| {
                let c = egui::ColorImage::from_rgba_unmultiplied([400, 400], img.as_raw());
                (p.clone(), img, ui.ctx().load_texture("photo_cv", c, egui::TextureOptions::LINEAR))
            }));
            if self.photo.is_none() {
                self.cv.photo = None;
            }
        }
        ui.heading("CV et lettre de motivation");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.onglet, 0, RichText::new("📄 Mon CV").strong());
            ui.selectable_value(&mut self.onglet, 1, RichText::new("✉ Lettre de motivation").strong());
        });
        ui.separator();
        let large = ui.available_width() > 900.0;
        let onglet = self.onglet;
        let formulaire = |s: &mut Self, ui: &mut egui::Ui| {
            let cv = &mut s.cv;
            if onglet == 0 {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Modèle :");
                    for (k, n) in MODELES_CV.iter().enumerate() {
                        ui.selectable_value(&mut cv.modele, k, *n);
                    }
                    if cv.modele != 2 {
                        ui.color_edit_button_srgb(&mut cv.couleur);
                    }
                });
                ui.label(RichText::new("Qui je suis").strong());
                egui::Grid::new("cv_moi").num_columns(2).show(ui, |ui| {
                    ui.label("Nom et prénom");
                    ui.text_edit_singleline(&mut cv.moi.nom);
                    ui.end_row();
                    ui.label("Titre");
                    ui.add(egui::TextEdit::singleline(&mut cv.titre).hint_text("ex. : Vendeur, Développeuse web, Recherche un stage"));
                    ui.end_row();
                    ui.label("Téléphone");
                    ui.text_edit_singleline(&mut cv.moi.telephone);
                    ui.end_row();
                    ui.label("E-mail");
                    ui.text_edit_singleline(&mut cv.moi.email);
                    ui.end_row();
                    ui.label("Ville");
                    ui.text_edit_singleline(&mut cv.moi.ville);
                    ui.end_row();
                    ui.label("Site ou LinkedIn");
                    ui.text_edit_singleline(&mut cv.lien);
                    ui.end_row();
                    ui.label("Photo");
                    ui.horizontal(|ui| {
                        if ui.button(if cv.photo.is_some() { "🖼 Changer" } else { "🖼 Ajouter une photo" }).clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file() {
                                cv.photo = Some(p);
                            }
                        }
                        if cv.photo.is_some() && ui.small_button("✖").clicked() {
                            cv.photo = None;
                        }
                    });
                    ui.end_row();
                });
                ui.label(RichText::new("Profil (2 ou 3 phrases sur toi)").strong());
                ui.add(egui::TextEdit::multiline(&mut cv.profil).desired_rows(3).desired_width(f32::INFINITY));
                ui.label(RichText::new("Expériences").strong());
                editer_blocs(ui, &mut cv.experiences, "exp", "Poste", "Entreprise, ville");
                ui.label(RichText::new("Formation").strong());
                editer_blocs(ui, &mut cv.formations, "form", "Diplôme", "École, ville");
                egui::Grid::new("cv_listes").num_columns(2).show(ui, |ui| {
                    for (t, v, h) in [("Compétences", &mut cv.competences, "Excel, Permis B, Photoshop…"), ("Langues", &mut cv.langues, "Anglais (courant), Espagnol (notions)"), ("Centres d'intérêt", &mut cv.interets, "Football, Musique, Bénévolat")] {
                        ui.label(t);
                        ui.add(egui::TextEdit::multiline(v).desired_rows(2).desired_width(f32::INFINITY).hint_text(format!("{h}\n(une par ligne ou séparées par des virgules)")));
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("📄 Enregistrer le CV en PDF").strong()).clicked() {
                        let pages = mise_cv(&s.cv, s.photo.is_some());
                        let nom = format!("CV_{}.pdf", nom_fichier(&s.cv.moi.nom));
                        s.message = enregistrer_pdf(&nom, pdf(&pages, s.photo.as_ref().map(|p| &p.1)));
                    }
                });
            } else {
                ui.weak("Ton nom et tes coordonnées viennent de l'onglet « Mon CV ».");
                let mut change = false;
                egui::Grid::new("motiv").num_columns(2).show(ui, |ui| {
                    for (t, v, h) in [
                        ("Entreprise", &mut cv.entreprise, "Nom de l'entreprise"),
                        ("Poste", &mut cv.poste, "ex. : vendeur en alternance"),
                        ("Annonce vue sur", &mut cv.annonce, "ex. : Indeed, France Travail, votre site (facultatif)"),
                        ("Ta situation", &mut cv.situation, "ex. : Actuellement en terminale STMG"),
                        ("Pourquoi eux ?", &mut cv.pourquoi, "ex. : vos valeurs, vos produits, la proximité…"),
                        ("Tes atouts", &mut cv.atouts, "ex. : le sens du contact et l'habitude du travail en équipe"),
                    ] {
                        ui.label(t);
                        change |= ui.add(egui::TextEdit::singleline(v).hint_text(h).desired_width(f32::INFINITY)).changed();
                        ui.end_row();
                    }
                    ui.label("Adresse de l'entreprise");
                    ui.add(egui::TextEdit::multiline(&mut cv.adresse_entreprise).desired_rows(2).desired_width(f32::INFINITY));
                    ui.end_row();
                    ui.label("Ton adresse");
                    ui.add(egui::TextEdit::multiline(&mut cv.moi.adresse).desired_rows(2).desired_width(f32::INFINITY));
                    ui.end_row();
                });
                if cv.motivation.is_empty() || (change && !cv.motivation_main) {
                    cv.motivation = texte_motivation(cv);
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("La lettre").strong());
                    if cv.motivation_main && ui.small_button("↺ Refaire le texte depuis les cases").clicked() {
                        cv.motivation = texte_motivation(cv);
                        cv.motivation_main = false;
                    }
                });
                if ui.add(egui::TextEdit::multiline(&mut cv.motivation).desired_rows(12).desired_width(f32::INFINITY)).changed() {
                    cv.motivation_main = true;
                }
                ui.weak("💡 Relis et personnalise le texte : une lettre écrite pour CETTE entreprise fait toute la différence.");
                if ui.button(RichText::new("📄 Enregistrer la lettre en PDF").strong()).clicked() {
                    let nom = format!("Lettre_motivation_{}.pdf", nom_fichier(&s.cv.entreprise));
                    s.message = enregistrer_pdf(&nom, pdf(&s.lettre(), None));
                }
            }
            ui.label(&s.message);
        };
        let pages = |s: &Self| if onglet == 0 { mise_cv(&s.cv, s.photo.is_some()) } else { s.lettre() };
        if large {
            let (g, d) = ui.available_rect_before_wrap().split_left_right_at_fraction(0.5);
            let mut cg = ui.new_child(egui::UiBuilder::new().max_rect(g.shrink(4.0)));
            egui::ScrollArea::vertical().id_salt("cv_g").show(&mut cg, |ui| formulaire(self, ui));
            let mut cd = ui.new_child(egui::UiBuilder::new().max_rect(d.shrink(4.0)));
            let p = pages(self);
            let tex = self.photo.as_ref().map(|p| p.2.clone());
            egui::ScrollArea::vertical().id_salt("cv_d").show(&mut cd, |ui| apercu_pages(ui, &p, tex.as_ref()));
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                formulaire(self, ui);
                ui.separator();
                let p = pages(self);
                let tex = self.photo.as_ref().map(|p| p.2.clone());
                apercu_pages(ui, &p, tex.as_ref());
            });
        }
        if self.cv != avant {
            enregistrer("cv", &self.cv);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn papiers() {
        assert_eq!(winansi('é'), 0xE9);
        assert_eq!(winansi('œ'), 0x9C);
        assert_eq!(winansi('€'), 0x80);
        assert!((largeur("Hello", 10.0, false) - 22.78).abs() < 0.01);
        assert!(largeur("é", 10.0, false) == largeur("e", 10.0, false));
        assert!(couper("un deux trois quatre cinq six", 10.0, false, 60.0).len() > 1);
        // modèle rempli : les champs vides deviennent [à remplir] ou leur valeur par défaut
        let m = &MODELES[0];
        let t = remplir(m.corps, m.champs, &["box internet".into()]);
        assert!(t.contains("abonnement box internet") && t.contains("[numéro de client ou de contrat]") && t.contains("préavis"));
        assert!(!t.contains('{'));
        for m in MODELES {
            let t = remplir(m.corps, m.champs, &[]);
            assert!(!t.contains('{') && t.contains("salutations"), "{}", m.nom);
        }
        // un PDF valide, avec plusieurs pages si le texte est long
        let p = Personne { nom: "Camille Martin".into(), adresse: "1 rue A\n75000 Paris".into(), ville: "Paris".into(), ..Default::default() };
        let long = "Bla bla. ".repeat(900);
        let pages = mise_lettre(&Lettre { de: &p, a: "Société (X)\nService", objet: "Test", reference: "", recommande: true, corps: &long, pj: "" });
        assert!(pages.len() >= 2);
        let o = pdf(&pages, None);
        assert!(o.starts_with(b"%PDF-1.4") && o.ends_with(b"%%EOF\n"));
        let cherche = b"Soci\xE9t\xE9 \\(X\\)";
        assert!(o.windows(cherche.len()).any(|w| w == cherche));
        // CV : les 3 modèles, avec photo
        let mut cv = Cv { moi: p.clone(), titre: "Vendeuse".into(), profil: "Dynamique.".into(), competences: "Excel, Word".into(), ..Default::default() };
        cv.experiences[0] = Bloc { dates: "2024".into(), titre: "Vendeuse".into(), lieu: "Boutique".into(), details: "- accueil\n- caisse".into() };
        for k in 0..3 {
            cv.modele = k;
            let pages = mise_cv(&cv, true);
            assert_eq!(pages.len(), 1);
            assert!(pages[0].iter().any(|o| matches!(o, Op::Photo { .. })));
            let img = RgbaImage::from_pixel(8, 8, image::Rgba([200, 100, 50, 255]));
            let o = pdf(&pages, Some(&img));
            assert!(o.windows(8).any(|w| w == b"/Im1 Do "));
        }
        assert!(texte_motivation(&cv).contains("[l'entreprise]"));
        if let Ok(d) = std::env::var("PAPIERS_TEST") {
            std::fs::write(format!("{d}/lettre.pdf"), pdf(&mise_lettre(&Lettre { de: &p, a: "Free\nService résiliation\n75371 Paris Cedex 08", objet: "Résiliation", reference: "", recommande: true, corps: &remplir(MODELES[0].corps, MODELES[0].champs, &["box internet".into(), "123456".into()]), pj: "" }), None)).unwrap();
            cv.modele = 0;
            cv.experiences.push(Bloc { dates: "2022 – 2023".into(), titre: "Équipier polyvalent".into(), lieu: "Restaurant, Lyon".into(), details: "Service en salle, préparation, encaissement. Travail en équipe dans un rythme soutenu.".into() });
            cv.formations[0] = Bloc { dates: "2024".into(), titre: "Baccalauréat STMG".into(), lieu: "Lycée Victor-Hugo, Paris".into(), details: "Mention bien".into() };
            cv.langues = "Anglais (courant)\nEspagnol (notions)".into();
            cv.interets = "Football, Musique".into();
            let img = RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 4) as u8, (y * 4) as u8, 150, 255]));
            for k in 0..3 {
                cv.modele = k;
                std::fs::write(format!("{d}/cv{k}.pdf"), pdf(&mise_cv(&cv, true), Some(&img))).unwrap();
            }
        }
    }
}
