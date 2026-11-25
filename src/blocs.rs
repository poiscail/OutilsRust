//! Minecraft : image → pixel art en blocs (5) et générateur de structures (6). Aperçu 3D, liste des
//! blocs à récupérer, export .schem (WorldEdit, Litematica), .mcfunction, ou construction directe
//! sur ton serveur.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, Pos2, RichText};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// Envoi d'une commande à la console du serveur (None : pas de serveur ici).
pub type Envoi = Option<Arc<dyn Fn(&str) + Send + Sync>>;

/// Blocs pleins avec leur couleur moyenne (face du dessus), et leur famille.
pub const PALETTE: &[(&str, [u8; 3], u8)] = &[
    // 0 béton
    ("white_concrete", [207, 213, 214], 0), ("orange_concrete", [224, 97, 1], 0), ("magenta_concrete", [169, 48, 159], 0), ("light_blue_concrete", [36, 137, 199], 0),
    ("yellow_concrete", [241, 175, 21], 0), ("lime_concrete", [94, 169, 24], 0), ("pink_concrete", [214, 101, 143], 0), ("gray_concrete", [55, 58, 62], 0),
    ("light_gray_concrete", [125, 125, 115], 0), ("cyan_concrete", [21, 119, 136], 0), ("purple_concrete", [100, 32, 156], 0), ("blue_concrete", [45, 47, 143], 0),
    ("brown_concrete", [96, 60, 32], 0), ("green_concrete", [73, 91, 36], 0), ("red_concrete", [142, 33, 33], 0), ("black_concrete", [8, 10, 15], 0),
    // 1 laine
    ("white_wool", [234, 236, 237], 1), ("orange_wool", [241, 118, 20], 1), ("magenta_wool", [190, 69, 180], 1), ("light_blue_wool", [58, 175, 217], 1),
    ("yellow_wool", [249, 198, 40], 1), ("lime_wool", [112, 185, 26], 1), ("pink_wool", [238, 141, 172], 1), ("gray_wool", [63, 68, 72], 1),
    ("light_gray_wool", [142, 142, 135], 1), ("cyan_wool", [21, 138, 145], 1), ("purple_wool", [122, 42, 173], 1), ("blue_wool", [53, 57, 157], 1),
    ("brown_wool", [114, 72, 41], 1), ("green_wool", [85, 110, 28], 1), ("red_wool", [161, 39, 35], 1), ("black_wool", [21, 21, 26], 1),
    // 2 terre cuite
    ("terracotta", [152, 94, 68], 2), ("white_terracotta", [210, 178, 161], 2), ("orange_terracotta", [162, 84, 38], 2), ("magenta_terracotta", [150, 88, 109], 2),
    ("light_blue_terracotta", [113, 109, 138], 2), ("yellow_terracotta", [186, 133, 35], 2), ("lime_terracotta", [104, 118, 53], 2), ("pink_terracotta", [162, 78, 79], 2),
    ("gray_terracotta", [58, 42, 36], 2), ("light_gray_terracotta", [135, 107, 98], 2), ("cyan_terracotta", [87, 91, 91], 2), ("purple_terracotta", [118, 70, 86], 2),
    ("blue_terracotta", [74, 60, 91], 2), ("brown_terracotta", [77, 51, 36], 2), ("green_terracotta", [76, 83, 42], 2), ("red_terracotta", [143, 61, 47], 2),
    ("black_terracotta", [37, 23, 16], 2),
    // 3 naturels et précieux
    ("stone", [126, 126, 126], 3), ("cobblestone", [128, 127, 128], 3), ("oak_planks", [162, 131, 79], 3), ("spruce_planks", [115, 85, 49], 3),
    ("birch_planks", [192, 175, 121], 3), ("dark_oak_planks", [67, 43, 20], 3), ("sandstone", [216, 203, 155], 3), ("sand", [219, 207, 163], 3),
    ("snow_block", [249, 254, 254], 3), ("gold_block", [246, 208, 61], 3), ("diamond_block", [98, 237, 228], 3), ("emerald_block", [42, 203, 87], 3),
    ("lapis_block", [31, 67, 140], 3), ("iron_block", [220, 220, 220], 3), ("redstone_block", [175, 24, 5], 3), ("coal_block", [16, 15, 15], 3),
    ("quartz_block", [235, 229, 222], 3), ("prismarine", [99, 156, 151], 3), ("bricks", [150, 97, 83], 3), ("deepslate", [80, 80, 82], 3),
    ("obsidian", [15, 10, 24], 3), ("netherrack", [97, 38, 38], 3), ("end_stone", [219, 222, 158], 3), ("purpur_block", [169, 125, 169], 3),
    ("mossy_cobblestone", [110, 118, 94], 3), ("clay", [160, 166, 179], 3), ("honeycomb_block", [229, 148, 29], 3), ("glowstone", [171, 131, 84], 3),
    // autres blocs des structures (pas utilisés pour le pixel art)
    ("stone_bricks", [122, 121, 122], 9), ("glass", [175, 213, 219], 9), ("glass_pane", [170, 210, 216], 9), ("oak_log", [109, 85, 50], 9),
    ("oak_fence", [162, 131, 79], 9), ("torch", [255, 200, 80], 9), ("smooth_stone", [158, 158, 158], 9), ("polished_andesite", [132, 134, 133], 9),
    ("red_sandstone", [186, 99, 29], 9), ("water", [60, 90, 230], 9), ("grass_block", [93, 140, 58], 9), ("dirt", [134, 96, 67], 9),
];

