//! Mini-jeux, suite : fusion de nombres, blocs qui tombent, 4 en ligne et morpion contre l'ordi,
//! paires, pendu, sudoku, raquettes, oiseau. Affichés comme onglets de l'outil Mini-jeux.
use crate::outil::{charger, enregistrer, hasard};
use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, RichText, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const FOND: Color32 = Color32::from_rgb(20, 14, 28);
const ROSE: Color32 = Color32::from_rgb(255, 95, 162);

fn h(n: usize) -> usize {
    hasard(n as u64) as usize
}

// ===================================================================================================
// Fusion de nombres (glisser, les tuiles identiques s'additionnent)
// ===================================================================================================

pub type Grille4 = [[u32; 4]; 4];

/// Fait glisser une ligne vers la gauche : (ligne, points gagnés).
pub fn glisser_ligne(l: [u32; 4]) -> ([u32; 4], u32) {
    let v: Vec<u32> = l.iter().copied().filter(|x| *x != 0).collect();
    let mut r = [0u32; 4];
    let (mut i, mut k, mut pts) = (0, 0, 0);
    while i < v.len() {
        if i + 1 < v.len() && v[i] == v[i + 1] {
            r[k] = v[i] * 2;
            pts += v[i] * 2;
            i += 2;
        } else {
            r[k] = v[i];
            i += 1;
        }
        k += 1;
    }
    (r, pts)
}

/// dir : 0 gauche, 1 droite, 2 haut, 3 bas. Renvoie (grille, points, a bougé).
pub fn glisser(g: &Grille4, dir: u8) -> (Grille4, u32, bool) {
    let mut n = *g;
    let mut pts = 0;
    for i in 0..4 {
        let ligne: [u32; 4] = match dir {
            0 => g[i],
            1 => [g[i][3], g[i][2], g[i][1], g[i][0]],
            2 => [g[0][i], g[1][i], g[2][i], g[3][i]],
            _ => [g[3][i], g[2][i], g[1][i], g[0][i]],
        };
        let (r, p) = glisser_ligne(ligne);
        pts += p;
        for k in 0..4 {
            match dir {
                0 => n[i][k] = r[k],
                1 => n[i][3 - k] = r[k],
                2 => n[k][i] = r[k],
                _ => n[3 - k][i] = r[k],
            }
        }
    }
    (n, pts, n != *g)
}

pub fn bloquee(g: &Grille4) -> bool {
    (0..4).all(|d| !glisser(g, d).2)
}

struct Fusion {
    g: Grille4,
    score: u32,
}

impl Fusion {
    fn new() -> Self {
        let mut f = Fusion { g: [[0; 4]; 4], score: 0 };
        f.ajouter();
        f.ajouter();
        f
    }
    fn ajouter(&mut self) {
        let vides: Vec<(usize, usize)> = (0..16).map(|k| (k / 4, k % 4)).filter(|(i, j)| self.g[*i][*j] == 0).collect();
        if !vides.is_empty() {
            let (i, j) = vides[h(vides.len())];
            self.g[i][j] = if h(10) == 0 { 4 } else { 2 };
        }
    }
}

// ===================================================================================================
// Blocs qui tombent
// ===================================================================================================

pub const BL: usize = 10;
pub const BH: usize = 20;

/// Les 7 formes (4 cases chacune), en coordonnées dans un carré 4×4.
const FORMES: [[(i32, i32); 4]; 7] = [
    [(0, 1), (1, 1), (2, 1), (3, 1)], // barre
    [(1, 0), (2, 0), (1, 1), (2, 1)], // carré
    [(1, 0), (0, 1), (1, 1), (2, 1)], // T
    [(1, 0), (2, 0), (0, 1), (1, 1)], // S
    [(0, 0), (1, 0), (1, 1), (2, 1)], // Z
    [(0, 0), (0, 1), (1, 1), (2, 1)], // J
    [(2, 0), (0, 1), (1, 1), (2, 1)], // L
];
const COULEURS: [Color32; 7] = [
    Color32::from_rgb(90, 200, 230), Color32::from_rgb(250, 210, 90), Color32::from_rgb(190, 120, 255), Color32::from_rgb(110, 220, 130),
    Color32::from_rgb(255, 100, 110), Color32::from_rgb(110, 150, 255), Color32::from_rgb(255, 160, 80),
];

#[derive(Clone, Copy)]
pub struct Piece {
    pub forme: usize,
    pub rot: u8,
    pub x: i32,
    pub y: i32,
}

impl Piece {
    pub fn cases(&self) -> [(i32, i32); 4] {
        FORMES[self.forme].map(|(mut a, mut b)| {
            if self.forme != 1 {
                for _ in 0..self.rot % 4 {
                    // rotation d'un quart de tour dans le carré (3×3, ou 4×4 pour la barre)
                    let n = if self.forme == 0 { 3 } else { 2 };
                    (a, b) = (n - b, a);
                }
            }
            (a + self.x, b + self.y)
        })
    }
}

pub struct Blocs {
    pub grille: [[u8; BL]; BH], // 0 vide, sinon forme + 1
    pub piece: Piece,
    pub suivante: usize,
    pub score: u32,
    pub lignes: u32,
    pub fini: bool,
    dernier: Instant,
}

impl Blocs {
    pub fn new() -> Self {
        Blocs { grille: [[0; BL]; BH], piece: Piece { forme: h(7), rot: 0, x: 3, y: 0 }, suivante: h(7), score: 0, lignes: 0, fini: false, dernier: Instant::now() }
    }

    pub fn libre(&self, p: &Piece) -> bool {
        p.cases().iter().all(|&(x, y)| x >= 0 && x < BL as i32 && y < BH as i32 && (y < 0 || self.grille[y as usize][x as usize] == 0))
    }

    pub fn essayer(&mut self, dx: i32, dy: i32, rot: u8) -> bool {
        let mut p = self.piece;
        p.x += dx;
        p.y += dy;
        p.rot = (p.rot + rot) % 4;
        // petits décalages si la rotation touche un mur
        for decal in if rot > 0 { &[0, -1, 1, -2, 2][..] } else { &[0][..] } {
            let mut q = p;
            q.x += decal;
            if self.libre(&q) {
                self.piece = q;
                return true;
            }
        }
        false
    }