pub fn couleur_bloc(nom: &str) -> [u8; 3] {
    PALETTE.iter().find(|p| p.0 == nom).map(|p| p.1).unwrap_or([150, 150, 150])
}

/// Distance perçue entre deux couleurs (« redmean »).
fn distance(a: [f32; 3], b: [u8; 3]) -> f32 {
    let r = (a[0] + b[0] as f32) / 2.0;
    let (dr, dg, db) = (a[0] - b[0] as f32, a[1] - b[1] as f32, a[2] - b[2] as f32);
    (2.0 + r / 256.0) * dr * dr + 4.0 * dg * dg + (2.0 + (255.0 - r) / 256.0) * db * db
}

/// Construction : grille l × h × p de blocs (indice dans `noms`, 0 = air).
#[derive(Clone, Default)]
pub struct Construction {
    pub l: usize,
    pub h: usize,
    pub p: usize,
    pub noms: Vec<String>,
    pub blocs: Vec<u16>,
}

impl Construction {
    pub fn nouvelle(l: usize, h: usize, p: usize) -> Self {
        Construction { l, h, p, noms: vec!["air".into()], blocs: vec![0; l * h * p] }
    }
    fn index(&self, x: usize, y: usize, z: usize) -> usize {
        (y * self.p + z) * self.l + x
    }
    pub fn mettre(&mut self, x: i64, y: i64, z: i64, nom: &str) {
        if x < 0 || y < 0 || z < 0 || x as usize >= self.l || y as usize >= self.h || z as usize >= self.p {
            return;
        }
        let id = match self.noms.iter().position(|n| n == nom) {
            Some(i) => i,
            None => {
                self.noms.push(nom.to_string());
                self.noms.len() - 1
            }
        } as u16;
        let i = self.index(x as usize, y as usize, z as usize);
        self.blocs[i] = id;
    }
    pub fn lire(&self, x: i64, y: i64, z: i64) -> u16 {
        if x < 0 || y < 0 || z < 0 || x as usize >= self.l || y as usize >= self.h || z as usize >= self.p {
            return 0;
        }
        self.blocs[self.index(x as usize, y as usize, z as usize)]
    }
    /// Nombre de blocs de chaque sorte (sans l'air), du plus utilisé au moins utilisé.
    pub fn comptes(&self) -> Vec<(String, usize)> {
        let mut c = vec![0usize; self.noms.len()];
        for b in &self.blocs {
            c[*b as usize] += 1;
        }
        let mut v: Vec<(String, usize)> = c.into_iter().enumerate().skip(1).filter(|(_, n)| *n > 0).map(|(i, n)| (self.noms[i].clone(), n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }
    /// Commandes « fill » relatives (~), une par suite de blocs identiques le long de x.
    pub fn commandes(&self, origine: Option<(i64, i64, i64)>) -> Vec<String> {
        let pos = |x: usize, y: usize, z: usize| match origine {
            Some((ox, oy, oz)) => format!("{} {} {}", ox + x as i64, oy + y as i64, oz + z as i64),
            None => format!("~{} ~{} ~{}", x, y, z),
        };
        let mut v = Vec::new();
        for y in 0..self.h {
            for z in 0..self.p {
                let mut x = 0;
                while x < self.l {
                    let b = self.blocs[self.index(x, y, z)];
                    let d = x;
                    while x < self.l && self.blocs[self.index(x, y, z)] == b {
                        x += 1;
                    }
                    if b != 0 {
                        v.push(format!("fill {} {} minecraft:{}", pos(d, y, z), pos(x - 1, y, z), self.noms[b as usize]));
                    }
                }
            }
        }
        v
    }
}

// --- NBT pour les fichiers .schem (format Sponge v2) ---

fn nbt_nom(o: &mut Vec<u8>, t: u8, nom: &str) {
    o.push(t);
    o.extend((nom.len() as u16).to_be_bytes());
    o.extend(nom.as_bytes());
}

/// Fichier .schem (gzip) lisible par WorldEdit et Litematica.
pub fn schem(c: &Construction) -> Vec<u8> {
    let mut o = Vec::new();
    nbt_nom(&mut o, 10, "Schematic");
    nbt_nom(&mut o, 3, "Version");
    o.extend(2i32.to_be_bytes());
    nbt_nom(&mut o, 3, "DataVersion");
    o.extend(3953i32.to_be_bytes()); // 1.21
    for (n, v) in [("Width", c.l), ("Height", c.h), ("Length", c.p)] {
        nbt_nom(&mut o, 2, n);
        o.extend((v as i16).to_be_bytes());
    }
    nbt_nom(&mut o, 11, "Offset");
    o.extend(3i32.to_be_bytes());
    o.extend([0u8; 12]);
    nbt_nom(&mut o, 3, "PaletteMax");
    o.extend((c.noms.len() as i32).to_be_bytes());
    nbt_nom(&mut o, 10, "Palette");
    for (i, n) in c.noms.iter().enumerate() {
        nbt_nom(&mut o, 3, &format!("minecraft:{n}"));
        o.extend((i as i32).to_be_bytes());
    }
    o.push(0);
    // index = x + z·largeur + y·largeur·longueur, en varint
    let mut d = Vec::new();
    for b in &c.blocs {
        let mut v = *b as u32;
        loop {
            if v < 0x80 {
                d.push(v as u8);
                break;
            }
            d.push((v as u8 & 0x7F) | 0x80);
            v >>= 7;
        }
    }
    nbt_nom(&mut o, 7, "BlockData");
    o.extend((d.len() as i32).to_be_bytes());
    o.extend(d);
    nbt_nom(&mut o, 9, "BlockEntities");
    o.push(10);
    o.extend(0i32.to_be_bytes());
    o.push(0);
    let mut g = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = g.write_all(&o);
    g.finish().unwrap_or_default()
}

// --- 5 · Image → blocs ---

/// Pixel art : chaque pixel devient le bloc le plus proche parmi les familles choisies.
pub fn pixel_art(img: &RgbaImage, largeur: u32, familles: &[u8], tramage: bool, au_sol: bool) -> Construction {
    let h = ((largeur as f32 * img.height() as f32 / img.width().max(1) as f32).round() as u32).max(1);
    let petite = image::imageops::resize(img, largeur.max(1), h, image::imageops::FilterType::Triangle);
    let choix: Vec<&(&str, [u8; 3], u8)> = PALETTE.iter().filter(|p| familles.contains(&p.2)).collect();
    let (l, hh) = (petite.width() as usize, petite.height() as usize);
    let mut c = if au_sol { Construction::nouvelle(l, 1, hh) } else { Construction::nouvelle(l, hh, 1) };
    let mut erreurs: Vec<[f32; 3]> = vec![[0.0; 3]; l * hh];
    for y in 0..hh {
        for x in 0..l {
            let p = petite.get_pixel(x as u32, y as u32);
            if p[3] < 128 {
                continue;
            }
            let e = erreurs[y * l + x];
            let voulu = [(p[0] as f32 + e[0]).clamp(0.0, 255.0), (p[1] as f32 + e[1]).clamp(0.0, 255.0), (p[2] as f32 + e[2]).clamp(0.0, 255.0)];
            let Some(b) = choix.iter().min_by(|a, b| distance(voulu, a.1).total_cmp(&distance(voulu, b.1))) else { continue };
            if tramage {
                // Floyd-Steinberg : l'écart de couleur est réparti sur les voisins
                let ecart = [voulu[0] - b.1[0] as f32, voulu[1] - b.1[1] as f32, voulu[2] - b.1[2] as f32];
                for (dx, dy, f) in [(1i64, 0i64, 7.0 / 16.0), (-1, 1, 3.0 / 16.0), (0, 1, 5.0 / 16.0), (1, 1, 1.0 / 16.0)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0 && (nx as usize) < l && (ny as usize) < hh {
                        let q = &mut erreurs[ny as usize * l + nx as usize];
                        for k in 0..3 {
                            q[k] += ecart[k] * f;
                        }
                    }
                }
            }
            if au_sol {
                c.mettre(x as i64, 0, y as i64, b.0);
            } else {
                c.mettre(x as i64, (hh - 1 - y) as i64, 0, b.0);
            }
        }
    }
    c
}

// --- 6 · Structures ---

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Modele {
    Maison,
    Tour,
    Arene,
    Pyramide,
    Dome,
    Pont,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Parametres {
    pub modele: Modele,
    pub taille: i64,
    pub hauteur: i64,
    pub etages: i64,
    pub mur: String,
    pub toit: String,
    pub sol: String,
}

impl Default for Parametres {
    fn default() -> Self {
        Parametres { modele: Modele::Maison, taille: 9, hauteur: 12, etages: 2, mur: "oak_planks".into(), toit: "dark_oak_planks".into(), sol: "spruce_planks".into() }
    }
}

pub fn structure(p: &Parametres) -> Construction {
    let t = p.taille.clamp(3, 80);
    match p.modele {
        Modele::Maison => {
            let (l, pr, e) = (t.max(5), (t * 3 / 4).max(5), p.etages.clamp(1, 5));
            let hm = e * 4;
            let toit_h = l / 2 + 1;
            let mut c = Construction::nouvelle(l as usize, (hm + toit_h + 1) as usize, pr as usize);
            for y in 0..=hm {
                for x in 0..l {
                    for z in 0..pr {
                        let bord = x == 0 || z == 0 || x == l - 1 || z == pr - 1;
                        let coin = (x == 0 || x == l - 1) && (z == 0 || z == pr - 1);
                        if y % 4 == 0 && !bord {
                            c.mettre(x, y, z, &p.sol);
                        } else if coin {
                            c.mettre(x, y, z, "oak_log");
                        } else if bord {
                            let fenetre = y % 4 == 2 && ((x % 3 == 1 && x > 0 && x < l - 1) || (z % 3 == 1 && z > 0 && z < pr - 1));
                            c.mettre(x, y, z, if fenetre { "glass_pane" } else { &p.mur });
                        }
                    }
                }
            }
            // porte (2 blocs vides) au milieu de la façade
            for y in 1..=2 {
                let i = c.index((l / 2) as usize, y as usize, (pr - 1) as usize);
                c.blocs[i] = 0;
            }
            // toit à deux pentes (en escalier), pignons dans le matériau des murs
            for k in 0..toit_h {
                let y = hm + 1 + k;
                if k > l - 1 - k {
                    break;
                }
                for z in 0..pr {
                    c.mettre(k, y, z, &p.toit);
                    c.mettre(l - 1 - k, y, z, &p.toit);
                }
                for x in k + 1..l - 1 - k {
                    for z in [0, pr - 1] {
                        c.mettre(x, y, z, &p.mur);
                    }
                }
            }
            c
        }
        Modele::Tour => {
            let r = t / 2;
            let h = p.hauteur.clamp(6, 120);
            let d = 2 * r + 1;
            let mut c = Construction::nouvelle(d as usize, (h + 2) as usize, d as usize);
            for y in 0..=h {
                for x in 0..d {
                    for z in 0..d {
                        let dist = (((x - r).pow(2) + (z - r).pow(2)) as f64).sqrt();
                        if dist <= r as f64 + 0.5 && dist > r as f64 - 0.5 {
                            let meurtriere = y % 5 == 3 && (x == r || z == r);
                            c.mettre(x, y, z, if meurtriere { "air" } else { &p.mur });
                        } else if dist <= r as f64 - 0.5 && y % 6 == 0 {
                            c.mettre(x, y, z, &p.sol);
                        }
                    }
                }
            }
            // créneaux
            for x in 0..d {
                for z in 0..d {
                    let dist = (((x - r).pow(2) + (z - r).pow(2)) as f64).sqrt();
                    if dist <= r as f64 + 0.5 && dist > r as f64 - 0.5 && (x + z) % 2 == 0 {
                        c.mettre(x, h + 1, z, &p.mur);
                    }
                }
            }
            c
        }
        Modele::Arene => {
            let r = t;
            let d = 2 * r + 1;
            let mut c = Construction::nouvelle(d as usize, 6, d as usize);
            for x in 0..d {
                for z in 0..d {
                    let dist = (((x - r).pow(2) + (z - r).pow(2)) as f64).sqrt();
                    if dist > r as f64 + 0.5 {
                        continue;
                    }
                    c.mettre(x, 0, z, if dist < r as f64 - 3.5 { &p.sol } else { &p.mur });
                    // gradins : 3 marches qui montent vers l'extérieur
                    for m in 0..3i64 {
                        if dist > (r - 3 + m) as f64 - 0.5 {
                            c.mettre(x, 1 + m, z, &p.mur);
                        }
                    }
                    if dist > r as f64 - 0.5 {
                        c.mettre(x, 4, z, &p.mur);
                        if (x + z) % 4 == 0 {
                            c.mettre(x, 5, z, "torch");
                        }
                    }
                }
            }
            c
        }
        Modele::Pyramide => {
            let mut c = Construction::nouvelle(t as usize, (t / 2 + 1) as usize, t as usize);
            for k in 0..=t / 2 {
                for x in k..t - k {
                    for z in k..t - k {
                        let bord = x == k || z == k || x == t - k - 1 || z == t - k - 1;
                        if bord || p.etages > 1 {
                            c.mettre(x, k, z, &p.mur);
                        }
                    }
                }
            }
            c
        }
        Modele::Dome => {
            let r = t / 2;
            let d = 2 * r + 1;
            let mut c = Construction::nouvelle(d as usize, (r + 1) as usize, d as usize);
            for y in 0..=r {
                for x in 0..d {
                    for z in 0..d {
                        let dist = (((x - r).pow(2) + y.pow(2) + (z - r).pow(2)) as f64).sqrt();
                        if dist <= r as f64 + 0.5 && dist > r as f64 - 0.5 {
                            c.mettre(x, y, z, &p.mur);
                        } else if y == 0 && dist <= r as f64 {
                            c.mettre(x, 0, z, &p.sol);
                        }
                    }
                }
            }
            c
        }
        Modele::Pont => {
            let lg = (t * 3).max(10);
            let h = 6i64;
            let mut c = Construction::nouvelle(lg as usize, (h + 2) as usize, 5);
            for x in 0..lg {
                for z in 0..5 {
                    c.mettre(x, h, z, &p.sol);
                }
                c.mettre(x, h + 1, 0, "oak_fence");
                c.mettre(x, h + 1, 4, "oak_fence");
                if x % 8 == 0 || x == lg - 1 {
                    for y in 0..h {
                        for z in 1..4 {
                            c.mettre(x, y, z, &p.mur);
                        }
                    }
                    c.mettre(x, h + 1, 0, "torch");
                    c.mettre(x, h + 1, 4, "torch");
                }
            }
            c
        }
    }
}

// --- Aperçu 3D isométrique ---

/// Dessine la construction en vue isométrique dans `r`.
pub fn apercu(p: &egui::Painter, r: egui::Rect, c: &Construction) {
    p.rect_filled(r, 6.0, Color32::from_rgb(28, 32, 44));
    if c.blocs.iter().all(|b| *b == 0) {
        return;
    }
    let (l, h, pr) = (c.l as f32, c.h as f32, c.p as f32);
    // taille d'un bloc pour que tout tienne
    let largeur_iso = l + pr;
    let hauteur_iso = (l + pr) / 2.0 + h;
    let a = (r.width() / largeur_iso).min(r.height() / hauteur_iso).clamp(0.5, 40.0);
    let ox = r.center().x - (l - pr) * a / 2.0;
    let oy = r.center().y - ((l + pr) / 2.0 * a / 2.0) + h * a / 2.0;
    let iso = |x: f32, y: f32, z: f32| Pos2::new(ox + (x - z) * a, oy + (x + z) * a / 2.0 - y * a);
    let mut visibles = Vec::new();
    for y in 0..c.h as i64 {
        for z in 0..c.p as i64 {
            for x in 0..c.l as i64 {
                let b = c.lire(x, y, z);
                if b != 0 && (c.lire(x + 1, y, z) == 0 || c.lire(x, y + 1, z) == 0 || c.lire(x, y, z + 1) == 0) {
                    visibles.push((x, y, z, b));
                }
            }
        }
    }
    visibles.sort_by_key(|(x, y, z, _)| (x + z, *y));
    for (x, y, z, b) in visibles.into_iter().take(60_000) {
        let col = couleur_bloc(&c.noms[b as usize]);
        let teinte = |f: f32| Color32::from_rgb((col[0] as f32 * f) as u8, (col[1] as f32 * f) as u8, (col[2] as f32 * f) as u8);
        let (xf, yf, zf) = (x as f32, y as f32, z as f32);
        if c.lire(x, y + 1, z) == 0 {
            p.add(egui::Shape::convex_polygon(vec![iso(xf, yf + 1.0, zf), iso(xf + 1.0, yf + 1.0, zf), iso(xf + 1.0, yf + 1.0, zf + 1.0), iso(xf, yf + 1.0, zf + 1.0)], teinte(1.0), egui::Stroke::NONE));
        }
        if c.lire(x + 1, y, z) == 0 {
            p.add(egui::Shape::convex_polygon(vec![iso(xf + 1.0, yf, zf), iso(xf + 1.0, yf + 1.0, zf), iso(xf + 1.0, yf + 1.0, zf + 1.0), iso(xf + 1.0, yf, zf + 1.0)], teinte(0.62), egui::Stroke::NONE));
        }
        if c.lire(x, y, z + 1) == 0 {
            p.add(egui::Shape::convex_polygon(vec![iso(xf, yf, zf + 1.0), iso(xf + 1.0, yf, zf + 1.0), iso(xf + 1.0, yf + 1.0, zf + 1.0), iso(xf, yf + 1.0, zf + 1.0)], teinte(0.8), egui::Stroke::NONE));
        }
    }
}

fn liste_blocs(ui: &mut egui::Ui, c: &Construction) {
    let total: usize = c.comptes().iter().map(|x| x.1).sum();
    ui.strong(format!("{total} blocs ({} × {} × {})", c.l, c.h, c.p));
    egui::ScrollArea::vertical().max_height(220.0).id_salt("liste_blocs").show(ui, |ui| {
        for (n, k) in c.comptes() {
            let col = couleur_bloc(&n);
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(col[0], col[1], col[2]), "■");
                ui.label(n.replace('_', " "));
                ui.weak(if k >= 64 { format!("{k} ({} piles + {})", k / 64, k % 64) } else { k.to_string() });
            });
        }
    });
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
struct Position {
    x: i64,
    y: i64,
    z: i64,
}

/// Exports et construction sur le serveur (commun aux deux outils).
fn exports(ui: &mut egui::Ui, c: &Construction, nom: &str, envoi: &Envoi, pos: &mut Position, message: &Arc<Mutex<String>>) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("💾 .schem (WorldEdit, Litematica)…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Schematic", &["schem"]).set_file_name(format!("{nom}.schem")).save_file() {
                *message.lock().unwrap() = std::fs::write(&p, schem(c)).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
            }
        }
        if ui.button("💾 .mcfunction (datapack)…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("mcfunction", &["mcfunction"]).set_file_name(format!("{nom}.mcfunction")).save_file() {
                *message.lock().unwrap() = std::fs::write(&p, c.commandes(None).join("\n")).map(|_| format!("✔ {} — lance-le avec /function, en te plaçant au coin de départ.", p.display())).unwrap_or_else(|e| e.to_string());
            }
        }
    });
    if let Some(envoi) = envoi {
        ui.horizontal_wrapped(|ui| {
            ui.label("Construire sur mon serveur à");
            for (n, v) in [("x", &mut pos.x), ("y", &mut pos.y), ("z", &mut pos.z)] {
                ui.label(n);
                ui.add(egui::DragValue::new(v));
            }
            if ui.button("🏗 Construire").on_hover_text("Le serveur doit tourner, et quelqu'un doit être près de l'endroit (zone chargée). F3 en jeu donne les coordonnées.").clicked() {
                let cmds = c.commandes(Some((pos.x, pos.y, pos.z)));
                let (e, m) = (envoi.clone(), message.clone());
                *m.lock().unwrap() = format!("Construction : {} commandes…", cmds.len());
                std::thread::spawn(move || {
                    for (k, cmd) in cmds.iter().enumerate() {
                        e(cmd);
                        if k % 20 == 19 {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                    }
                    *m.lock().unwrap() = "✔ Construit ! (si rien n'apparaît : la zone n'était pas chargée)".into();
                });
            }
        });
    }
    ui.label(message.lock().unwrap().clone());
}