    /// Pose la pièce, efface les lignes pleines, passe à la suivante.
    pub fn poser(&mut self) {
        for (x, y) in self.piece.cases() {
            if y < 0 {
                self.fini = true;
                return;
            }
            self.grille[y as usize][x as usize] = self.piece.forme as u8 + 1;
        }
        let avant = self.grille.len();
        let mut g: Vec<[u8; BL]> = self.grille.iter().copied().filter(|l| l.iter().any(|c| *c == 0)).collect();
        let n = avant - g.len();
        while g.len() < BH {
            g.insert(0, [0; BL]);
        }
        self.grille.copy_from_slice(&g);
        self.lignes += n as u32;
        self.score += [0, 100, 300, 500, 800][n] * (1 + self.lignes / 10);
        self.piece = Piece { forme: self.suivante, rot: 0, x: 3, y: 0 };
        self.suivante = h(7);
        if !self.libre(&self.piece) {
            self.fini = true;
        }
    }

    pub fn descendre(&mut self) {
        if !self.essayer(0, 1, 0) {
            self.poser();
        }
    }

    fn vitesse(&self) -> Duration {
        Duration::from_millis(600u64.saturating_sub(self.lignes as u64 / 10 * 50).max(80))
    }
}

// ===================================================================================================
// 4 en ligne (contre l'ordinateur)
// ===================================================================================================

pub const C4L: usize = 7;
pub const C4H: usize = 6;
pub type Plateau4 = [[u8; C4L]; C4H]; // 0 vide, 1 toi, 2 ordi ; ligne 0 = haut

pub fn jouer4(p: &mut Plateau4, col: usize, j: u8) -> Option<usize> {
    (0..C4H).rev().find(|&l| p[l][col] == 0).inspect(|&l| p[l][col] = j)
}

pub fn gagnant4(p: &Plateau4) -> Option<u8> {
    for l in 0..C4H {
        for c in 0..C4L {
            let j = p[l][c];
            if j == 0 {
                continue;
            }
            for (dl, dc) in [(0i32, 1i32), (1, 0), (1, 1), (1, -1)] {
                if (1..4).all(|k| {
                    let (ll, cc) = (l as i32 + dl * k, c as i32 + dc * k);
                    ll >= 0 && ll < C4H as i32 && cc >= 0 && cc < C4L as i32 && p[ll as usize][cc as usize] == j
                }) {
                    return Some(j);
                }
            }
        }
    }
    None
}

fn eval4(p: &Plateau4) -> i32 {
    let mut s = 0;
    // le centre vaut plus
    for row in p.iter() {
        s += match row[3] {
            2 => 3,
            1 => -3,
            _ => 0,
        };
    }
    for l in 0..C4H as i32 {
        for c in 0..C4L as i32 {
            for (dl, dc) in [(0, 1), (1, 0), (1, 1), (1, -1)] {
                let fen: Option<Vec<u8>> = (0..4).map(|k| {
                    let (ll, cc) = (l + dl * k, c + dc * k);
                    (ll >= 0 && ll < C4H as i32 && cc >= 0 && cc < C4L as i32).then(|| p[ll as usize][cc as usize])
                }).collect();
                if let Some(f) = fen {
                    let (o, t) = (f.iter().filter(|x| **x == 2).count(), f.iter().filter(|x| **x == 1).count());
                    s += match (o, t) {
                        (3, 0) => 5,
                        (2, 0) => 2,
                        (0, 3) => -4,
                        _ => 0,
                    };
                }
            }
        }
    }
    s
}

fn minimax4(p: &mut Plateau4, prof: u32, mut a: i32, mut b: i32, ordi: bool) -> i32 {
    match gagnant4(p) {
        Some(2) => return 100_000 + prof as i32,
        Some(_) => return -100_000 - prof as i32,
        None => {}
    }
    if prof == 0 || p[0].iter().all(|c| *c != 0) {
        return eval4(p);
    }
    let mut meilleur = if ordi { i32::MIN } else { i32::MAX };
    for c in [3, 2, 4, 1, 5, 0, 6] {
        let Some(l) = jouer4(p, c, if ordi { 2 } else { 1 }) else { continue };
        let v = minimax4(p, prof - 1, a, b, !ordi);
        p[l][c] = 0;
        if ordi {
            meilleur = meilleur.max(v);
            a = a.max(v);
        } else {
            meilleur = meilleur.min(v);
            b = b.min(v);
        }
        if a >= b {
            break;
        }
    }
    meilleur
}

/// Colonne choisie par l'ordinateur (profondeur selon la difficulté).
pub fn coup_ordi4(p: &Plateau4, prof: u32) -> Option<usize> {
    let mut q = *p;
    let mut best = None;
    let mut bv = i32::MIN;
    for c in [3, 2, 4, 1, 5, 0, 6] {
        let Some(l) = jouer4(&mut q, c, 2) else { continue };
        let v = minimax4(&mut q, prof, i32::MIN, i32::MAX, false);
        q[l][c] = 0;
        if v > bv {
            bv = v;
            best = Some(c);
        }
    }
    best
}

// ===================================================================================================
// Morpion (contre l'ordinateur, imbattable en difficile)
// ===================================================================================================

pub fn gagnant3(p: &[u8; 9]) -> Option<u8> {
    const L: [[usize; 3]; 8] = [[0, 1, 2], [3, 4, 5], [6, 7, 8], [0, 3, 6], [1, 4, 7], [2, 5, 8], [0, 4, 8], [2, 4, 6]];
    L.iter().find(|l| p[l[0]] != 0 && p[l[0]] == p[l[1]] && p[l[1]] == p[l[2]]).map(|l| p[l[0]])
}

fn minimax3(p: &mut [u8; 9], ordi: bool, prof: i32) -> i32 {
    match gagnant3(p) {
        Some(2) => return 10 - prof,
        Some(_) => return prof - 10,
        None if p.iter().all(|c| *c != 0) => return 0,
        None => {}
    }
    let mut m = if ordi { -100 } else { 100 };
    for i in 0..9 {
        if p[i] == 0 {
            p[i] = if ordi { 2 } else { 1 };
            let v = minimax3(p, !ordi, prof + 1);
            p[i] = 0;
            m = if ordi { m.max(v) } else { m.min(v) };
        }
    }
    m
}

pub fn coup_ordi3(p: &[u8; 9], parfait: bool) -> Option<usize> {
    let libres: Vec<usize> = (0..9).filter(|i| p[*i] == 0).collect();
    if libres.is_empty() {
        return None;
    }
    if !parfait && h(3) == 0 {
        return Some(libres[h(libres.len())]);
    }
    let mut q = *p;
    libres.into_iter().max_by_key(|&i| {
        q[i] = 2;
        let v = minimax3(&mut q, false, 0);
        q[i] = 0;
        v
    })
}

// ===================================================================================================
// Pendu
// ===================================================================================================

const MOTS: &[&str] = &[
    "ordinateur", "clavier", "souris", "manette", "console", "serveur", "creeper", "diamant", "obsidienne", "pioche", "chateau", "dragon",
    "sakura", "kimono", "samourai", "ninja", "temple", "lanterne", "bambou", "sushi", "ramen", "manga", "volcan", "tsunami",
    "girafe", "elephant", "papillon", "dauphin", "crocodile", "kangourou", "hibou", "renard", "ecureuil", "pingouin", "tortue", "requin",
    "bibliotheque", "chocolat", "croissant", "baguette", "fromage", "confiture", "pamplemousse", "framboise", "citrouille", "ananas",
    "astronaute", "planete", "galaxie", "comete", "telescope", "satellite", "fusee", "meteorite", "eclipse", "nebuleuse",
    "aventure", "mystere", "tresor", "pirate", "boussole", "carte", "montagne", "riviere", "foret", "desert", "glacier", "ocean",
    "guitare", "batterie", "violon", "trompette", "musique", "concert", "melodie", "orchestre", "piano", "micro",
    "parapluie", "escalier", "fenetre", "trampoline", "toboggan", "labyrinthe", "puzzle", "domino", "echecs", "billard",
];

pub fn mot_affiche(mot: &str, essais: &[char]) -> String {
    mot.chars().map(|c| if essais.contains(&c) { c.to_ascii_uppercase() } else { '_' }).map(|c| c.to_string()).collect::<Vec<_>>().join(" ")
}

pub fn erreurs_pendu(mot: &str, essais: &[char]) -> usize {
    essais.iter().filter(|c| !mot.contains(**c)).count()
}

// ===================================================================================================
// Sudoku
// ===================================================================================================

pub type Sudoku = [u8; 81];

pub(crate) fn possible(g: &Sudoku, i: usize, v: u8) -> bool {
    let (l, c) = (i / 9, i % 9);
    let (bl, bc) = (l / 3 * 3, c / 3 * 3);
    (0..9).all(|k| g[l * 9 + k] != v && g[k * 9 + c] != v && g[(bl + k / 3) * 9 + bc + k % 3] != v)
}

/// Compte les solutions (s'arrête à `max`). `ordre` : ordre d'essai des chiffres.
pub fn resoudre(g: &mut Sudoku, max: u32, ordre: &[u8; 9]) -> u32 {
    let Some(i) = g.iter().position(|v| *v == 0) else { return 1 };
    let mut n = 0;
    for &v in ordre {
        if possible(g, i, v) {
            g[i] = v;
            n += resoudre(g, max - n, ordre);
            if n >= max {
                return n; // on garde la grille remplie (utile pour générer)
            }
            g[i] = 0;
        }
    }
    n
}

/// (grille à trous, solution). `indices` : nombre de cases laissées (≈ 30 difficile, 40 facile).
pub fn generer_sudoku(indices: usize) -> (Sudoku, Sudoku) {
    generer_sudoku_avec(indices, &mut h)
}

/// Même chose avec un tirage donné (grille du jour : même graine → même grille).
pub fn generer_sudoku_avec(indices: usize, h: &mut impl FnMut(usize) -> usize) -> (Sudoku, Sudoku) {
    let mut ordre = [1, 2, 3, 4, 5, 6, 7, 8, 9];
    for i in (1..9).rev() {
        ordre.swap(i, h(i + 1));
    }
    let mut sol = [0u8; 81];
    resoudre(&mut sol, 1, &ordre);
    let mut g = sol;
    let mut cases: Vec<usize> = (0..81).collect();
    for i in (1..81).rev() {
        cases.swap(i, h(i + 1));
    }
    let mut restantes = 81;
    for i in cases {
        if restantes <= indices {
            break;
        }
        let v = g[i];
        g[i] = 0;
        let mut t = g;
        if resoudre(&mut t, 2, &[1, 2, 3, 4, 5, 6, 7, 8, 9]) != 1 {
            g[i] = v; // la solution ne serait plus unique
        } else {
            restantes -= 1;
        }
    }
    (g, sol)
}

// ===================================================================================================
// Raquettes et oiseau
// ===================================================================================================

const PL: f32 = 640.0;
const PH: f32 = 400.0;

struct Raquettes {
    toi: f32,
    ordi: f32,
    balle: Pos2,
    v: Vec2,
    score: (u32, u32),
}

impl Raquettes {
    fn new() -> Self {
        let mut r = Raquettes { toi: PH / 2.0, ordi: PH / 2.0, balle: Pos2::ZERO, v: Vec2::ZERO, score: (0, 0) };
        r.servir(true);
        r
    }
    fn servir(&mut self, vers_toi: bool) {
        self.balle = Pos2::new(PL / 2.0, PH / 2.0);
        let a = (h(60) as f32 - 30.0).to_radians();
        let s = if vers_toi { -1.0 } else { 1.0 };
        self.v = Vec2::new(s * 300.0 * a.cos(), 300.0 * a.sin());
    }
    fn avancer(&mut self, dt: f32) {
        self.balle += self.v * dt;
        if self.balle.y < 6.0 || self.balle.y > PH - 6.0 {
            self.v.y = -self.v.y;
            self.balle.y = self.balle.y.clamp(6.0, PH - 6.0);
        }
        // l'ordi suit la balle, un peu moins vite qu'elle
        let cible = if self.v.x > 0.0 { self.balle.y } else { PH / 2.0 };
        self.ordi += (cible - self.ordi).clamp(-260.0 * dt, 260.0 * dt);
        for (x, y, sens) in [(24.0, self.toi, 1.0f32), (PL - 24.0, self.ordi, -1.0)] {
            if (self.balle.x - x).abs() < 10.0 && (self.balle.y - y).abs() < 45.0 && self.v.x * sens < 0.0 {
                let decal = (self.balle.y - y) / 45.0;
                let vit = (self.v.length() * 1.05).min(900.0);
                self.v = Vec2::new(sens * vit * (1.0 - decal.abs() * 0.4), vit * decal * 0.8);
            }
        }
        if self.balle.x < 0.0 {
            self.score.1 += 1;
            self.servir(false);
        } else if self.balle.x > PL {
            self.score.0 += 1;
            self.servir(true);
        }
    }
}