pub struct PixelArtMc {
    image: Option<RgbaImage>,
    largeur: u32,
    familles: Vec<u8>,
    tramage: bool,
    au_sol: bool,
    c: Option<Construction>,
    envoi: Envoi,
    pos: Position,
    message: Arc<Mutex<String>>,
}

impl PixelArtMc {
    pub fn new(envoi: Envoi) -> Self {
        PixelArtMc { image: None, largeur: 64, familles: vec![0, 1], tramage: false, au_sol: false, c: None, envoi, pos: charger("blocs_position"), message: Arc::default() }
    }
}

impl Outil for PixelArtMc {
    fn nom(&self) -> &'static str {
        "🧱 Image en blocs Minecraft"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Image en pixel art de blocs");
        let mut refaire = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("🖼 Choisir une image…").clicked() {
                if let Some(i) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif"]).pick_file().and_then(|p| image::open(p).ok()) {
                    self.image = Some(i.to_rgba8());
                    refaire = true;
                }
            }
            ui.label("Largeur");
            refaire |= ui.add(egui::Slider::new(&mut self.largeur, 8..=256).suffix(" blocs")).changed();
        });
        ui.horizontal_wrapped(|ui| {
            for (f, n) in [(0u8, "Béton"), (1, "Laine"), (2, "Terre cuite"), (3, "Pierres et minerais")] {
                let mut on = self.familles.contains(&f);
                if ui.checkbox(&mut on, n).changed() {
                    if on {
                        self.familles.push(f);
                    } else {
                        self.familles.retain(|x| *x != f);
                    }
                    refaire = true;
                }
            }
            refaire |= ui.checkbox(&mut self.tramage, "Tramage (dégradés plus doux)").changed();
            refaire |= ui.checkbox(&mut self.au_sol, "À plat au sol (sinon un mur)").changed();
        });
        if refaire && !self.familles.is_empty() {
            self.c = self.image.as_ref().map(|i| pixel_art(i, self.largeur, &self.familles, self.tramage, self.au_sol));
        }
        let Some(c) = &self.c else {
            ui.weak("Choisis une image : un logo, un perso, une photo… Elle sera recréée en blocs.");
            return;
        };
        ui.horizontal_top(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(560.0), 420.0), egui::Sense::hover());
            apercu(&ui.painter_at(r), r, c);
            ui.vertical(|ui| liste_blocs(ui, c));
        });
        let avant = self.pos.clone();
        exports(ui, c, "pixel_art", &self.envoi, &mut self.pos, &self.message);
        if self.pos != avant {
            enregistrer("blocs_position", &self.pos);
        }
    }
}