struct Oiseau {
    y: f32,
    v: f32,
    tuyaux: Vec<(f32, f32, bool)>, // x, centre du trou, compté
    score: u32,
    fini: bool,
    parti: bool,
}

impl Oiseau {
    fn new() -> Self {
        Oiseau { y: PH / 2.0, v: 0.0, tuyaux: vec![], score: 0, fini: false, parti: false }
    }
    fn avancer(&mut self, dt: f32) {
        if !self.parti || self.fini {
            return;
        }
        self.v += 1300.0 * dt;
        self.y += self.v * dt;
        if self.tuyaux.last().is_none_or(|t| t.0 < PL - 220.0) {
            self.tuyaux.push((PL + 40.0, 90.0 + h((PH - 180.0) as usize) as f32, false));
        }
        for t in &mut self.tuyaux {
            t.0 -= 170.0 * dt;
            if !t.2 && t.0 < 120.0 {
                t.2 = true;
                self.score += 1;
            }
            if (t.0 - 140.0).abs() < 38.0 && (self.y - t.1).abs() > 72.0 {
                self.fini = true;
            }
        }
        self.tuyaux.retain(|t| t.0 > -60.0);
        if self.y > PH - 10.0 || self.y < 0.0 {
            self.fini = true;
        }
    }
}

// ===================================================================================================
// Salle de jeux (onglets)
// ===================================================================================================

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Records {
    fusion: u32,
    blocs: u32,
    paires: Option<u32>,
    oiseau: u32,
    victoires4: u32,
    sudoku: Option<u64>,
}

pub const NOMS: [&str; 9] = ["🔢 Fusion", "🟦 Blocs", "🔴 4 en ligne", "❌ Morpion", "🃏 Paires", "🪢 Pendu", "9️⃣ Sudoku", "🏓 Raquettes", "🐦 Oiseau"];

pub struct Salle {
    fusion: Fusion,
    blocs: Blocs,
    p4: Plateau4,
    p4_fin: Option<u8>,
    p4_prof: u32,
    m3: [u8; 9],
    m3_parfait: bool,
    paires: Vec<(char, bool)>,
    paires_ouvertes: Vec<usize>,
    paires_coups: u32,
    paires_cache: Option<Instant>,
    pendu: (String, Vec<char>),
    sudoku: (Sudoku, Sudoku, Sudoku, Option<usize>, Instant, bool), // départ, solution, saisie, case choisie, début, gagné
    raquettes: Raquettes,
    oiseau: Oiseau,
    dernier: Instant,
    records: Records,
}

fn nouvelles_paires() -> Vec<(char, bool)> {
    let mut v: Vec<(char, bool)> = ['🌸', '🐱', '🍣', '🎮', '⛩', '🐉', '🌙', '🍡'].iter().flat_map(|c| [(*c, false), (*c, false)]).collect();
    for i in (1..v.len()).rev() {
        v.swap(i, h(i + 1));
    }
    v
}

fn nouveau_pendu() -> (String, Vec<char>) {
    (MOTS[h(MOTS.len())].to_string(), vec![])
}

fn nouveau_sudoku(indices: usize) -> (Sudoku, Sudoku, Sudoku, Option<usize>, Instant, bool) {
    let (g, s) = generer_sudoku(indices);
    (g, s, g, None, Instant::now(), false)
}

impl Salle {
    pub fn new() -> Self {
        Salle {
            fusion: Fusion::new(), blocs: Blocs::new(), p4: [[0; C4L]; C4H], p4_fin: None, p4_prof: 4, m3: [0; 9], m3_parfait: false,
            paires: nouvelles_paires(), paires_ouvertes: vec![], paires_coups: 0, paires_cache: None, pendu: nouveau_pendu(),
            sudoku: nouveau_sudoku(36), raquettes: Raquettes::new(), oiseau: Oiseau::new(), dernier: Instant::now(), records: charger("minijeux2"),
        }
    }

    fn sauver(&self) {
        enregistrer("minijeux2", &self.records);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, jeu: usize) {
        let dt = self.dernier.elapsed().as_secs_f32().min(0.05);
        self.dernier = Instant::now();
        match jeu {
            0 => self.ui_fusion(ui),
            1 => self.ui_blocs(ui),
            2 => self.ui_4(ui),
            3 => self.ui_morpion(ui),
            4 => self.ui_paires(ui),
            5 => self.ui_pendu(ui),
            6 => self.ui_sudoku(ui),
            7 => self.ui_raquettes(ui, dt),
            _ => self.ui_oiseau(ui, dt),
        }
    }

    fn ui_fusion(&mut self, ui: &mut egui::Ui) {
        let f = &mut self.fusion;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Score {} · record {}", f.score, self.records.fusion)).strong());
            if ui.button("Nouvelle partie").clicked() {
                *f = Fusion::new();
            }
            ui.weak("Flèches ou ZQSD : les tuiles identiques fusionnent. Vise 2048 !");
        });
        let dir = ui.input(|i| {
            [(Key::ArrowLeft, 0), (Key::Q, 0), (Key::ArrowRight, 1), (Key::D, 1), (Key::ArrowUp, 2), (Key::Z, 2), (Key::ArrowDown, 3), (Key::S, 3)]
                .iter().find(|(k, _)| i.key_pressed(*k)).map(|x| x.1)
        });
        if let Some(d) = dir {
            let (g, p, bouge) = glisser(&f.g, d);
            if bouge {
                f.g = g;
                f.score += p;
                f.ajouter();
                if f.score > self.records.fusion {
                    self.records.fusion = f.score;
                    enregistrer("minijeux2", &self.records);
                }
            }
        }
        let c = 96.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 4.0 + 10.0), Sense::hover());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 8.0, FOND);
        for i in 0..4 {
            for j in 0..4 {
                let v = f.g[i][j];
                let r = Rect::from_min_size(zone.min + Vec2::new(5.0 + j as f32 * c, 5.0 + i as f32 * c), Vec2::splat(c)).shrink(5.0);
                let niveau = if v == 0 { 0.0 } else { (v as f32).log2() / 11.0 };
                let col = if v == 0 { Color32::from_rgb(45, 35, 55) } else { Color32::from_rgb(120 + (135.0 * niveau) as u8, (90.0 * (1.0 - niveau) + 60.0) as u8, (200.0 * (1.0 - niveau) + 50.0) as u8) };
                p.rect_filled(r, 8.0, col);
                if v > 0 {
                    p.text(r.center(), Align2::CENTER_CENTER, v.to_string(), FontId::proportional(if v >= 1000 { 26.0 } else { 34.0 }), Color32::WHITE);
                }
            }
        }
        if bloquee(&f.g) {
            p.text(zone.center(), Align2::CENTER_CENTER, "Bloqué !", FontId::proportional(40.0), Color32::WHITE);
        }
    }

    fn ui_blocs(&mut self, ui: &mut egui::Ui) {
        let b = &mut self.blocs;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Score {} · lignes {} · record {}", b.score, b.lignes, self.records.blocs)).strong());
            if ui.button("Nouvelle partie").clicked() {
                *b = Blocs::new();
            }
            ui.weak("← → déplacer · ↑ tourner · ↓ accélérer · Espace : tout en bas");
        });
        if !b.fini {
            ui.input(|i| {
                if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::Q) {
                    b.essayer(-1, 0, 0);
                }
                if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::D) {
                    b.essayer(1, 0, 0);
                }
                if i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::Z) {
                    b.essayer(0, 0, 1);
                }
                if i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::S) {
                    b.descendre();
                }
                if i.key_pressed(Key::Space) {
                    while b.essayer(0, 1, 0) {}
                    b.poser();
                }
            });
            if b.dernier.elapsed() >= b.vitesse() {
                b.descendre();
                b.dernier = Instant::now();
            }
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
        if b.score > self.records.blocs {
            self.records.blocs = b.score;
            enregistrer("minijeux2", &self.records);
        }
        let c = 24.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::new(c * BL as f32 + 140.0, c * BH as f32), Sense::hover());
        let p = ui.painter_at(zone);
        let jeu = Rect::from_min_size(zone.min, Vec2::new(c * BL as f32, c * BH as f32));
        p.rect_filled(jeu, 4.0, FOND);
        let case = |x: i32, y: i32| Rect::from_min_size(jeu.min + Vec2::new(x as f32 * c, y as f32 * c), Vec2::splat(c)).shrink(1.0);
        for (y, l) in b.grille.iter().enumerate() {
            for (x, v) in l.iter().enumerate() {
                if *v > 0 {
                    p.rect_filled(case(x as i32, y as i32), 3.0, COULEURS[*v as usize - 1]);
                }
            }
        }
        if !b.fini {
            // ombre de la pièce
            let mut fant = b.piece;
            while b.libre(&Piece { y: fant.y + 1, ..fant }) {
                fant.y += 1;
            }
            for (x, y) in fant.cases() {
                if y >= 0 {
                    p.rect_stroke(case(x, y), 3.0, egui::Stroke::new(1.0_f32, COULEURS[b.piece.forme]), egui::StrokeKind::Inside);
                }
            }
            for (x, y) in b.piece.cases() {
                if y >= 0 {
                    p.rect_filled(case(x, y), 3.0, COULEURS[b.piece.forme]);
                }
            }
        }
        p.text(jeu.right_top() + Vec2::new(20.0, 10.0), Align2::LEFT_TOP, "Suivante", FontId::proportional(14.0), Color32::GRAY);
        for (x, y) in FORMES[b.suivante] {
            p.rect_filled(Rect::from_min_size(jeu.right_top() + Vec2::new(20.0 + x as f32 * 20.0, 36.0 + y as f32 * 20.0), Vec2::splat(19.0)), 3.0, COULEURS[b.suivante]);
        }
        if b.fini {
            p.text(jeu.center(), Align2::CENTER_CENTER, "Perdu !", FontId::proportional(34.0), Color32::WHITE);
        }
    }

    fn ui_4(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Victoires contre l'ordi : {}", self.records.victoires4)).strong());
            for (n, pr) in [("Facile", 1), ("Moyen", 4), ("Difficile", 6)] {
                if ui.selectable_label(self.p4_prof == pr, n).clicked() {
                    self.p4_prof = pr;
                }
            }
            if ui.button("Nouvelle partie").clicked() {
                self.p4 = [[0; C4L]; C4H];
                self.p4_fin = None;
            }
        });
        ui.weak("Aligne 4 jetons roses (ligne, colonne ou diagonale). Clique sur une colonne.");
        let c = 62.0;
        let (zone, rep) = ui.allocate_exact_size(Vec2::new(c * C4L as f32, c * C4H as f32), Sense::click());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 10.0, Color32::from_rgb(50, 40, 110));
        let survol = rep.hover_pos().map(|q| ((q.x - zone.left()) / c) as usize);
        for l in 0..C4H {
            for col in 0..C4L {
                let ctr = zone.min + Vec2::new((col as f32 + 0.5) * c, (l as f32 + 0.5) * c);
                let couleur = match self.p4[l][col] {
                    1 => ROSE,
                    2 => Color32::from_rgb(250, 210, 90),
                    _ if survol == Some(col) && self.p4_fin.is_none() => Color32::from_rgb(40, 30, 70),
                    _ => FOND,
                };
                p.circle_filled(ctr, c * 0.4, couleur);
            }
        }
        if rep.clicked() && self.p4_fin.is_none() {
            if let Some(col) = survol.filter(|c| *c < C4L) {
                if jouer4(&mut self.p4, col, 1).is_some() {
                    self.p4_fin = gagnant4(&self.p4);
                    if self.p4_fin == Some(1) {
                        self.records.victoires4 += 1;
                        self.sauver();
                    } else if self.p4_fin.is_none() {
                        if let Some(o) = coup_ordi4(&self.p4, self.p4_prof) {
                            jouer4(&mut self.p4, o, 2);
                            self.p4_fin = gagnant4(&self.p4);
                        }
                    }
                    if self.p4_fin.is_none() && self.p4[0].iter().all(|x| *x != 0) {
                        self.p4_fin = Some(0);
                    }
                }
            }
        }
        if let Some(g) = self.p4_fin {
            ui.label(RichText::new(match g {
                1 => "Tu as gagné ! 🎉",
                2 => "L'ordi a gagné 🤖",
                _ => "Match nul",
            }).size(22.0));
        }
    }

    fn ui_morpion(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.m3_parfait, false, "Normal");
            ui.selectable_value(&mut self.m3_parfait, true, "Imbattable");
            if ui.button("Nouvelle partie").clicked() {
                self.m3 = [0; 9];
            }
        });
        let fin = gagnant3(&self.m3).or(self.m3.iter().all(|c| *c != 0).then_some(0));
        let c = 110.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 3.0), Sense::hover());
        for i in 0..9 {
            let r = Rect::from_min_size(zone.min + Vec2::new((i % 3) as f32 * c, (i / 3) as f32 * c), Vec2::splat(c)).shrink(4.0);
            let rep = ui.interact(r, ui.id().with(("morpion", i)), Sense::click());
            ui.painter().rect_filled(r, 8.0, if rep.hovered() && self.m3[i] == 0 && fin.is_none() { Color32::from_rgb(60, 45, 80) } else { Color32::from_rgb(45, 35, 55) });
            let t = match self.m3[i] {
                1 => ("✖", ROSE),
                2 => ("◯", Color32::from_rgb(250, 210, 90)),
                _ => ("", Color32::WHITE),
            };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, t.0, FontId::proportional(64.0), t.1);
            if rep.clicked() && self.m3[i] == 0 && fin.is_none() {
                self.m3[i] = 1;
                if gagnant3(&self.m3).is_none() {
                    if let Some(o) = coup_ordi3(&self.m3, self.m3_parfait) {
                        self.m3[o] = 2;
                    }
                }
            }
        }
        if let Some(g) = fin {
            ui.label(RichText::new(match g {
                1 => "Tu as gagné ! 🎉",
                2 => "L'ordi a gagné 🤖",
                _ => "Match nul",
            }).size(22.0));
        }
    }

    fn ui_paires(&mut self, ui: &mut egui::Ui) {
        let trouvees = self.paires.iter().filter(|x| x.1).count() / 2;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Paires {trouvees}/8 · coups {}", self.paires_coups)).strong());
            if let Some(r) = self.records.paires {
                ui.weak(format!("record {r} coups"));
            }
            if ui.button("Nouvelle partie").clicked() {
                self.paires = nouvelles_paires();
                self.paires_ouvertes.clear();
                self.paires_coups = 0;
            }
        });
        // deux cartes différentes : on les montre 0,8 s puis on les retourne
        if let Some(t) = self.paires_cache {
            if t.elapsed() > Duration::from_millis(800) {
                self.paires_ouvertes.clear();
                self.paires_cache = None;
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
        let c = 90.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 4.0), Sense::hover());
        let mut clic = None;
        for i in 0..16 {
            let r = Rect::from_min_size(zone.min + Vec2::new((i % 4) as f32 * c, (i / 4) as f32 * c), Vec2::splat(c)).shrink(5.0);
            let rep = ui.interact(r, ui.id().with(("paire", i)), Sense::click());
            let visible = self.paires[i].1 || self.paires_ouvertes.contains(&i);
            ui.painter().rect_filled(r, 10.0, if visible { Color32::from_rgb(60, 45, 80) } else { ROSE });
            if visible {
                ui.painter().text(r.center(), Align2::CENTER_CENTER, self.paires[i].0, FontId::proportional(42.0), Color32::WHITE);
            }
            if rep.clicked() && !visible && self.paires_cache.is_none() {
                clic = Some(i);
            }
        }
        if let Some(i) = clic {
            self.paires_ouvertes.push(i);
            if self.paires_ouvertes.len() == 2 {
                self.paires_coups += 1;
                let (a, b) = (self.paires_ouvertes[0], self.paires_ouvertes[1]);
                if self.paires[a].0 == self.paires[b].0 {
                    self.paires[a].1 = true;
                    self.paires[b].1 = true;
                    self.paires_ouvertes.clear();
                    if self.paires.iter().all(|x| x.1) && self.records.paires.is_none_or(|r| self.paires_coups < r) {
                        self.records.paires = Some(self.paires_coups);
                        self.sauver();
                    }
                } else {
                    self.paires_cache = Some(Instant::now());
                }
            }
        }
        if trouvees == 8 {
            ui.label(RichText::new(format!("Bravo, en {} coups ! 🎉", self.paires_coups)).size(22.0));
        }
    }

    fn ui_pendu(&mut self, ui: &mut egui::Ui) {
        let (mot, essais) = &mut self.pendu;
        let err = erreurs_pendu(mot, essais);
        let gagne = mot.chars().all(|c| essais.contains(&c));
        let perdu = err >= 7;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Erreurs {err}/7")).strong());
            if ui.button("Nouveau mot").clicked() {
                self.pendu = nouveau_pendu();
            }
        });
        let (mot, essais) = &mut self.pendu;
        let (zone, _) = ui.allocate_exact_size(Vec2::new(220.0, 200.0), Sense::hover());
        let p = ui.painter_at(zone);
        let s = egui::Stroke::new(4.0f32, Color32::WHITE);
        let o = zone.min;
        let traits: [(Pos2, Pos2); 4] = [(o + Vec2::new(20.0, 190.0), o + Vec2::new(140.0, 190.0)), (o + Vec2::new(50.0, 190.0), o + Vec2::new(50.0, 15.0)), (o + Vec2::new(50.0, 15.0), o + Vec2::new(150.0, 15.0)), (o + Vec2::new(150.0, 15.0), o + Vec2::new(150.0, 45.0))];
        for t in traits.iter().take(err.min(4)) {
            p.line_segment([t.0, t.1], s);
        }
        if err >= 4 {
            p.circle_stroke(o + Vec2::new(150.0, 62.0), 17.0, s);
        }
        if err >= 5 {
            p.line_segment([o + Vec2::new(150.0, 79.0), o + Vec2::new(150.0, 130.0)], s);
        }
        if err >= 6 {
            p.line_segment([o + Vec2::new(150.0, 95.0), o + Vec2::new(125.0, 115.0)], s);
            p.line_segment([o + Vec2::new(150.0, 95.0), o + Vec2::new(175.0, 115.0)], s);
        }
        if err >= 7 {
            p.line_segment([o + Vec2::new(150.0, 130.0), o + Vec2::new(130.0, 165.0)], s);
            p.line_segment([o + Vec2::new(150.0, 130.0), o + Vec2::new(170.0, 165.0)], s);
        }
        let affiche = if perdu { mot.to_uppercase().chars().map(|c| c.to_string()).collect::<Vec<_>>().join(" ") } else { mot_affiche(mot, essais) };
        ui.label(RichText::new(affiche).size(30.0).monospace());
        if gagne {
            ui.label(RichText::new("Trouvé ! 🎉").size(22.0));
        } else if perdu {
            ui.label(RichText::new("Pendu ! 💀").size(22.0));
        } else {
            let tape = ui.input(|i| i.events.iter().find_map(|e| match e {
                egui::Event::Text(t) => t.chars().next().map(|c| c.to_ascii_lowercase()).filter(|c| c.is_ascii_lowercase()),
                _ => None,
            }));
            ui.horizontal_wrapped(|ui| {
                for c in 'a'..='z' {
                    let deja = essais.contains(&c);
                    if (ui.add_enabled(!deja, egui::Button::new(RichText::new(c.to_ascii_uppercase().to_string()).size(18.0)).min_size(Vec2::splat(32.0))).clicked() || tape == Some(c)) && !deja {
                        essais.push(c);
                    }
                }
            });
            ui.weak("(sans accents : « é » s'écrit « e »)");
        }
    }

    fn ui_sudoku(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (n, k) in [("Facile", 40), ("Moyen", 32), ("Difficile", 27)] {
                if ui.button(n).clicked() {
                    self.sudoku = nouveau_sudoku(k);
                }
            }
            let (_, _, _, _, debut, gagne) = &self.sudoku;
            if !gagne {
                ui.label(format!("⏱ {} s", debut.elapsed().as_secs()));
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
            if let Some(r) = self.records.sudoku {
                ui.weak(format!("record {} min {:02} s", r / 60, r % 60));
            }
        });
        ui.weak("Clique une case puis tape un chiffre (0 ou Suppr pour effacer).");
        let (depart, sol, saisie, choisie, debut, gagne) = &mut self.sudoku;
        let chiffre = ui.input(|i| {
            for (k, v) in [(Key::Num1, 1), (Key::Num2, 2), (Key::Num3, 3), (Key::Num4, 4), (Key::Num5, 5), (Key::Num6, 6), (Key::Num7, 7), (Key::Num8, 8), (Key::Num9, 9), (Key::Num0, 0), (Key::Delete, 0), (Key::Backspace, 0)] {
                if i.key_pressed(k) {
                    return Some(v);
                }
            }
            None
        });
        if let (Some(v), Some(i)) = (chiffre, *choisie) {
            if depart[i] == 0 && !*gagne {
                saisie[i] = v;
                if saisie == sol {
                    *gagne = true;
                    let t = debut.elapsed().as_secs();
                    if self.records.sudoku.is_none_or(|r| t < r) {
                        self.records.sudoku = Some(t);
                        enregistrer("minijeux2", &self.records);
                    }
                }
            }
        }
        let c = 44.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 9.0), Sense::hover());
        let sel = choisie.map(|i| saisie[i]).filter(|v| *v != 0);
        for i in 0..81 {
            let r = Rect::from_min_size(zone.min + Vec2::new((i % 9) as f32 * c, (i / 9) as f32 * c), Vec2::splat(c)).shrink(1.0);
            let rep = ui.interact(r, ui.id().with(("sudoku", i)), Sense::click());
            if rep.clicked() {
                *choisie = Some(i);
            }
            let v = saisie[i];
            let conflit = v != 0 && depart[i] == 0 && {
                let mut g = *saisie;
                g[i] = 0;
                !possible(&g, i, v)
            };
            let fond = if *choisie == Some(i) { Color32::from_rgb(110, 60, 110) } else if sel.is_some() && sel == Some(v) { Color32::from_rgb(70, 50, 90) } else { Color32::from_rgb(40, 32, 52) };
            ui.painter().rect_filled(r, 2.0, fond);
            if v != 0 {
                let col = if conflit { Color32::LIGHT_RED } else if depart[i] != 0 { Color32::WHITE } else { ROSE };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, v.to_string(), FontId::proportional(24.0), col);
            }
        }
        for k in 0..=3 {
            let d = k as f32 * 3.0 * c;
            let s = egui::Stroke::new(2.5f32, Color32::from_rgb(200, 160, 220));
            ui.painter().line_segment([zone.min + Vec2::new(d, 0.0), zone.min + Vec2::new(d, 9.0 * c)], s);
            ui.painter().line_segment([zone.min + Vec2::new(0.0, d), zone.min + Vec2::new(9.0 * c, d)], s);
        }
        if *gagne {
            let t = debut.elapsed().as_secs();
            ui.label(RichText::new(format!("Résolu ! 🎉 ({} min {:02} s)", t / 60, t % 60)).size(22.0));
        }
    }

    fn ui_raquettes(&mut self, ui: &mut egui::Ui, dt: f32) {
        let r = &mut self.raquettes;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Toi {} — {} Ordi", r.score.0, r.score.1)).size(20.0).strong());
            if ui.button("Nouvelle partie").clicked() {
                *r = Raquettes::new();
            }
            ui.weak("La souris déplace ta raquette (à gauche). Premier à 7.");
        });
        let (zone, rep) = ui.allocate_exact_size(Vec2::new(PL, PH), Sense::hover());
        if let Some(q) = rep.hover_pos() {
            r.toi = (q.y - zone.top()).clamp(45.0, PH - 45.0);
        }
        let fini = r.score.0 >= 7 || r.score.1 >= 7;
        if !fini {
            for _ in 0..4 {
                r.avancer(dt / 4.0);
            }
            ui.ctx().request_repaint();
        }
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 6.0, FOND);
        for k in 0..20 {
            p.rect_filled(Rect::from_center_size(zone.min + Vec2::new(PL / 2.0, k as f32 * 20.0 + 10.0), Vec2::new(3.0, 10.0)), 0.0, Color32::from_gray(70));
        }
        p.rect_filled(Rect::from_center_size(zone.min + Vec2::new(24.0, r.toi), Vec2::new(10.0, 90.0)), 4.0, ROSE);
        p.rect_filled(Rect::from_center_size(zone.min + Vec2::new(PL - 24.0, r.ordi), Vec2::new(10.0, 90.0)), 4.0, Color32::from_rgb(250, 210, 90));
        p.circle_filled(zone.min + r.balle.to_vec2(), 7.0, Color32::WHITE);
        if fini {
            p.text(zone.center(), Align2::CENTER_CENTER, if r.score.0 >= 7 { "Gagné ! 🎉" } else { "Perdu !" }, FontId::proportional(36.0), Color32::WHITE);
        }
    }

    fn ui_oiseau(&mut self, ui: &mut egui::Ui, dt: f32) {
        let o = &mut self.oiseau;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Score {} · record {}", o.score, self.records.oiseau)).strong());
            if ui.button("Nouvelle partie").clicked() {
                *o = Oiseau::new();
            }
            ui.weak("Espace ou clic pour battre des ailes");
        });
        let (zone, rep) = ui.allocate_exact_size(Vec2::new(PL, PH), Sense::click());
        if (rep.clicked() || ui.input(|i| i.key_pressed(Key::Space))) && !o.fini {
            o.parti = true;
            o.v = -420.0;
        }
        o.avancer(dt);
        if o.score > self.records.oiseau {
            self.records.oiseau = o.score;
            enregistrer("minijeux2", &self.records);
        }
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 6.0, Color32::from_rgb(40, 25, 60));
        for t in &o.tuyaux {
            let x = zone.left() + t.0;
            p.rect_filled(Rect::from_min_max(Pos2::new(x - 30.0, zone.top()), Pos2::new(x + 30.0, zone.top() + t.1 - 72.0)), 4.0, Color32::from_rgb(110, 200, 130));
            p.rect_filled(Rect::from_min_max(Pos2::new(x - 30.0, zone.top() + t.1 + 72.0), Pos2::new(x + 30.0, zone.bottom())), 4.0, Color32::from_rgb(110, 200, 130));
        }
        let pos = zone.min + Vec2::new(140.0, o.y);
        p.circle_filled(pos, 13.0, ROSE);
        p.circle_filled(pos + Vec2::new(5.0, -4.0), 3.0, Color32::WHITE);
        p.text(zone.center_top() + Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, o.score.to_string(), FontId::proportional(36.0), Color32::WHITE);
        if o.fini {
            p.text(zone.center(), Align2::CENTER_CENTER, "Perdu !", FontId::proportional(36.0), Color32::WHITE);
        } else if !o.parti {
            p.text(zone.center(), Align2::CENTER_CENTER, "Espace pour commencer", FontId::proportional(24.0), Color32::WHITE);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fusion() {
        assert_eq!(glisser_ligne([2, 2, 2, 2]), ([4, 4, 0, 0], 8));
        assert_eq!(glisser_ligne([0, 2, 0, 2]), ([4, 0, 0, 0], 4));
        assert_eq!(glisser_ligne([2, 4, 4, 0]), ([2, 8, 0, 0], 8));
        assert_eq!(glisser_ligne([4, 4, 8, 0]), ([8, 8, 0, 0], 8)); // pas de double fusion
        let g = [[2, 0, 0, 2], [0; 4], [0; 4], [0; 4]];
        assert_eq!(glisser(&g, 1).0[0], [0, 0, 0, 4]);
        assert_eq!(glisser(&g, 3).0[3], [2, 0, 0, 2]);
        assert!(!glisser(&[[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]], 0).2);
        assert!(bloquee(&[[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]]));
    }

    #[test]
    fn blocs() {
        let mut b = Blocs::new();
        for f in 0..7 {
            for r in 0..4 {
                let p = Piece { forme: f, rot: r, x: 3, y: 2 };
                let c = p.cases();
                let mut u: Vec<_> = c.to_vec();
                u.sort();
                u.dedup();
                assert_eq!(u.len(), 4);
                // 4 rotations reviennent au départ
                assert_eq!(Piece { rot: r + 4, ..p }.cases(), c);
            }
        }
        // une ligne pleine s'efface
        b.grille[BH - 1] = [1; BL];
        b.grille[BH - 1][0] = 0;
        b.piece = Piece { forme: 0, rot: 1, x: -2, y: BH as i32 - 4 }; // barre verticale dans la colonne 0
        assert!(b.libre(&b.piece), "{:?}", b.piece.cases());
        b.poser();
        assert_eq!(b.lignes, 1);
        assert_eq!(b.grille[BH - 1].iter().filter(|c| **c != 0).count(), 1);
    }

    #[test]
    fn quatre_et_morpion() {
        let mut p: Plateau4 = [[0; C4L]; C4H];
        for _ in 0..3 {
            jouer4(&mut p, 0, 1);
        }
        assert_eq!(gagnant4(&p), None);
        // l'ordi bloque la colonne 0
        assert_eq!(coup_ordi4(&p, 3), Some(0));
        jouer4(&mut p, 0, 1);
        assert_eq!(gagnant4(&p), Some(1));
        // morpion : l'ordi gagne quand il peut, et bloque sinon
        assert_eq!(coup_ordi3(&[2, 2, 0, 1, 1, 0, 0, 0, 0], true), Some(2));
        assert_eq!(coup_ordi3(&[1, 1, 0, 0, 2, 0, 0, 0, 0], true), Some(2));
        assert_eq!(gagnant3(&[1, 0, 0, 0, 1, 0, 0, 0, 1]), Some(1));
    }

    #[test]
    fn pendu_et_sudoku() {
        assert_eq!(mot_affiche("dragon", &['a', 'o']), "_ _ A _ O _");
        assert_eq!(erreurs_pendu("dragon", &['a', 'z', 'x']), 2);
        assert!(MOTS.iter().all(|m| m.chars().all(|c| c.is_ascii_lowercase())));
        let (g, s) = generer_sudoku(32);
        assert!(s.iter().all(|v| (1..=9).contains(v)));
        assert!(g.iter().zip(&s).all(|(a, b)| *a == 0 || a == b));
        let n = g.iter().filter(|v| **v != 0).count();
        assert!((32..=40).contains(&n), "{n}");
        let mut t = g;
        assert_eq!(resoudre(&mut t, 2, &[1, 2, 3, 4, 5, 6, 7, 8, 9]), 1); // solution unique
        let mut t = g;
        resoudre(&mut t, 1, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(t, s);
    }
}