pub struct Structures {
    p: Parametres,
    c: Construction,
    envoi: Envoi,
    pos: Position,
    message: Arc<Mutex<String>>,
}

impl Structures {
    pub fn new(envoi: Envoi) -> Self {
        let p: Parametres = charger("structures_mc");
        let c = structure(&p);
        Structures { p, c, envoi, pos: charger("blocs_position"), message: Arc::default() }
    }
}

const MATERIAUX: [&str; 14] = ["oak_planks", "spruce_planks", "birch_planks", "dark_oak_planks", "stone_bricks", "cobblestone", "bricks", "sandstone", "quartz_block", "deepslate", "glass", "white_concrete", "red_terracotta", "smooth_stone"];

impl Outil for Structures {
    fn nom(&self) -> &'static str {
        "🏰 Générateur de structures"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Générateur de structures");
        let avant = self.p.clone();
        ui.horizontal_wrapped(|ui| {
            for (m, n) in [(Modele::Maison, "🏠 Maison"), (Modele::Tour, "🗼 Tour"), (Modele::Arene, "🏟 Arène"), (Modele::Pyramide, "🔺 Pyramide"), (Modele::Dome, "🔮 Dôme"), (Modele::Pont, "🌉 Pont")] {
                ui.selectable_value(&mut self.p.modele, m, n);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(match self.p.modele {
                Modele::Arene | Modele::Dome | Modele::Tour => "Diamètre / rayon",
                Modele::Pont => "Longueur",
                _ => "Taille",
            });
            ui.add(egui::Slider::new(&mut self.p.taille, 3..=60));
            match self.p.modele {
                Modele::Maison => {
                    ui.label("Étages");
                    ui.add(egui::Slider::new(&mut self.p.etages, 1..=5));
                }
                Modele::Tour => {
                    ui.label("Hauteur");
                    ui.add(egui::Slider::new(&mut self.p.hauteur, 6..=80));
                }
                Modele::Pyramide => {
                    let mut pleine = self.p.etages > 1;
                    if ui.checkbox(&mut pleine, "Pleine").changed() {
                        self.p.etages = if pleine { 2 } else { 1 };
                    }
                }
                _ => {}
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (n, v) in [("Murs", &mut self.p.mur), ("Toit", &mut self.p.toit), ("Sol", &mut self.p.sol)] {
                ui.label(n);
                egui::ComboBox::from_id_salt(n).selected_text(v.replace('_', " ")).show_ui(ui, |ui| {
                    for m in MATERIAUX {
                        ui.selectable_value(v, m.to_string(), m.replace('_', " "));
                    }
                });
            }
        });
        if self.p != avant {
            enregistrer("structures_mc", &self.p);
            self.c = structure(&self.p);
        }
        ui.horizontal_top(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(560.0), 420.0), egui::Sense::hover());
            apercu(&ui.painter_at(r), r, &self.c);
            ui.vertical(|ui| liste_blocs(ui, &self.c));
        });
        let avant = self.pos.clone();
        let nom = format!("{:?}", self.p.modele).to_lowercase();
        exports(ui, &self.c, &nom, &self.envoi, &mut self.pos, &self.message);
        if self.pos != avant {
            enregistrer("blocs_position", &self.pos);
        }
        ui.weak(RichText::new("Astuce : la porte est un trou de 2 blocs, ajoute ta porte et tes décorations en jeu.").small());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocs() {
        let mut img = RgbaImage::new(4, 2);
        img.put_pixel(0, 0, image::Rgba([240, 118, 20, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
        let c = pixel_art(&img, 4, &[1], false, false);
        assert_eq!((c.l, c.h, c.p), (4, 2, 1));
        assert_eq!(c.noms[c.lire(0, 1, 0) as usize], "orange_wool"); // en haut à gauche
        assert_eq!(c.lire(1, 1, 0), 0); // transparent = air
        let cmds = c.commandes(Some((100, 64, 0)));
        assert!(cmds[0].starts_with("fill ") && cmds.iter().any(|x| x.contains("100 65 0 100 65 0 minecraft:orange_wool")));
        let s = schem(&c);
        assert_eq!(&s[..2], &[0x1f, 0x8b]);
        for m in [Modele::Maison, Modele::Tour, Modele::Arene, Modele::Pyramide, Modele::Dome, Modele::Pont] {
            let c = structure(&Parametres { modele: m, ..Default::default() });
            assert!(c.comptes().iter().map(|x| x.1).sum::<usize>() > 20, "{m:?}");
        }
        // maison : la porte est vide, les coins sont en rondins
        let m = structure(&Parametres::default());
        assert_eq!(m.lire(4, 1, 5), 0);
        assert_eq!(m.noms[m.lire(0, 1, 0) as usize], "oak_log");
        // fill groupe les suites
        let mut c = Construction::nouvelle(5, 1, 1);
        for x in 0..5 {
            c.mettre(x, 0, 0, "stone");
        }
        assert_eq!(c.commandes(None), vec!["fill ~0 ~0 ~0 ~4 ~0 ~0 minecraft:stone".to_string()]);
    }
}
