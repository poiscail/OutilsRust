//! Fun, suite : jeux de cartes (58), bataille navale (59), snake à deux (60), dames (61),
//! sudoku du jour (62), quiz drapeaux et capitales (63), course de frappe en réseau (64).
use crate::minijeux2::{generer_sudoku_avec, possible, Sudoku};
use crate::minijeux3::Glyphes;
use crate::outil::{charger, enregistrer, hasard, Cat, Outil};
use eframe::egui::{self, Align2, Color32, FontId, Key, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ROSE: Color32 = Color32::from_rgb(255, 95, 162);
const OR: Color32 = Color32::from_rgb(250, 210, 90);
const BLEU: Color32 = Color32::from_rgb(90, 170, 255);

fn h(n: usize) -> usize {
    hasard(n.max(1) as u64) as usize
}

fn melanger<T>(v: &mut [T], r: &mut impl FnMut(usize) -> usize) {
    for i in (1..v.len()).rev() {
        v.swap(i, r(i + 1));
    }
}

// ===================================================================================================
// Lien réseau générique : une ligne de texte par message
// ===================================================================================================

#[derive(Default)]
struct EtatLien {
    connecte: bool,
    autre: String,
    recus: Vec<String>,
    erreur: String,
    attente: bool,
    annule: bool,
    nouveau: Option<TcpStream>,
    generation: u64,
}

pub struct Lien {
    port: u16,
    ip: String,
    pub hote: bool,
    e: Arc<Mutex<EtatLien>>,
    sortie: Option<TcpStream>,
}

impl Lien {
    pub fn new(port: u16) -> Self {
        Lien { port, ip: String::new(), hote: true, e: Arc::default(), sortie: None }
    }

    pub fn connecte(&self) -> bool {
        self.e.lock().unwrap().connecte
    }

    pub fn autre(&self) -> String {
        self.e.lock().unwrap().autre.clone()
    }

    fn brancher(&mut self, s: TcpStream) {
        let _ = s.set_nodelay(true);
        let lecture = s.try_clone();
        self.sortie = Some(s);
        let e = self.e.clone();
        let g = {
            let mut x = e.lock().unwrap();
            x.generation += 1;
            x.connecte = true;
            x.erreur.clear();
            x.recus.clear();
            x.generation
        };
        if let Ok(l) = lecture {
            std::thread::spawn(move || {
                for ligne in std::io::BufReader::new(l).lines() {
                    match ligne {
                        Ok(m) => {
                            let mut x = e.lock().unwrap();
                            if x.generation != g {
                                return;
                            }
                            x.recus.push(m);
                        }
                        Err(_) => break,
                    }
                }
                let mut x = e.lock().unwrap();
                if x.generation == g {
                    x.connecte = false;
                    x.erreur = "L'autre joueur est parti.".into();
                }
            });
        }
    }

    pub fn envoyer(&self, m: &str) {
        if let Some(s) = &self.sortie {
            let mut w: &TcpStream = s;
            let _ = w.write_all(format!("{m}\n").as_bytes());
        }
    }

    pub fn recus(&self) -> Vec<String> {
        std::mem::take(&mut self.e.lock().unwrap().recus)
    }

    pub fn quitter(&mut self) {
        if let Some(s) = self.sortie.take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        let mut x = self.e.lock().unwrap();
        x.generation += 1;
        x.connecte = false;
    }

    /// Écran « Créer une partie / Rejoindre ». Renvoie vrai à l'instant où la connexion s'établit.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let nouveau = self.e.lock().unwrap().nouveau.take();
        if let Some(s) = nouveau {
            self.brancher(s);
            return true;
        }
        let attente = self.e.lock().unwrap().attente;
        let mut fait = false;
        ui.add_enabled_ui(!attente, |ui| {
            ui.horizontal(|ui| {
                if ui.button(RichText::new("🏠 Créer une partie").size(16.0)).clicked() {
                    self.hote = true;
                    let (e, port) = (self.e.clone(), self.port);
                    {
                        let mut x = e.lock().unwrap();
                        x.attente = true;
                        x.annule = false;
                        x.erreur.clear();
                    }
                    std::thread::spawn(move || {
                        let r = std::net::TcpListener::bind(("0.0.0.0", port)).and_then(|l| l.accept());
                        let mut x = e.lock().unwrap();
                        x.attente = false;
                        match r {
                            Ok(_) if x.annule => {}
                            Ok((s, ad)) => {
                                x.autre = ad.ip().to_string();
                                x.nouveau = Some(s);
                            }
                            Err(er) => x.erreur = format!("impossible d'attendre un joueur : {er}"),
                        }
                    });
                }
                ui.weak(format!("Ton adresse : {}", crate::telephone::ip_locale().unwrap_or_default()));
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.ip).hint_text("adresse de l'autre PC (ex. 192.168.1.20)").desired_width(220.0));
                if ui.button("🔗 Rejoindre").clicked() && !self.ip.trim().is_empty() {
                    self.hote = false;
                    let r = format!("{}:{}", self.ip.trim(), self.port).parse().map_err(|_| "adresse invalide".to_string()).and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(5)).map_err(|e| e.to_string()));
                    match r {
                        Ok(s) => {
                            self.e.lock().unwrap().autre = self.ip.trim().to_string();
                            self.brancher(s);
                            fait = true;
                        }
                        Err(e) => self.e.lock().unwrap().erreur = format!("connexion impossible : {e}"),
                    }
                }
            });
        });
        if attente {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("En attente de l'autre joueur… (sur l'autre PC : Rejoindre avec ton adresse)");
                if ui.small_button("Annuler").clicked() {
                    self.e.lock().unwrap().annule = true;
                    let _ = TcpStream::connect_timeout(&format!("127.0.0.1:{}", self.port).parse().unwrap(), Duration::from_secs(1));
                }
            });
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        let e = self.e.lock().unwrap().erreur.clone();
        if !e.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        ui.weak("Les deux PC doivent avoir Outils ouvert, sur le même réseau. Windows peut demander d'autoriser Outils dans le pare-feu (réseaux privés).");
        fait
    }

    /// Bandeau « Connecté à … · Quitter ». Renvoie vrai si on vient de quitter.
    pub fn bandeau(&mut self, ui: &mut egui::Ui, score: Option<(u32, u32)>) -> bool {
        let mut q = false;
        ui.horizontal(|ui| {
            ui.label(format!("🔗 Connecté à {}", self.autre()));
            if let Some((a, b)) = score {
                ui.label(RichText::new(format!("Moi {a} — {b} Lui")).strong());
            }
            if ui.small_button("Quitter").clicked() {
                self.quitter();
                q = true;
            }
        });
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        q
    }
}

// ===================================================================================================
// 58 · Jeux de cartes : bataille et huit américain
// ===================================================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Carte {
    pub v: u8, // 1 = as … 11 valet, 12 dame, 13 roi
    pub c: u8, // 0 ♠ 1 ♥ 2 ♦ 3 ♣
}

const SYMBOLES: [char; 4] = ['♠', '♥', '♦', '♣'];

impl Carte {
    fn rang(&self) -> u8 {
        if self.v == 1 { 14 } else { self.v }
    }
    fn texte(&self) -> String {
        match self.v {
            1 => "A".into(),
            11 => "V".into(),
            12 => "D".into(),
            13 => "R".into(),
            v => v.to_string(),
        }
    }
}

pub fn paquet() -> Vec<Carte> {
    (0..4).flat_map(|c| (1..=13).map(move |v| Carte { v, c })).collect()
}

/// Une manche de bataille. Renvoie (gagnant 0/1, cartes en jeu, nombre de batailles).
/// Si un joueur n'a plus assez de cartes pendant une bataille, il perd la manche avec ce qu'il reste.
pub fn manche(a: &mut VecDeque<Carte>, b: &mut VecDeque<Carte>) -> (usize, Vec<Carte>, u32) {
    let mut table = vec![];
    let mut batailles = 0;
    loop {
        let (x, y) = (a.pop_front(), b.pop_front());
        let (Some(x), Some(y)) = (x, y) else {
            let perd_a = x.is_none();
            table.extend(x.into_iter().chain(y));
            return (if perd_a { 1 } else { 0 }, table, batailles);
        };
        table.extend([x, y]);
        if x.rang() != y.rang() {
            return (if x.rang() > y.rang() { 0 } else { 1 }, table, batailles);
        }
        batailles += 1;
        // une carte face cachée chacun
        match (a.pop_front(), b.pop_front()) {
            (Some(p), Some(q)) => table.extend([p, q]),
            (p, q) => {
                let perd_a = p.is_none();
                table.extend(p.into_iter().chain(q));
                return (if perd_a { 1 } else { 0 }, table, batailles);
            }
        }
    }
}

/// Huit américain : une carte peut se poser sur `dessus` si même couleur (ou celle annoncée), même valeur, ou si c'est un 8.
pub fn jouable(c: Carte, dessus: Carte, annoncee: Option<u8>) -> bool {
    c.v == 8 || c.v == dessus.v || c.c == annoncee.unwrap_or(dessus.c)
}

/// Couleur la plus présente dans la main (pour annoncer après un 8).
pub fn couleur_preferee(main: &[Carte]) -> u8 {
    (0..4u8).max_by_key(|k| (main.iter().filter(|c| c.c == *k && c.v != 8).count(), 3 - *k)).unwrap()
}

/// Coup de l'ordi : garde les 8 pour la fin, joue la couleur dont il a le plus.
pub fn coup_ordi(main: &[Carte], dessus: Carte, annoncee: Option<u8>) -> Option<usize> {
    let pref = couleur_preferee(main);
    let mut v: Vec<usize> = (0..main.len()).filter(|k| jouable(main[*k], dessus, annoncee)).collect();
    v.sort_by_key(|k| {
        let c = main[*k];
        (c.v == 8, c.c != pref, std::cmp::Reverse(c.v))
    });
    v.first().copied()
}

#[derive(PartialEq, Clone, Copy)]
enum JeuCartes {
    Bataille,
    Huit,
}

pub struct Cartes {
    jeu: JeuCartes,
    glyphes: Glyphes,
    // bataille
    tas: [VecDeque<Carte>; 2],
    table: Vec<Carte>,
    dernier: String,
    auto: bool,
    prochain: Instant,
    tours: u32,
    // huit américain
    main: [Vec<Carte>; 2],
    pioche: Vec<Carte>,
    defausse: Vec<Carte>,
    annoncee: Option<u8>,
    choix_couleur: bool,
    a_pioche: bool,
    message: String,
    victoires: (u32, u32),
}

impl Cartes {
    pub fn new() -> Self {
        let mut c = Cartes {
            jeu: JeuCartes::Huit, glyphes: Glyphes::default(), tas: Default::default(), table: vec![], dernier: String::new(), auto: false, prochain: Instant::now(), tours: 0,
            main: Default::default(), pioche: vec![], defausse: vec![], annoncee: None, choix_couleur: false, a_pioche: false, message: String::new(), victoires: (0, 0),
        };
        c.nouvelle_bataille();
        c.nouveau_huit();
        c
    }

    fn nouvelle_bataille(&mut self) {
        let mut p = paquet();
        melanger(&mut p, &mut h);
        self.tas = [p[..26].iter().copied().collect(), p[26..].iter().copied().collect()];
        self.table.clear();
        self.dernier.clear();
        self.tours = 0;
        self.auto = false;
    }

    fn tour_bataille(&mut self) {
        if self.tas[0].is_empty() || self.tas[1].is_empty() {
            self.auto = false;
            return;
        }
        let [a, b] = &mut self.tas;
        let (g, mut table, n) = manche(a, b);
        self.tours += 1;
        self.table = table.clone();
        melanger(&mut table, &mut h);
        self.tas[g].extend(table);
        self.dernier = format!("{}{}", if n > 0 { format!("⚔ Bataille{} ! ", if n > 1 { format!(" ×{n}") } else { String::new() }) } else { String::new() }, if g == 0 { "Tu gagnes le pli" } else { "L'ordi gagne le pli" });
        if self.tours >= 1500 {
            self.auto = false;
        }
    }

    fn nouveau_huit(&mut self) {
        let mut p = paquet();
        melanger(&mut p, &mut h);
        self.main = [p.split_off(p.len() - 7), p.split_off(p.len() - 7)];
        // la première carte retournée n'est pas un 8
        let k = p.iter().rposition(|c| c.v != 8).unwrap();
        self.defausse = vec![p.remove(k)];
        self.pioche = p;
        self.annoncee = None;
        self.choix_couleur = false;
        self.a_pioche = false;
        self.message = "À toi : pose une carte de même couleur ou de même valeur, ou un 8.".into();
    }

    fn piocher(&mut self, j: usize) -> Option<Carte> {
        if self.pioche.is_empty() && self.defausse.len() > 1 {
            let dessus = self.defausse.pop().unwrap();
            self.pioche = std::mem::replace(&mut self.defausse, vec![dessus]);
            melanger(&mut self.pioche, &mut h);
        }
        let c = self.pioche.pop()?;
        self.main[j].push(c);
        Some(c)
    }

    fn poser(&mut self, j: usize, k: usize) {
        let c = self.main[j].remove(k);
        self.defausse.push(c);
        self.annoncee = None;
    }

    fn fin_huit(&mut self) -> bool {
        if self.main[0].is_empty() {
            self.message = "🎉 Tu as gagné !".into();
            self.victoires.0 += 1;
            true
        } else if self.main[1].is_empty() {
            self.message = "L'ordi a gagné…".into();
            self.victoires.1 += 1;
            true
        } else {
            false
        }
    }

    fn jouer_ordi(&mut self) {
        let dessus = *self.defausse.last().unwrap();
        let mut k = coup_ordi(&self.main[1], dessus, self.annoncee);
        if k.is_none() {
            if let Some(c) = self.piocher(1) {
                if jouable(c, dessus, self.annoncee) {
                    k = Some(self.main[1].len() - 1);
                }
            }
        }
        match k {
            Some(k) => {
                let c = self.main[1][k];
                self.poser(1, k);
                if c.v == 8 {
                    let a = couleur_preferee(&self.main[1]);
                    self.annoncee = Some(a);
                    self.message = format!("L'ordi pose un 8 et demande du {}.", SYMBOLES[a as usize]);
                } else {
                    self.message = format!("L'ordi pose {}{}.", c.texte(), SYMBOLES[c.c as usize]);
                }
            }
            None => self.message = "L'ordi pioche et passe.".into(),
        }
        if self.main[1].len() == 1 {
            self.message.push_str(" Il ne lui reste qu'une carte !");
        }
        self.fin_huit();
    }

    fn dessiner_carte(&mut self, ui: &egui::Ui, r: Rect, c: Option<Carte>, surbrillance: bool) {
        let p = ui.painter();
        match c {
            None => {
                p.rect_filled(r, 6.0, Color32::from_rgb(120, 50, 110));
                p.rect_stroke(r.shrink(4.0), 4.0, Stroke::new(1.5_f32, Color32::from_rgb(230, 170, 220)), egui::StrokeKind::Inside);
            }
            Some(c) => {
                p.rect_filled(r, 6.0, Color32::WHITE);
                let rouge = c.c == 1 || c.c == 2;
                let coul = if rouge { [210, 30, 50] } else { [20, 20, 30] };
                p.text(r.min + Vec2::new(6.0, 4.0), Align2::LEFT_TOP, c.texte(), FontId::proportional(r.height() * 0.2), Color32::from_rgb(coul[0], coul[1], coul[2]));
                let s = r.height() * 0.42;
                self.glyphes.dessiner(ui, Rect::from_center_size(r.center() + Vec2::new(0.0, r.height() * 0.08), Vec2::splat(s)), SYMBOLES[c.c as usize], coul, None);
            }
        }
        if surbrillance {
            ui.painter().rect_stroke(r, 6.0, Stroke::new(3.0_f32, OR), egui::StrokeKind::Outside);
        }
    }

    fn ui_bataille(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Toi : {} cartes", self.tas[0].len())).color(ROSE).strong());
            ui.label(RichText::new(format!("Ordi : {} cartes", self.tas[1].len())).color(BLEU).strong());
            ui.weak(format!("pli n° {}", self.tours));
        });
        let fini = self.tas[0].is_empty() || self.tas[1].is_empty() || self.tours >= 1500;
        ui.horizontal(|ui| {
            if ui.add_enabled(!fini, egui::Button::new(RichText::new("🃏 Retourner").size(18.0))).clicked() {
                self.tour_bataille();
            }
            ui.checkbox(&mut self.auto, "Jouer tout seul");
            if ui.button("Nouvelle partie").clicked() {
                self.nouvelle_bataille();
            }
        });
        if self.auto && !fini && Instant::now() >= self.prochain {
            self.tour_bataille();
            self.prochain = Instant::now() + Duration::from_millis(350);
        }
        if self.auto {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let (zone, _) = ui.allocate_exact_size(Vec2::new(520.0, 170.0), Sense::hover());
        ui.painter().rect_filled(zone, 10.0, Color32::from_rgb(25, 80, 50));
        // cartes visibles : les deux dernières face visible, les cachées des batailles entre elles
        let n = self.table.len();
        for (k, c) in self.table.clone().iter().enumerate() {
            let joueur = k % 2;
            let rang = k / 2;
            let visible = rang % 2 == 0 || k + 2 >= n;
            let x = zone.left() + 30.0 + joueur as f32 * 250.0 + rang as f32 * 16.0;
            let r = Rect::from_min_size(egui::pos2(x, zone.top() + 20.0), Vec2::new(90.0, 130.0));
            self.dessiner_carte(ui, r, visible.then_some(*c), false);
        }
        ui.label(RichText::new(&self.dernier).size(17.0));
        if fini && self.tours > 0 {
            ui.label(RichText::new(match self.tas[0].len().cmp(&self.tas[1].len()) {
                std::cmp::Ordering::Greater => "🎉 Tu as gagné la partie !",
                std::cmp::Ordering::Less => "L'ordi a gagné la partie…",
                _ => "Égalité !",
            }).size(20.0).strong());
        }
    }

    fn ui_huit(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Victoires : toi {} — ordi {}", self.victoires.0, self.victoires.1)).strong());
            if ui.button("Nouvelle donne").clicked() {
                self.nouveau_huit();
            }
        });
        let fini = self.main[0].is_empty() || self.main[1].is_empty();
        // main de l'ordi (dos)
        let (zone, _) = ui.allocate_exact_size(Vec2::new(620.0, 92.0), Sense::hover());
        for k in 0..self.main[1].len() {
            let r = Rect::from_min_size(zone.min + Vec2::new(k as f32 * 22.0, 0.0), Vec2::new(60.0, 88.0));
            self.dessiner_carte(ui, r, None, false);
        }
        // pioche et défausse
        let (zone, _) = ui.allocate_exact_size(Vec2::new(620.0, 140.0), Sense::hover());
        let rp = Rect::from_min_size(zone.min + Vec2::new(120.0, 5.0), Vec2::new(90.0, 130.0));
        let rd = Rect::from_min_size(zone.min + Vec2::new(240.0, 5.0), Vec2::new(90.0, 130.0));
        if !self.pioche.is_empty() || self.defausse.len() > 1 {
            self.dessiner_carte(ui, rp, None, false);
        }
        let dessus = *self.defausse.last().unwrap();
        self.dessiner_carte(ui, rd, Some(dessus), false);
        if let Some(a) = self.annoncee {
            self.glyphes.dessiner(ui, Rect::from_min_size(rd.right_top() + Vec2::new(16.0, 30.0), Vec2::splat(50.0)), SYMBOLES[a as usize], if a == 1 || a == 2 { [255, 90, 90] } else { [240, 240, 240] }, None);
        }
        let mon_tour = !fini && !self.choix_couleur;
        let rep = ui.interact(rp, ui.id().with("pioche"), Sense::click());
        if mon_tour && !self.a_pioche && rep.clicked() {
            if self.piocher(0).is_some() {
                self.a_pioche = true;
                self.message = "Tu as pioché. Pose une carte si tu peux, sinon passe.".into();
            }
        }
        ui.label(RichText::new(&self.message).size(16.0));
        // ma main
        self.main[0].sort_by_key(|c| (c.c, c.v));
        let n = self.main[0].len();
        let ecart = if n > 9 { 560.0 / n as f32 } else { 64.0 };
        let (zone, _) = ui.allocate_exact_size(Vec2::new(640.0, 140.0), Sense::hover());
        let mut jouee = None;
        for k in 0..n {
            let c = self.main[0][k];
            let ok = mon_tour && jouable(c, dessus, self.annoncee);
            let r = Rect::from_min_size(zone.min + Vec2::new(k as f32 * ecart, if ok { 0.0 } else { 10.0 }), Vec2::new(90.0, 130.0));
            self.dessiner_carte(ui, r, Some(c), ok);
            let vis = if k + 1 < n { Rect::from_min_size(r.min, Vec2::new(ecart, r.height())) } else { r };
            if ui.interact(vis, ui.id().with(("main", k)), Sense::click()).clicked() && ok {
                jouee = Some(k);
            }
        }
        if let Some(k) = jouee {
            let c = self.main[0][k];
            self.poser(0, k);
            self.a_pioche = false;
            if !self.fin_huit() {
                if c.v == 8 && !self.main[0].is_empty() {
                    self.choix_couleur = true;
                } else {
                    self.jouer_ordi();
                }
            }
        }
        if self.choix_couleur {
            ui.horizontal(|ui| {
                ui.label("Tu demandes :");
                for k in 0..4u8 {
                    if ui.button(RichText::new(SYMBOLES[k as usize].to_string()).size(24.0).color(if k == 1 || k == 2 { Color32::from_rgb(230, 60, 70) } else { Color32::WHITE })).clicked() {
                        self.annoncee = Some(k);
                        self.choix_couleur = false;
                        self.jouer_ordi();
                    }
                }
            });
        }
        if mon_tour && self.a_pioche && ui.button("Passer").clicked() {
            self.a_pioche = false;
            self.jouer_ordi();
        }
        if mon_tour && !self.a_pioche && self.pioche.is_empty() && self.defausse.len() <= 1 && !self.main[0].iter().any(|c| jouable(*c, dessus, self.annoncee)) && ui.button("Passer (plus de pioche)").clicked() {
            self.jouer_ordi();
        }
        ui.weak("Règles : pose une carte de la même couleur ou de la même valeur. Le 8 se pose sur tout et te laisse choisir la couleur. Clique la pioche si tu ne peux pas jouer. Le premier sans carte gagne.");
    }
}

impl Outil for Cartes {
    fn nom(&self) -> &'static str {
        "🃏 Jeux de cartes"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.jeu, JeuCartes::Huit, RichText::new("🎱 Huit américain").size(16.0));
            ui.selectable_value(&mut self.jeu, JeuCartes::Bataille, RichText::new("⚔ Bataille").size(16.0));
        });
        ui.separator();
        match self.jeu {
            JeuCartes::Bataille => self.ui_bataille(ui),
            JeuCartes::Huit => self.ui_huit(ui),
        }
    }
}

// ===================================================================================================
// 59 · Bataille navale (contre l'ordi ou en réseau)
// ===================================================================================================

pub const BATEAUX: [usize; 5] = [5, 4, 3, 3, 2];
const PORT_NAVALE: u16 = 47_321;

/// Place la flotte au hasard (bateaux qui ne se touchent pas, même en diagonale).
pub fn placer_flotte(r: &mut impl FnMut(usize) -> usize) -> Vec<Vec<usize>> {
    'recommencer: loop {
        let mut occupe = [false; 100];
        let mut flotte = vec![];
        for &n in &BATEAUX {
            let mut essais = 0;
            loop {
                essais += 1;
                if essais > 500 {
                    continue 'recommencer;
                }
                let horiz = r(2) == 0;
                let (l, c) = if horiz { (r(10), r(11 - n)) } else { (r(11 - n), r(10)) };
                let cases: Vec<usize> = (0..n).map(|k| if horiz { l * 10 + c + k } else { (l + k) * 10 + c }).collect();
                let libre = cases.iter().all(|&x| {
                    let (xl, xc) = ((x / 10) as i32, (x % 10) as i32);
                    (-1..=1).all(|dl| (-1..=1).all(|dc| {
                        let (yl, yc) = (xl + dl, xc + dc);
                        !(0..10).contains(&yl) || !(0..10).contains(&yc) || !occupe[(yl * 10 + yc) as usize]
                    }))
                });
                if libre {
                    for &x in &cases {
                        occupe[x] = true;
                    }
                    flotte.push(cases);
                    break;
                }
            }
        }
        return flotte;
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Tir {
    Eau,
    Touche,
    Coule(Vec<usize>),
}

pub fn tirer(flotte: &[Vec<usize>], touches: &mut [bool; 100], k: usize) -> Tir {
    match flotte.iter().find(|b| b.contains(&k)) {
        None => Tir::Eau,
        Some(b) => {
            touches[k] = true;
            if b.iter().all(|x| touches[*x]) { Tir::Coule(b.clone()) } else { Tir::Touche }
        }
    }
}

pub fn tout_coule(flotte: &[Vec<usize>], touches: &[bool; 100]) -> bool {
    flotte.iter().flatten().all(|x| touches[*x])
}

/// Ce que je sais de la grille adverse : 0 inconnu, 1 eau, 2 touché, 3 coulé.
pub type Radar = [u8; 100];

/// Case visée par l'ordi : finit les bateaux touchés, sinon tire en damier.
pub fn cible_ordi(radar: &Radar, r: &mut impl FnMut(usize) -> usize) -> usize {
    let voisin = |k: usize, dl: i32, dc: i32| -> Option<usize> {
        let (l, c) = ((k / 10) as i32 + dl, (k % 10) as i32 + dc);
        ((0..10).contains(&l) && (0..10).contains(&c)).then_some((l * 10 + c) as usize)
    };
    let touches: Vec<usize> = (0..100).filter(|k| radar[*k] == 2).collect();
    if !touches.is_empty() {
        // deux touchés alignés : on prolonge la ligne
        for &a in &touches {
            for (dl, dc) in [(0, 1), (1, 0)] {
                if voisin(a, dl, dc).is_some_and(|b| radar[b] == 2) {
                    for s in [1, -1] {
                        let mut x = a;
                        while let Some(y) = voisin(x, dl * s, dc * s) {
                            match radar[y] {
                                2 => x = y,
                                0 => return y,
                                _ => break,
                            }
                        }
                    }
                }
            }
        }
        let mut v: Vec<usize> = touches.iter().flat_map(|&a| [(0, 1), (1, 0), (0, -1), (-1, 0)].into_iter().filter_map(move |(dl, dc)| voisin(a, dl, dc))).filter(|b| radar[*b] == 0).collect();
        v.dedup();
        if !v.is_empty() {
            return v[r(v.len())];
        }
    }
    // pas à côté d'un bateau coulé (ils ne se touchent pas)
    let pres_coule = |k: usize| (-1..=1).any(|dl| (-1..=1).any(|dc| voisin(k, dl, dc).is_some_and(|b| radar[b] == 3)));
    let damier: Vec<usize> = (0..100).filter(|k| radar[*k] == 0 && (k / 10 + k % 10) % 2 == 0 && !pres_coule(*k)).collect();
    let libres: Vec<usize> = (0..100).filter(|k| radar[*k] == 0 && !pres_coule(*k)).collect();
    let tout: Vec<usize> = (0..100).filter(|k| radar[*k] == 0).collect();
    let v = if !damier.is_empty() { damier } else if !libres.is_empty() { libres } else { tout };
    v[r(v.len())]
}

fn marquer(radar: &mut Radar, k: usize, t: &Tir) {
    match t {
        Tir::Eau => radar[k] = 1,
        Tir::Touche => radar[k] = 2,
        Tir::Coule(b) => {
            for x in b {
                radar[*x] = 3;
            }
        }
    }
}

pub struct Navale {
    reseau: bool,
    lien: Lien,
    flotte: Vec<Vec<usize>>,
    touches_moi: [bool; 100],
    tirs_recus: [bool; 100],
    flotte_ordi: Vec<Vec<usize>>,
    touches_ordi: [bool; 100],
    radar: Radar,
    radar_ordi: Radar,
    mon_tour: bool,
    attente_reponse: bool,
    commence: bool,
    fin: Option<bool>, // Some(true) = gagné
    message: String,
    score: (u32, u32),
}

impl Navale {
    pub fn new() -> Self {
        let mut n = Navale {
            reseau: false, lien: Lien::new(PORT_NAVALE), flotte: vec![], touches_moi: [false; 100], tirs_recus: [false; 100], flotte_ordi: vec![], touches_ordi: [false; 100],
            radar: [0; 100], radar_ordi: [0; 100], mon_tour: true, attente_reponse: false, commence: false, fin: None, message: String::new(), score: (0, 0),
        };
        n.nouvelle(true);
        n
    }

    fn nouvelle(&mut self, je_commence: bool) {
        self.flotte = placer_flotte(&mut h);
        self.flotte_ordi = placer_flotte(&mut h);
        self.touches_moi = [false; 100];
        self.tirs_recus = [false; 100];
        self.touches_ordi = [false; 100];
        self.radar = [0; 100];
        self.radar_ordi = [0; 100];
        self.mon_tour = je_commence;
        self.attente_reponse = false;
        self.commence = false;
        self.fin = None;
        self.message = if je_commence { "À toi de tirer : clique sur la grille de droite.".into() } else { "L'autre commence…".into() };
    }

    fn texte_tir(t: &Tir, qui: &str) -> String {
        match t {
            Tir::Eau => format!("{qui} : à l'eau."),
            Tir::Touche => format!("{qui} : touché !"),
            Tir::Coule(b) => format!("{qui} : touché, coulé ! (bateau de {})", b.len()),
        }
    }

    fn tir_ordi(&mut self) {
        let k = cible_ordi(&self.radar_ordi, &mut h);
        self.tirs_recus[k] = true;
        let t = tirer(&self.flotte, &mut self.touches_moi, k);
        marquer(&mut self.radar_ordi, k, &t);
        self.message.push(' ');
        self.message.push_str(&Self::texte_tir(&t, "L'ordi"));
        if tout_coule(&self.flotte, &self.touches_moi) {
            self.fin = Some(false);
            self.score.1 += 1;
        }
    }

    fn grille(&self, ui: &mut egui::Ui, id: &str, moi: bool, cliquable: bool) -> Option<usize> {
        let c = 30.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 10.0 + 2.0), Sense::hover());
        ui.painter().rect_filled(zone, 4.0, Color32::from_rgb(20, 50, 90));
        let mut clic = None;
        for k in 0..100 {
            let r = Rect::from_min_size(zone.min + Vec2::new((k % 10) as f32 * c + 1.0, (k / 10) as f32 * c + 1.0), Vec2::splat(c)).shrink(1.0);
            let p = ui.painter();
            if moi {
                let bateau = self.flotte.iter().any(|b| b.contains(&k));
                if bateau {
                    p.rect_filled(r, 3.0, if self.touches_moi[k] { Color32::from_rgb(200, 60, 50) } else { Color32::from_gray(150) });
                } else {
                    p.rect_filled(r, 2.0, Color32::from_rgb(30, 70, 120));
                    if self.tirs_recus[k] {
                        p.circle_filled(r.center(), 4.0, Color32::from_gray(200));
                    }
                }
            } else {
                let v = self.radar[k];
                let rep = ui.interact(r, ui.id().with((id, k)), Sense::click());
                let survol = cliquable && v == 0 && rep.hovered();
                ui.painter().rect_filled(r, 2.0, match v {
                    1 => Color32::from_rgb(30, 70, 120),
                    2 => Color32::from_rgb(230, 120, 40),
                    3 => Color32::from_rgb(170, 30, 40),
                    _ if survol => Color32::from_rgb(80, 130, 190),
                    _ => Color32::from_rgb(40, 90, 150),
                });
                if v == 1 {
                    ui.painter().circle_filled(r.center(), 4.0, Color32::from_gray(200));
                }
                if v >= 2 {
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, "✖", FontId::proportional(18.0), Color32::WHITE);
                }
                if cliquable && v == 0 && rep.clicked() {
                    clic = Some(k);
                }
            }
        }
        clic
    }

    fn messages_reseau(&mut self) {
        for m in self.lien.recus() {
            let mut p = m.split_whitespace();
            match p.next() {
                Some("NOUVEAU") => self.nouvelle(self.lien.hote),
                Some("TIR") => {
                    let Some(k) = p.next().and_then(|x| x.parse::<usize>().ok()).filter(|k| *k < 100) else { continue };
                    self.tirs_recus[k] = true;
                    let t = tirer(&self.flotte, &mut self.touches_moi, k);
                    let rep = match &t {
                        Tir::Eau => format!("RES {k} 0"),
                        Tir::Touche => format!("RES {k} 1"),
                        Tir::Coule(b) => format!("RES {k} 2 {}", b.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")),
                    };
                    self.lien.envoyer(&rep);
                    self.message = Self::texte_tir(&t, "L'autre");
                    if tout_coule(&self.flotte, &self.touches_moi) {
                        self.lien.envoyer("PERDU");
                        self.fin = Some(false);
                        self.score.1 += 1;
                    } else {
                        self.mon_tour = true;
                    }
                }
                Some("RES") => {
                    let (Some(k), Some(r)) = (p.next().and_then(|x| x.parse::<usize>().ok()).filter(|k| *k < 100), p.next()) else { continue };
                    let t = match r {
                        "1" => Tir::Touche,
                        "2" => Tir::Coule(p.next().unwrap_or("").split(',').filter_map(|x| x.parse().ok()).filter(|x| *x < 100).collect()),
                        _ => Tir::Eau,
                    };
                    marquer(&mut self.radar, k, &t);
                    self.message = Self::texte_tir(&t, "Toi");
                    self.attente_reponse = false;
                }
                Some("PERDU") => {
                    self.fin = Some(true);
                    self.score.0 += 1;
                }
                _ => {}
            }
        }
    }
}

impl Outil for Navale {
    fn nom(&self) -> &'static str {
        "🚢 Bataille navale"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.selectable_label(!self.reseau, RichText::new("🤖 Contre l'ordi").size(16.0)).clicked() && self.reseau {
                self.reseau = false;
                self.lien.quitter();
                self.nouvelle(true);
            }
            if ui.selectable_label(self.reseau, RichText::new("🌐 En réseau local").size(16.0)).clicked() {
                self.reseau = true;
            }
        });
        if self.reseau {
            if !self.lien.connecte() {
                if self.lien.ui(ui) {
                    self.nouvelle(self.lien.hote);
                }
                return;
            }
            if self.lien.bandeau(ui, Some(self.score)) {
                return;
            }
            self.messages_reseau();
        } else {
            ui.label(RichText::new(format!("Toi {} — {} Ordi", self.score.0, self.score.1)).strong());
        }
        let en_jeu = self.fin.is_none();
        ui.horizontal(|ui| {
            if !self.commence && en_jeu && ui.button("🎲 Replacer mes bateaux").clicked() {
                self.flotte = placer_flotte(&mut h);
            }
            if (!en_jeu || !self.reseau) && ui.button("Nouvelle partie").clicked() {
                self.nouvelle(!self.reseau || self.lien.hote);
                if self.reseau {
                    self.lien.envoyer("NOUVEAU");
                }
            }
        });
        let peut_tirer = en_jeu && self.mon_tour && !self.attente_reponse;
        let mut tir = None;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Ma flotte");
                self.grille(ui, "moi", true, false);
            });
            ui.add_space(20.0);
            ui.vertical(|ui| {
                ui.label(if peut_tirer { "Chez l'adversaire — à toi de tirer !" } else { "Chez l'adversaire" });
                tir = self.grille(ui, "lui", false, peut_tirer);
            });
        });
        if let Some(k) = tir {
            self.commence = true;
            if self.reseau {
                self.lien.envoyer(&format!("TIR {k}"));
                self.attente_reponse = true;
                self.mon_tour = false;
                self.message = "Tir envoyé…".into();
            } else {
                let t = tirer(&self.flotte_ordi, &mut self.touches_ordi, k);
                marquer(&mut self.radar, k, &t);
                self.message = Self::texte_tir(&t, "Toi");
                if tout_coule(&self.flotte_ordi, &self.touches_ordi) {
                    self.fin = Some(true);
                    self.score.0 += 1;
                } else {
                    self.tir_ordi();
                }
            }
        }
        if !self.mon_tour && en_jeu && self.reseau && !self.attente_reponse {
            self.message = "Au tour de l'autre…".into();
        }
        ui.label(RichText::new(match self.fin {
            Some(true) => "🎉 Toute la flotte adverse est coulée : tu as gagné !".to_string(),
            Some(false) => "💥 Ta flotte est coulée… perdu !".to_string(),
            None => self.message.clone(),
        }).size(17.0));
        if self.fin == Some(false) && !self.reseau {
            // montrer les bateaux restants de l'ordi
            let restants: usize = self.flotte_ordi.iter().filter(|b| !b.iter().all(|x| self.touches_ordi[*x])).count();
            ui.weak(format!("Il restait {restants} bateau(x) à l'ordi."));
        }
        ui.weak("Flotte : un porte-avions (5), un croiseur (4), deux sous-marins (3) et un torpilleur (2). Les bateaux ne se touchent pas.");
    }
}

// ===================================================================================================
// 60 · Snake à deux (même clavier ou en réseau)
// ===================================================================================================

pub const SL: i32 = 36;
pub const SH: i32 = 24;
const PORT_SNAKE: u16 = 47_322;

#[derive(Clone, PartialEq, Debug)]
pub struct Arene {
    pub corps: [VecDeque<(i32, i32)>; 2],
    pub dir: [(i32, i32); 2],
    pub voulu: [(i32, i32); 2],
    pub vivant: [bool; 2],
    pub score: [u32; 2],
    pub pomme: (i32, i32),
}

impl Arene {
    pub fn new(r: &mut impl FnMut(usize) -> usize) -> Self {
        let a: VecDeque<(i32, i32)> = (0..4).map(|k| (6 - k, SH / 2 - 4)).collect();
        let b: VecDeque<(i32, i32)> = (0..4).map(|k| (SL - 7 + k, SH / 2 + 3)).collect();
        let mut x = Arene { corps: [a, b], dir: [(1, 0), (-1, 0)], voulu: [(1, 0), (-1, 0)], vivant: [true; 2], score: [0; 2], pomme: (0, 0) };
        x.nouvelle_pomme(r);
        x
    }

    fn nouvelle_pomme(&mut self, r: &mut impl FnMut(usize) -> usize) {
        let libres: Vec<(i32, i32)> = (0..SL * SH).map(|k| (k % SL, k / SL)).filter(|p| !self.corps.iter().any(|c| c.contains(p))).collect();
        if !libres.is_empty() {
            self.pomme = libres[r(libres.len())];
        }
    }

    pub fn tourner(&mut self, j: usize, d: (i32, i32)) {
        self.voulu[j] = d;
    }

    pub fn fini(&self) -> bool {
        !self.vivant[0] || !self.vivant[1]
    }

    /// Un pas de jeu.
    pub fn pas(&mut self, r: &mut impl FnMut(usize) -> usize) {
        if self.fini() {
            return;
        }
        for j in 0..2 {
            let v = self.voulu[j];
            if (v.0 + self.dir[j].0, v.1 + self.dir[j].1) != (0, 0) {
                self.dir[j] = v;
            }
        }
        let tetes: Vec<(i32, i32)> = (0..2).map(|j| {
            let t = self.corps[j][0];
            (t.0 + self.dir[j].0, t.1 + self.dir[j].1)
        }).collect();
        let mange: Vec<bool> = tetes.iter().map(|t| *t == self.pomme).collect();
        // les queues avancent (sauf si le serpent mange)
        for j in 0..2 {
            if !mange[j] {
                self.corps[j].pop_back();
            }
        }
        for j in 0..2 {
            let t = tetes[j];
            let mur = t.0 < 0 || t.1 < 0 || t.0 >= SL || t.1 >= SH;
            let corps = self.corps.iter().any(|c| c.contains(&t));
            let face = tetes[0] == tetes[1];
            if mur || corps || face {
                self.vivant[j] = false;
            }
        }
        for j in 0..2 {
            self.corps[j].push_front(tetes[j]);
            if mange[j] {
                self.score[j] += 1;
            }
        }
        if mange.iter().any(|m| *m) {
            self.nouvelle_pomme(r);
        }
    }

    pub fn vers_texte(&self) -> String {
        let c = |j: usize| self.corps[j].iter().map(|(x, y)| format!("{x}.{y}")).collect::<Vec<_>>().join(",");
        format!("S {}.{}|{}:{}:{}|{}:{}:{}", self.pomme.0, self.pomme.1, self.vivant[0] as u8, self.score[0], c(0), self.vivant[1] as u8, self.score[1], c(1))
    }

    pub fn depuis_texte(t: &str) -> Option<Self> {
        let t = t.strip_prefix("S ")?;
        let mut p = t.split('|');
        let point = |s: &str| -> Option<(i32, i32)> {
            let (a, b) = s.split_once('.')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        };
        let pomme = point(p.next()?)?;
        let mut corps: [VecDeque<(i32, i32)>; 2] = Default::default();
        let mut vivant = [true; 2];
        let mut score = [0; 2];
        for j in 0..2 {
            let mut q = p.next()?.splitn(3, ':');
            vivant[j] = q.next()? == "1";
            score[j] = q.next()?.parse().ok()?;
            corps[j] = q.next()?.split(',').map(point).collect::<Option<_>>()?;
        }
        Some(Arene { corps, dir: [(1, 0), (-1, 0)], voulu: [(1, 0), (-1, 0)], vivant, score, pomme })
    }
}

pub struct SnakeDeux {
    reseau: bool,
    lien: Lien,
    a: Arene,
    marche: bool,
    dernier: Instant,
    vitesse: u64,
    victoires: [u32; 2],
    compte: bool,
}

impl SnakeDeux {
    pub fn new() -> Self {
        SnakeDeux { reseau: false, lien: Lien::new(PORT_SNAKE), a: Arene::new(&mut h), marche: false, dernier: Instant::now(), vitesse: 110, victoires: [0; 2], compte: false }
    }

    fn relancer(&mut self) {
        self.a = Arene::new(&mut h);
        self.marche = true;
        self.compte = false;
        self.dernier = Instant::now();
    }
}

fn direction(i: &egui::InputState, fleches: bool, lettres: bool) -> Option<(i32, i32)> {
    let mut d = None;
    if fleches {
        for (k, v) in [(Key::ArrowUp, (0, -1)), (Key::ArrowDown, (0, 1)), (Key::ArrowLeft, (-1, 0)), (Key::ArrowRight, (1, 0))] {
            if i.key_pressed(k) {
                d = Some(v);
            }
        }
    }
    if lettres {
        // ZQSD (AZERTY) et WASD (QWERTY)
        for (k, v) in [(Key::Z, (0, -1)), (Key::W, (0, -1)), (Key::S, (0, 1)), (Key::Q, (-1, 0)), (Key::A, (-1, 0)), (Key::D, (1, 0))] {
            if i.key_pressed(k) {
                d = Some(v);
            }
        }
    }
    d
}

impl Outil for SnakeDeux {
    fn nom(&self) -> &'static str {
        "🐍 Snake à deux"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.selectable_label(!self.reseau, RichText::new("⌨ Même clavier").size(16.0)).clicked() && self.reseau {
                self.reseau = false;
                self.lien.quitter();
                self.marche = false;
            }
            if ui.selectable_label(self.reseau, RichText::new("🌐 En réseau local").size(16.0)).clicked() {
                self.reseau = true;
                self.marche = false;
            }
        });
        let client = self.reseau && !self.lien.hote;
        if self.reseau {
            if !self.lien.connecte() {
                if self.lien.ui(ui) {
                    self.a = Arene::new(&mut h);
                    self.marche = false;
                }
                return;
            }
            if self.lien.bandeau(ui, Some((self.victoires[if client { 1 } else { 0 }], self.victoires[if client { 0 } else { 1 }]))) {
                return;
            }
            for m in self.lien.recus() {
                if client {
                    if let Some(a) = Arene::depuis_texte(&m) {
                        if a.fini() && !self.a.fini() {
                            for j in 0..2 {
                                if a.vivant[j] && !a.vivant[1 - j] {
                                    self.victoires[j] += 1;
                                }
                            }
                        }
                        self.a = a;
                    }
                } else if let Some(d) = m.strip_prefix("D ") {
                    let v: Vec<i32> = d.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    if v.len() == 2 && v[0].abs() + v[1].abs() == 1 {
                        self.a.tourner(1, (v[0], v[1]));
                    }
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("🟪 {}", self.a.score[0])).color(ROSE).size(18.0).strong());
            ui.label(RichText::new(format!("🟨 {}", self.a.score[1])).color(OR).size(18.0).strong());
            if !self.reseau {
                ui.weak(format!("manches : {} — {}", self.victoires[0], self.victoires[1]));
            }
            if !client && ui.button(if self.marche && !self.a.fini() { "Recommencer" } else { "▶ Jouer" }).clicked() {
                self.relancer();
            }
            if !client {
                ui.add(egui::Slider::new(&mut self.vitesse, 60..=200).text("ms par pas").integer());
            }
        });
        ui.weak(if self.reseau {
            if client { "Tu es le serpent jaune : flèches ou ZQSD." } else { "Tu es le serpent rose : flèches ou ZQSD." }
        } else {
            "Rose : ZQSD (ou WASD) · Jaune : flèches. Mange les pommes, évite les murs et l'autre serpent."
        });
        // commandes
        let (d1, d2) = ui.input(|i| if self.reseau { (direction(i, true, true), None) } else { (direction(i, false, true), direction(i, true, false)) });
        if client {
            if let Some(d) = d1 {
                self.lien.envoyer(&format!("D {} {}", d.0, d.1));
            }
        } else {
            if let Some(d) = d1 {
                self.a.tourner(0, d);
            }
            if let Some(d) = d2 {
                self.a.tourner(1, d);
            }
            if self.marche && self.dernier.elapsed() >= Duration::from_millis(self.vitesse) {
                self.dernier = Instant::now();
                self.a.pas(&mut h);
                if self.reseau {
                    self.lien.envoyer(&self.a.vers_texte());
                }
                if self.a.fini() && !self.compte {
                    self.compte = true;
                    self.marche = false;
                    for j in 0..2 {
                        if self.a.vivant[j] {
                            self.victoires[j] += 1;
                        }
                    }
                }
            }
            if self.marche {
                ui.ctx().request_repaint_after(Duration::from_millis(15));
            }
        }
        // dessin
        let c = 18.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::new(SL as f32 * c, SH as f32 * c), Sense::hover());
        let p = ui.painter();
        p.rect_filled(zone, 6.0, Color32::from_rgb(22, 18, 32));
        p.rect_stroke(zone, 6.0, Stroke::new(2.0_f32, Color32::from_rgb(90, 70, 120)), egui::StrokeKind::Inside);
        let case = |x: i32, y: i32| Rect::from_min_size(zone.min + Vec2::new(x as f32 * c, y as f32 * c), Vec2::splat(c)).shrink(1.5);
        p.circle_filled(case(self.a.pomme.0, self.a.pomme.1).center(), c * 0.4, Color32::from_rgb(230, 60, 60));
        for (j, coul) in [(0, ROSE), (1, OR)] {
            for (k, &(x, y)) in self.a.corps[j].iter().enumerate() {
                let col = if self.a.vivant[j] { coul } else { Color32::from_gray(90) };
                p.rect_filled(case(x, y), if k == 0 { 6.0 } else { 3.0 }, if k == 0 { col } else { col.gamma_multiply(0.75) });
            }
        }
        if self.a.fini() {
            let t = match (self.a.vivant[0], self.a.vivant[1]) {
                (true, false) => "Le rose gagne ! 🎉",
                (false, true) => "Le jaune gagne ! 🎉",
                _ => "Égalité !",
            };
            p.text(zone.center(), Align2::CENTER_CENTER, t, FontId::proportional(34.0), Color32::WHITE);
        } else if !self.marche && !client {
            p.text(zone.center(), Align2::CENTER_CENTER, "▶ Jouer pour commencer", FontId::proportional(26.0), Color32::from_gray(200));
        }
    }
}

// ===================================================================================================
// 61 · Dames (10×10, règles internationales) contre l'ordi
// ===================================================================================================

/// 1 pion blanc, 2 dame blanche, −1 pion noir, −2 dame noire. Les blancs (le joueur) montent vers la ligne 0.
pub type Damier = [i8; 100];

#[derive(Clone, Debug, PartialEq)]
pub struct Mouv {
    pub chemin: Vec<usize>,
    pub prises: Vec<usize>,
}

pub fn damier_depart() -> Damier {
    let mut d = [0i8; 100];
    for (k, x) in d.iter_mut().enumerate() {
        let (l, c) = (k / 10, k % 10);
        if (l + c) % 2 == 1 {
            if l < 4 {
                *x = -1;
            } else if l > 5 {
                *x = 1;
            }
        }
    }
    d
}

fn case(l: i32, c: i32) -> Option<usize> {
    ((0..10).contains(&l) && (0..10).contains(&c)).then_some((l * 10 + c) as usize)
}

const DIAG: [(i32, i32); 4] = [(-1, -1), (-1, 1), (1, -1), (1, 1)];

fn rafles(d: &Damier, de: usize, piece: i8, pris: &mut Vec<usize>, chemin: &mut Vec<usize>, sortie: &mut Vec<Mouv>) {
    let (l, c) = ((de / 10) as i32, (de % 10) as i32);
    let ennemi = |x: usize| d[x] != 0 && d[x].signum() != piece.signum();
    let mut suite = false;
    for (dl, dc) in DIAG {
        if piece.abs() == 1 {
            if let (Some(a), Some(b)) = (case(l + dl, c + dc), case(l + 2 * dl, c + 2 * dc)) {
                if ennemi(a) && !pris.contains(&a) && d[b] == 0 {
                    suite = true;
                    pris.push(a);
                    chemin.push(b);
                    rafles(d, b, piece, pris, chemin, sortie);
                    chemin.pop();
                    pris.pop();
                }
            }
        } else {
            let mut k = 1;
            while let Some(x) = case(l + k * dl, c + k * dc) {
                if d[x] == 0 {
                    k += 1;
                    continue;
                }
                if ennemi(x) && !pris.contains(&x) {
                    let mut j = k + 1;
                    while let Some(y) = case(l + j * dl, c + j * dc) {
                        if d[y] != 0 {
                            break;
                        }
                        suite = true;
                        pris.push(x);
                        chemin.push(y);
                        rafles(d, y, piece, pris, chemin, sortie);
                        chemin.pop();
                        pris.pop();
                        j += 1;
                    }
                }
                break;
            }
        }
    }
    if !suite && !pris.is_empty() {
        sortie.push(Mouv { chemin: chemin.clone(), prises: pris.clone() });
    }
}

/// Coups légaux pour `cote` (1 blancs, −1 noirs) : la prise est obligatoire, et il faut prendre le plus de pièces possible.
pub fn coups(d: &Damier, cote: i8) -> Vec<Mouv> {
    let mut prises = vec![];
    for k in 0..100 {
        if d[k].signum() == cote {
            let mut b = *d;
            b[k] = 0;
            rafles(&b, k, d[k], &mut vec![], &mut vec![k], &mut prises);
        }
    }
    if !prises.is_empty() {
        let max = prises.iter().map(|m| m.prises.len()).max().unwrap();
        prises.retain(|m| m.prises.len() == max);
        return prises;
    }
    let mut v = vec![];
    for k in 0..100 {
        if d[k].signum() != cote {
            continue;
        }
        let (l, c) = ((k / 10) as i32, (k % 10) as i32);
        for (dl, dc) in DIAG {
            if d[k].abs() == 1 {
                if dl != -cote as i32 {
                    continue; // les pions avancent seulement
                }
                if let Some(x) = case(l + dl, c + dc).filter(|x| d[*x] == 0) {
                    v.push(Mouv { chemin: vec![k, x], prises: vec![] });
                }
            } else {
                let mut j = 1;
                while let Some(x) = case(l + j * dl, c + j * dc).filter(|x| d[*x] == 0) {
                    v.push(Mouv { chemin: vec![k, x], prises: vec![] });
                    j += 1;
                }
            }
        }
    }
    v
}

pub fn appliquer(d: &mut Damier, m: &Mouv) {
    let (de, vers) = (m.chemin[0], *m.chemin.last().unwrap());
    let p = d[de];
    d[de] = 0;
    for x in &m.prises {
        d[*x] = 0;
    }
    let l = vers / 10;
    d[vers] = if p == 1 && l == 0 { 2 } else if p == -1 && l == 9 { -2 } else { p };
}

fn evaluer(d: &Damier) -> i32 {
    let mut s = 0;
    for (k, &p) in d.iter().enumerate() {
        let l = (k / 10) as i32;
        let c = (k % 10) as i32;
        let centre = if (3..=6).contains(&c) { 4 } else { 0 };
        s += match p {
            1 => 100 + (9 - l) * 3 + centre,
            2 => 330,
            -1 => -(100 + l * 3 + centre),
            -2 => -330,
            _ => 0,
        };
    }
    s
}

fn negamax(d: &Damier, cote: i8, prof: u32, mut a: i32, b: i32) -> i32 {
    let v = coups(d, cote);
    if v.is_empty() {
        return -100_000 - prof as i32;
    }
    if prof == 0 {
        return evaluer(d) * cote as i32;
    }
    let mut meilleur = i32::MIN + 1;
    for m in &v {
        let mut e = *d;
        appliquer(&mut e, m);
        let s = -negamax(&e, -cote, prof - 1, -b, -a);
        meilleur = meilleur.max(s);
        a = a.max(s);
        if a >= b {
            break;
        }
    }
    meilleur
}

pub fn meilleur_coup(d: &Damier, cote: i8, prof: u32, r: &mut impl FnMut(usize) -> usize) -> Option<Mouv> {
    let v = coups(d, cote);
    let mut notes: Vec<(i32, Mouv)> = v.into_iter().map(|m| {
        let mut e = *d;
        appliquer(&mut e, &m);
        (-negamax(&e, -cote, prof.saturating_sub(1), -i32::MAX, i32::MAX), m)
    }).collect();
    let max = notes.iter().map(|x| x.0).max()?;
    notes.retain(|x| x.0 == max);
    let k = r(notes.len());
    Some(notes.swap_remove(k).1)
}

pub struct Dames {
    d: Damier,
    choisie: Option<usize>,
    niveau: u32,
    dernier: Option<Mouv>,
    reflexion: Option<Arc<Mutex<Option<Option<Mouv>>>>>,
    historique: Vec<Damier>,
    score: (u32, u32),
    compte: bool,
}

impl Dames {
    pub fn new() -> Self {
        Dames { d: damier_depart(), choisie: None, niveau: 4, dernier: None, reflexion: None, historique: vec![], score: (0, 0), compte: false }
    }
}

impl Outil for Dames {
    fn nom(&self) -> &'static str {
        "⛀ Dames"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Toi {} — {} Ordi", self.score.0, self.score.1)).strong());
            if ui.button("Nouvelle partie").clicked() && self.reflexion.is_none() {
                *self = Dames { score: self.score, niveau: self.niveau, ..Dames::new() };
            }
            if !self.historique.is_empty() && self.reflexion.is_none() && ui.button("↶ Annuler mon coup").clicked() {
                self.d = self.historique.pop().unwrap();
                self.dernier = None;
                self.choisie = None;
            }
            ui.label("Niveau");
            for (n, p) in [("Facile", 2), ("Moyen", 4), ("Fort", 6)] {
                ui.selectable_value(&mut self.niveau, p, n);
            }
        });
        // réponse de l'ordi
        if let Some(r) = self.reflexion.clone() {
            if let Some(m) = r.lock().unwrap().take() {
                if let Some(m) = m {
                    appliquer(&mut self.d, &m);
                    self.dernier = Some(m);
                }
                self.reflexion = None;
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            }
        }
        let mes_coups = if self.reflexion.is_none() { coups(&self.d, 1) } else { vec![] };
        let fin = if self.reflexion.is_some() { None } else if mes_coups.is_empty() { Some(false) } else if coups(&self.d, -1).is_empty() { Some(true) } else { None };
        if let Some(g) = fin {
            if !self.compte {
                self.compte = true;
                if g { self.score.0 += 1 } else { self.score.1 += 1 }
            }
        }
        ui.label(RichText::new(match fin {
            Some(true) => "🎉 Tu as gagné !".to_string(),
            Some(false) => "L'ordi a gagné…".to_string(),
            None if self.reflexion.is_some() => "L'ordi réfléchit…".to_string(),
            None if mes_coups.first().is_some_and(|m| !m.prises.is_empty()) => format!("À toi : prise obligatoire ({} pièce{})", mes_coups[0].prises.len(), if mes_coups[0].prises.len() > 1 { "s" } else { "" }),
            None => "À toi (blancs)".to_string(),
        }).size(17.0));
        let c = 50.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 10.0), Sense::hover());
        let mut clic = None;
        let destinations: Vec<usize> = self.choisie.map(|s| mes_coups.iter().filter(|m| m.chemin[0] == s).map(|m| *m.chemin.last().unwrap()).collect()).unwrap_or_default();
        let jouables: Vec<usize> = mes_coups.iter().map(|m| m.chemin[0]).collect();
        for k in 0..100 {
            let r = Rect::from_min_size(zone.min + Vec2::new((k % 10) as f32 * c, (k / 10) as f32 * c), Vec2::splat(c));
            let fonce = (k / 10 + k % 10) % 2 == 1;
            let mut fond = if fonce { Color32::from_rgb(120, 80, 60) } else { Color32::from_rgb(235, 215, 185) };
            if self.dernier.as_ref().is_some_and(|m| m.chemin.contains(&k)) {
                fond = Color32::from_rgb(150, 120, 60);
            }
            if self.choisie == Some(k) {
                fond = Color32::from_rgb(170, 90, 140);
            }
            ui.painter().rect_filled(r, 0.0, fond);
            if destinations.contains(&k) {
                ui.painter().circle_filled(r.center(), 8.0, Color32::from_rgba_unmultiplied(120, 255, 140, 180));
            }
            let p = self.d[k];
            if p != 0 {
                let (corps, bord) = if p > 0 { (Color32::from_rgb(245, 240, 230), Color32::from_gray(150)) } else { (Color32::from_rgb(40, 30, 30), Color32::from_gray(10)) };
                ui.painter().circle_filled(r.center(), c * 0.4, bord);
                ui.painter().circle_filled(r.center(), c * 0.35, corps);
                if p.abs() == 2 {
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, "♛", FontId::proportional(24.0), if p > 0 { Color32::from_rgb(200, 150, 30) } else { Color32::from_rgb(240, 200, 80) });
                }
                if fin.is_none() && self.reflexion.is_none() && p > 0 && jouables.contains(&k) && self.choisie.is_none() {
                    ui.painter().circle_stroke(r.center(), c * 0.42, Stroke::new(2.0_f32, OR));
                }
            }
            if fonce && ui.interact(r, ui.id().with(("dame", k)), Sense::click()).clicked() {
                clic = Some(k);
            }
        }
        if let (Some(k), None) = (clic, fin) {
            if jouables.contains(&k) {
                self.choisie = Some(k);
            } else if let Some(s) = self.choisie {
                if let Some(m) = mes_coups.iter().find(|m| m.chemin[0] == s && *m.chemin.last().unwrap() == k) {
                    self.historique.push(self.d);
                    appliquer(&mut self.d, m);
                    self.dernier = Some(m.clone());
                    self.choisie = None;
                    let (d, n) = (self.d, self.niveau);
                    let r: Arc<Mutex<Option<Option<Mouv>>>> = Arc::default();
                    let r2 = r.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(300));
                        let m = meilleur_coup(&d, -1, n, &mut h);
                        *r2.lock().unwrap() = Some(m);
                    });
                    self.reflexion = Some(r);
                }
            }
        }
        ui.weak("Règles internationales : les pions avancent en diagonale et prennent dans tous les sens ; la prise est obligatoire et il faut prendre le maximum de pièces. Un pion arrivé au bout devient dame et se déplace de plusieurs cases.");
    }
}

// ===================================================================================================
// 62 · Sudoku du jour
// ===================================================================================================

/// Numéro du jour (local) depuis le 1er janvier 1970.
fn jour_local() -> i64 {
    let t = crate::minecraft::horodatage();
    let n = |a: usize, b: usize| t.get(a..b).and_then(|x| x.parse::<u32>().ok()).unwrap_or(1);
    crate::quotidien2::jours_depuis_1970(n(0, 4) as i32, n(4, 6), n(6, 8))
}

/// Grille du jour : la même pour tout le monde le même jour.
pub fn sudoku_du_jour(jour: i64) -> (Sudoku, Sudoku) {
    let mut s = (jour as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut r = move |n: usize| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s % n.max(1) as u64) as usize
    };
    // plus difficile en fin de semaine (le 1er janvier 1970 était un jeudi)
    let indices = match jour.rem_euclid(7) {
        2 | 3 => 28,
        0 | 1 => 32,
        _ => 36,
    };
    generer_sudoku_avec(indices, &mut r)
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct EtatJour {
    jour: i64,
    saisie: Vec<u8>,
    secondes: u64,
    fini: bool,
    serie: u32,
    dernier_reussi: i64,
    meilleur: Option<u64>,
    total: u32,
}

pub struct SudokuJour {
    e: EtatJour,
    depart: Sudoku,
    sol: Sudoku,
    choisie: Option<usize>,
    vu: Instant,
    accu: f32,
}

impl SudokuJour {
    pub fn new() -> Self {
        let mut s = SudokuJour { e: charger("sudoku_jour"), depart: [0; 81], sol: [0; 81], choisie: None, vu: Instant::now(), accu: 0.0 };
        s.preparer();
        s
    }

    fn preparer(&mut self) {
        let j = jour_local();
        (self.depart, self.sol) = sudoku_du_jour(j);
        if self.e.jour != j || self.e.saisie.len() != 81 {
            self.e.jour = j;
            self.e.saisie = self.depart.to_vec();
            self.e.secondes = 0;
            self.e.fini = false;
        }
    }
}

impl Outil for SudokuJour {
    fn nom(&self) -> &'static str {
        "📅 Sudoku du jour"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.e.jour != jour_local() {
            self.preparer();
        }
        // chrono : ne compte que quand la grille est à l'écran
        let dt = self.vu.elapsed().as_secs_f32();
        self.vu = Instant::now();
        if !self.e.fini && dt < 2.0 {
            self.accu += dt;
            if self.accu >= 1.0 {
                self.e.secondes += self.accu as u64;
                self.accu = self.accu.fract();
                if self.e.secondes % 15 == 0 {
                    enregistrer("sudoku_jour", &self.e);
                }
            }
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        let serie = if self.e.dernier_reussi >= self.e.jour - 1 { self.e.serie } else { 0 };
        ui.horizontal(|ui| {
            let niveau = match self.e.jour.rem_euclid(7) {
                2 | 3 => "difficile",
                0 | 1 => "moyen",
                _ => "facile",
            };
            ui.heading(format!("Grille du jour ({niveau})"));
            ui.label(RichText::new(format!("⏱ {}:{:02}", self.e.secondes / 60, self.e.secondes % 60)).size(17.0));
            ui.label(RichText::new(format!("🔥 série : {serie} jour{}", if serie > 1 { "s" } else { "" })).color(OR));
            if let Some(m) = self.e.meilleur {
                ui.weak(format!("record {}:{:02} · {} grilles réussies", m / 60, m % 60, self.e.total));
            }
        });
        ui.weak("Une nouvelle grille chaque jour, la même pour tout le monde : compare ton temps avec tes amis ! Clique une case puis tape un chiffre.");
        let chiffre = ui.input(|i| {
            for (k, v) in [(Key::Num1, 1), (Key::Num2, 2), (Key::Num3, 3), (Key::Num4, 4), (Key::Num5, 5), (Key::Num6, 6), (Key::Num7, 7), (Key::Num8, 8), (Key::Num9, 9), (Key::Num0, 0), (Key::Delete, 0), (Key::Backspace, 0)] {
                if i.key_pressed(k) {
                    return Some(v);
                }
            }
            None
        });
        let mut saisie: Sudoku = [0; 81];
        saisie.copy_from_slice(&self.e.saisie);
        if let (Some(v), Some(i)) = (chiffre, self.choisie) {
            if self.depart[i] == 0 && !self.e.fini {
                saisie[i] = v;
                self.e.saisie = saisie.to_vec();
                if saisie == self.sol {
                    self.e.fini = true;
                    self.e.total += 1;
                    self.e.serie = if self.e.dernier_reussi == self.e.jour - 1 { self.e.serie + 1 } else { 1 };
                    self.e.dernier_reussi = self.e.jour;
                    if self.e.meilleur.is_none_or(|m| self.e.secondes < m) {
                        self.e.meilleur = Some(self.e.secondes);
                    }
                }
                enregistrer("sudoku_jour", &self.e);
            }
        }
        let c = 46.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 9.0), Sense::hover());
        let sel = self.choisie.map(|i| saisie[i]).filter(|v| *v != 0);
        for i in 0..81 {
            let r = Rect::from_min_size(zone.min + Vec2::new((i % 9) as f32 * c, (i / 9) as f32 * c), Vec2::splat(c)).shrink(1.0);
            if ui.interact(r, ui.id().with(("sdj", i)), Sense::click()).clicked() {
                self.choisie = Some(i);
            }
            let v = saisie[i];
            let conflit = v != 0 && self.depart[i] == 0 && {
                let mut g = saisie;
                g[i] = 0;
                !possible(&g, i, v)
            };
            let fond = if self.choisie == Some(i) { Color32::from_rgb(110, 60, 110) } else if sel.is_some() && sel == Some(v) { Color32::from_rgb(70, 50, 90) } else { Color32::from_rgb(40, 32, 52) };
            ui.painter().rect_filled(r, 2.0, fond);
            if v != 0 {
                let coul = if self.depart[i] != 0 { Color32::WHITE } else if conflit { Color32::from_rgb(255, 90, 90) } else { ROSE };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, v.to_string(), FontId::proportional(24.0), coul);
            }
        }
        for k in 0..=3 {
            let x = zone.left() + k as f32 * 3.0 * c;
            let y = zone.top() + k as f32 * 3.0 * c;
            ui.painter().line_segment([egui::pos2(x, zone.top()), egui::pos2(x, zone.bottom())], Stroke::new(2.5_f32, Color32::from_gray(200)));
            ui.painter().line_segment([egui::pos2(zone.left(), y), egui::pos2(zone.right(), y)], Stroke::new(2.5_f32, Color32::from_gray(200)));
        }
        if self.e.fini {
            ui.label(RichText::new(format!("🎉 Réussie en {}:{:02} ! Reviens demain pour une nouvelle grille.", self.e.secondes / 60, self.e.secondes % 60)).size(18.0).strong());
            if ui.button("📋 Copier mon temps").clicked() {
                ui.ctx().copy_text(format!("📅 Sudoku du jour d'Outils : réussi en {}:{:02} 🔥 série de {} jour(s)", self.e.secondes / 60, self.e.secondes % 60, self.e.serie));
            }
        }
    }
}

// ===================================================================================================
// 63 · Quiz drapeaux et capitales
// ===================================================================================================

/// (code ISO pour flagcdn, pays, capitale)
pub const PAYS: [(&str, &str, &str); 100] = [
    ("fr", "France", "Paris"), ("be", "Belgique", "Bruxelles"), ("ch", "Suisse", "Berne"), ("ca", "Canada", "Ottawa"), ("de", "Allemagne", "Berlin"),
    ("es", "Espagne", "Madrid"), ("it", "Italie", "Rome"), ("pt", "Portugal", "Lisbonne"), ("gb", "Royaume-Uni", "Londres"), ("ie", "Irlande", "Dublin"),
    ("nl", "Pays-Bas", "Amsterdam"), ("lu", "Luxembourg", "Luxembourg"), ("at", "Autriche", "Vienne"), ("pl", "Pologne", "Varsovie"), ("cz", "Tchéquie", "Prague"),
    ("sk", "Slovaquie", "Bratislava"), ("hu", "Hongrie", "Budapest"), ("ro", "Roumanie", "Bucarest"), ("bg", "Bulgarie", "Sofia"), ("gr", "Grèce", "Athènes"),
    ("hr", "Croatie", "Zagreb"), ("si", "Slovénie", "Ljubljana"), ("rs", "Serbie", "Belgrade"), ("ba", "Bosnie-Herzégovine", "Sarajevo"), ("al", "Albanie", "Tirana"),
    ("mk", "Macédoine du Nord", "Skopje"), ("me", "Monténégro", "Podgorica"), ("ua", "Ukraine", "Kiev"), ("by", "Biélorussie", "Minsk"), ("ru", "Russie", "Moscou"),
    ("lt", "Lituanie", "Vilnius"), ("lv", "Lettonie", "Riga"), ("ee", "Estonie", "Tallinn"), ("fi", "Finlande", "Helsinki"), ("se", "Suède", "Stockholm"),
    ("no", "Norvège", "Oslo"), ("dk", "Danemark", "Copenhague"), ("is", "Islande", "Reykjavik"), ("mt", "Malte", "La Valette"), ("cy", "Chypre", "Nicosie"),
    ("tr", "Turquie", "Ankara"), ("md", "Moldavie", "Chisinau"), ("ma", "Maroc", "Rabat"), ("dz", "Algérie", "Alger"), ("tn", "Tunisie", "Tunis"),
    ("eg", "Égypte", "Le Caire"), ("sn", "Sénégal", "Dakar"), ("ci", "Côte d'Ivoire", "Yamoussoukro"), ("ml", "Mali", "Bamako"), ("cm", "Cameroun", "Yaoundé"),
    ("ng", "Nigeria", "Abuja"), ("gh", "Ghana", "Accra"), ("ke", "Kenya", "Nairobi"), ("et", "Éthiopie", "Addis-Abeba"), ("za", "Afrique du Sud", "Pretoria"),
    ("cd", "RD Congo", "Kinshasa"), ("cg", "Congo", "Brazzaville"), ("mg", "Madagascar", "Antananarivo"), ("ga", "Gabon", "Libreville"), ("bf", "Burkina Faso", "Ouagadougou"),
    ("ne", "Niger", "Niamey"), ("td", "Tchad", "N'Djamena"), ("gn", "Guinée", "Conakry"), ("bj", "Bénin", "Porto-Novo"), ("tg", "Togo", "Lomé"),
    ("us", "États-Unis", "Washington"), ("mx", "Mexique", "Mexico"), ("br", "Brésil", "Brasilia"), ("ar", "Argentine", "Buenos Aires"), ("cl", "Chili", "Santiago"),
    ("pe", "Pérou", "Lima"), ("co", "Colombie", "Bogota"), ("ve", "Venezuela", "Caracas"), ("uy", "Uruguay", "Montevideo"), ("py", "Paraguay", "Asuncion"),
    ("bo", "Bolivie", "Sucre"), ("ec", "Équateur", "Quito"), ("cu", "Cuba", "La Havane"), ("ht", "Haïti", "Port-au-Prince"), ("jm", "Jamaïque", "Kingston"),
    ("cn", "Chine", "Pékin"), ("jp", "Japon", "Tokyo"), ("kr", "Corée du Sud", "Séoul"), ("in", "Inde", "New Delhi"), ("pk", "Pakistan", "Islamabad"),
    ("th", "Thaïlande", "Bangkok"), ("vn", "Viêt Nam", "Hanoï"), ("id", "Indonésie", "Jakarta"), ("ph", "Philippines", "Manille"), ("my", "Malaisie", "Kuala Lumpur"),
    ("sa", "Arabie saoudite", "Riyad"), ("ir", "Iran", "Téhéran"), ("iq", "Irak", "Bagdad"), ("il", "Israël", "Jérusalem"), ("lb", "Liban", "Beyrouth"),
    ("au", "Australie", "Canberra"), ("nz", "Nouvelle-Zélande", "Wellington"), ("af", "Afghanistan", "Kaboul"), ("mn", "Mongolie", "Oulan-Bator"), ("np", "Népal", "Katmandou"),
];

/// Question : (pays visé, 4 propositions d'indices de pays mélangées).
pub fn question(r: &mut impl FnMut(usize) -> usize, deja: &[usize]) -> (usize, [usize; 4]) {
    let mut bonne = r(PAYS.len());
    let mut essais = 0;
    while deja.contains(&bonne) && essais < 200 {
        bonne = r(PAYS.len());
        essais += 1;
    }
    let mut v = vec![bonne];
    while v.len() < 4 {
        let x = r(PAYS.len());
        if !v.contains(&x) {
            v.push(x);
        }
    }
    melanger(&mut v, r);
    (bonne, [v[0], v[1], v[2], v[3]])
}

#[derive(PartialEq, Clone, Copy)]
enum ModeQuiz {
    Drapeaux,
    Capitales,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct RecordsQuiz {
    drapeaux: u32,
    capitales: u32,
}

pub struct QuizDrapeaux {
    mode: ModeQuiz,
    q: (usize, [usize; 4]),
    vus: Vec<usize>,
    numero: usize,
    points: u32,
    repondu: Option<usize>,
    records: RecordsQuiz,
    serie_finie: bool,
}

const QUESTIONS: usize = 10;

impl QuizDrapeaux {
    pub fn new() -> Self {
        let mut q = QuizDrapeaux { mode: ModeQuiz::Drapeaux, q: (0, [0; 4]), vus: vec![], numero: 0, points: 0, repondu: None, records: charger("quiz_drapeaux"), serie_finie: false };
        q.recommencer();
        q
    }

    fn recommencer(&mut self) {
        self.vus.clear();
        self.numero = 1;
        self.points = 0;
        self.repondu = None;
        self.serie_finie = false;
        self.q = question(&mut h, &self.vus);
        self.vus.push(self.q.0);
    }

    fn suivante(&mut self) {
        if self.numero >= QUESTIONS {
            self.serie_finie = true;
            let r = if self.mode == ModeQuiz::Drapeaux { &mut self.records.drapeaux } else { &mut self.records.capitales };
            if self.points > *r {
                *r = self.points;
                enregistrer("quiz_drapeaux", &self.records);
            }
            return;
        }
        self.numero += 1;
        self.repondu = None;
        self.q = question(&mut h, &self.vus);
        self.vus.push(self.q.0);
    }
}

fn drapeau(ui: &mut egui::Ui, code: &str, largeur: f32) {
    ui.add(egui::Image::new(format!("https://flagcdn.com/w320/{code}.png")).fit_to_exact_size(Vec2::new(largeur, largeur * 0.62)).maintain_aspect_ratio(true).corner_radius(4.0));
}

impl Outil for QuizDrapeaux {
    fn nom(&self) -> &'static str {
        "🏳 Quiz drapeaux et capitales"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (m, n) in [(ModeQuiz::Drapeaux, "🏳 Drapeaux"), (ModeQuiz::Capitales, "🏛 Capitales")] {
                if ui.selectable_label(self.mode == m, RichText::new(n).size(16.0)).clicked() && self.mode != m {
                    self.mode = m;
                    self.recommencer();
                }
            }
            ui.weak(format!("record : {}/{QUESTIONS}", if self.mode == ModeQuiz::Drapeaux { self.records.drapeaux } else { self.records.capitales }));
        });
        ui.separator();
        if self.serie_finie {
            ui.label(RichText::new(format!("Score : {}/{QUESTIONS}", self.points)).size(28.0).strong());
            ui.label(match self.points {
                10 => "Parfait ! 🌍🏆",
                8..=9 => "Excellent ! 🎉",
                5..=7 => "Pas mal du tout !",
                _ => "Continue de t'entraîner 🙂",
            });
            if ui.button(RichText::new("Rejouer").size(18.0)).clicked() {
                self.recommencer();
            }
            return;
        }
        let (bonne, choix) = self.q;
        ui.label(RichText::new(format!("Question {}/{QUESTIONS} · {} point{}", self.numero, self.points, if self.points > 1 { "s" } else { "" })).weak());
        match self.mode {
            ModeQuiz::Drapeaux => {
                ui.label(RichText::new("À quel pays est ce drapeau ?").size(18.0));
                drapeau(ui, PAYS[bonne].0, 260.0);
            }
            ModeQuiz::Capitales => {
                ui.horizontal(|ui| {
                    drapeau(ui, PAYS[bonne].0, 70.0);
                    ui.label(RichText::new(format!("Quelle est la capitale de : {} ?", PAYS[bonne].1)).size(20.0).strong());
                });
            }
        }
        ui.add_space(8.0);
        let mut clic = None;
        egui::Grid::new("quiz_choix").spacing(Vec2::splat(8.0)).show(ui, |ui| {
            for (k, &c) in choix.iter().enumerate() {
                let texte = if self.mode == ModeQuiz::Drapeaux { PAYS[c].1 } else { PAYS[c].2 };
                let mut b = egui::Button::new(RichText::new(texte).size(17.0)).min_size(Vec2::new(240.0, 44.0));
                if let Some(r) = self.repondu {
                    if c == bonne {
                        b = b.fill(Color32::from_rgb(40, 130, 70));
                    } else if c == r {
                        b = b.fill(Color32::from_rgb(160, 40, 50));
                    }
                }
                if ui.add(b).clicked() && self.repondu.is_none() {
                    clic = Some(c);
                }
                if k % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        let touche = ui.input(|i| [Key::Num1, Key::Num2, Key::Num3, Key::Num4].iter().position(|k| i.key_pressed(*k)));
        if let (Some(k), None) = (touche, self.repondu) {
            clic = Some(choix[k]);
        }
        if let Some(c) = clic {
            self.repondu = Some(c);
            if c == bonne {
                self.points += 1;
            }
        }
        if let Some(r) = self.repondu {
            ui.label(RichText::new(if r == bonne { "✔ Bonne réponse !".to_string() } else { format!("✖ C'était {} ({})", PAYS[bonne].1, PAYS[bonne].2) }).size(17.0));
            if ui.button(RichText::new("Suivante ➜").size(17.0)).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                self.suivante();
            }
        }
    }
}

// ===================================================================================================
// 64 · Course de frappe en réseau
// ===================================================================================================

const PORT_COURSE: u16 = 47_323;

/// Nombre de caractères tapés correctement depuis le début.
pub fn avance(texte: &str, tape: &str) -> usize {
    texte.chars().zip(tape.chars()).take_while(|(a, b)| a == b).count()
}

/// Mots par minute (un mot = 5 caractères).
pub fn mpm(caracteres: usize, ms: u128) -> u32 {
    if ms == 0 { 0 } else { (caracteres as f64 / 5.0 / (ms as f64 / 60_000.0)).round() as u32 }
}

pub struct CourseFrappe {
    lien: Lien,
    texte: Option<String>,
    tape: String,
    depart: Option<Instant>,
    mon_temps: Option<u128>,
    son_avance: usize,
    son_temps: Option<u128>,
    envoye: usize,
    score: (u32, u32),
    compte: bool,
}

impl CourseFrappe {
    pub fn new() -> Self {
        CourseFrappe { lien: Lien::new(PORT_COURSE), texte: None, tape: String::new(), depart: None, mon_temps: None, son_avance: 0, son_temps: None, envoye: 0, score: (0, 0), compte: false }
    }

    fn lancer(&mut self, k: usize) {
        self.texte = Some(crate::minijeux3::TEXTES[k % crate::minijeux3::TEXTES.len()].to_string());
        self.tape.clear();
        self.depart = Some(Instant::now() + Duration::from_secs(3));
        self.mon_temps = None;
        self.son_avance = 0;
        self.son_temps = None;
        self.envoye = 0;
        self.compte = false;
    }
}

impl Outil for CourseFrappe {
    fn nom(&self) -> &'static str {
        "🏁 Course de frappe en réseau"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Course de frappe à deux");
        if !self.lien.connecte() {
            ui.weak("Le même texte s'affiche sur les deux PC : le premier qui le tape sans faute gagne.");
            if self.lien.ui(ui) {
                self.texte = None;
            }
            return;
        }
        if self.lien.bandeau(ui, Some(self.score)) {
            return;
        }
        for m in self.lien.recus() {
            let mut p = m.splitn(2, ' ');
            match (p.next(), p.next()) {
                (Some("COURSE"), Some(k)) => self.lancer(k.trim().parse().unwrap_or(0)),
                (Some("P"), Some(n)) => self.son_avance = n.trim().parse().unwrap_or(0),
                (Some("FIN"), Some(ms)) => self.son_temps = ms.trim().parse().ok(),
                _ => {}
            }
        }
        let fini = self.mon_temps.is_some() || self.son_temps.is_some();
        if self.lien.hote && (self.texte.is_none() || fini) {
            if ui.button(RichText::new("🏁 Lancer une course").size(18.0)).clicked() {
                let k = h(crate::minijeux3::TEXTES.len());
                self.lien.envoyer(&format!("COURSE {k}"));
                self.lancer(k);
            }
        } else if self.texte.is_none() {
            ui.label("En attente : l'autre joueur va lancer la course…");
        }
        let Some(texte) = self.texte.clone() else { return };
        let total = texte.chars().count();
        let maintenant = Instant::now();
        let depart = self.depart.unwrap_or(maintenant);
        if maintenant < depart {
            let s = (depart - maintenant).as_secs() + 1;
            ui.label(RichText::new(format!("Prêt ? {s}…")).size(40.0).strong().color(OR));
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        }
        let ecoule = self.mon_temps.unwrap_or_else(|| (maintenant - depart).as_millis());
        let a = avance(&texte, &self.tape);
        for (nom, n, coul) in [("Toi", a, ROSE), ("L'autre", self.son_avance, BLEU)] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(nom).color(coul).strong());
                ui.add(egui::ProgressBar::new(n as f32 / total as f32).desired_width(420.0).fill(coul).text(format!("{} %", n * 100 / total.max(1))));
            });
        }
        // texte coloré : vert = juste, rouge = la faute
        let mut job = egui::text::LayoutJob::default();
        let police = FontId::proportional(20.0);
        let tape: Vec<char> = self.tape.chars().collect();
        for (k, ch) in texte.chars().enumerate() {
            let coul = if k < a { Color32::from_rgb(110, 220, 130) } else if k < tape.len() { Color32::from_rgb(255, 90, 90) } else { Color32::from_gray(210) };
            let fond = if k == a { Color32::from_rgb(70, 50, 90) } else { Color32::TRANSPARENT };
            job.append(&ch.to_string(), 0.0, egui::TextFormat { font_id: police.clone(), color: coul, background: fond, ..Default::default() });
        }
        job.wrap.max_width = 620.0;
        ui.label(job);
        if self.mon_temps.is_none() {
            let r = ui.add(egui::TextEdit::singleline(&mut self.tape).desired_width(620.0).font(FontId::proportional(20.0)));
            r.request_focus();
            let a = avance(&texte, &self.tape);
            if a != self.envoye {
                self.envoye = a;
                self.lien.envoyer(&format!("P {a}"));
            }
            if a == total {
                self.mon_temps = Some(ecoule);
                self.lien.envoyer(&format!("FIN {ecoule}"));
            }
            ui.label(format!("⏱ {:.1} s · {} mots/min", ecoule as f64 / 1000.0, mpm(a, ecoule)));
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let fin = match (self.mon_temps, self.son_temps) {
            (Some(m), Some(s)) => Some(m <= s),
            (Some(_), None) => Some(true),
            (None, Some(_)) => Some(false),
            _ => None,
        };
        if let Some(g) = fin {
            if !self.compte {
                self.compte = true;
                if g { self.score.0 += 1 } else { self.score.1 += 1 }
            }
            ui.label(RichText::new(if g {
                format!("🏆 Tu gagnes ! {:.1} s, {} mots/min", self.mon_temps.unwrap_or(0) as f64 / 1000.0, mpm(total, self.mon_temps.unwrap_or(1)))
            } else {
                format!("L'autre a fini en {:.1} s ({} mots/min)", self.son_temps.unwrap_or(0) as f64 / 1000.0, mpm(total, self.son_temps.unwrap_or(1)))
            }).size(20.0).strong());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng(mut s: u64) -> impl FnMut(usize) -> usize {
        move |n| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % n.max(1) as u64) as usize
        }
    }

    #[test]
    fn cartes() {
        let p = paquet();
        assert_eq!(p.len(), 52);
        let c = |v, c| Carte { v, c };
        let mut a: VecDeque<Carte> = [c(1, 0)].into();
        let mut b: VecDeque<Carte> = [c(13, 1)].into();
        assert_eq!(manche(&mut a, &mut b).0, 0); // l'as bat le roi
        let mut a: VecDeque<Carte> = [c(5, 0), c(2, 0), c(9, 0)].into();
        let mut b: VecDeque<Carte> = [c(5, 1), c(3, 1), c(4, 1)].into();
        let (g, t, n) = manche(&mut a, &mut b);
        assert_eq!((g, t.len(), n), (0, 6, 1));
        // bataille et l'un n'a plus de carte
        let mut a: VecDeque<Carte> = [c(5, 0)].into();
        let mut b: VecDeque<Carte> = [c(5, 1), c(3, 1), c(4, 1)].into();
        assert_eq!(manche(&mut a, &mut b).0, 1);
        // une partie complète se termine toujours (ou atteint la limite)
        let mut r = rng(42);
        let mut p = paquet();
        melanger(&mut p, &mut r);
        let mut t: [VecDeque<Carte>; 2] = [p[..26].iter().copied().collect(), p[26..].iter().copied().collect()];
        for _ in 0..3000 {
            if t[0].is_empty() || t[1].is_empty() {
                break;
            }
            let [a, b] = &mut t;
            let (g, mut pile, _) = manche(a, b);
            melanger(&mut pile, &mut r);
            t[g].extend(pile);
        }
        assert_eq!(t[0].len() + t[1].len(), 52);
        // huit américain
        assert!(jouable(c(8, 2), c(3, 0), None));
        assert!(jouable(c(3, 2), c(3, 0), None));
        assert!(!jouable(c(4, 2), c(3, 0), None));
        assert!(jouable(c(4, 2), c(8, 0), Some(2)));
        let main = [c(8, 1), c(5, 0), c(9, 0), c(5, 3)];
        assert_eq!(coup_ordi(&main, c(5, 2), None), Some(2 - 1)); // le 5♠ (couleur préférée) plutôt que le 8
        assert_eq!(coup_ordi(&[c(8, 1)], c(5, 2), None), Some(0));
        assert_eq!(coup_ordi(&[c(4, 1)], c(5, 2), None), None);
    }

    #[test]
    fn navale() {
        let mut r = rng(7);
        for _ in 0..30 {
            let f = placer_flotte(&mut r);
            assert_eq!(f.iter().map(|b| b.len()).sum::<usize>(), 17);
            let mut toutes: Vec<usize> = f.iter().flatten().copied().collect();
            toutes.sort();
            toutes.dedup();
            assert_eq!(toutes.len(), 17);
        }
        let f = vec![vec![0, 1], vec![50, 60, 70]];
        let mut t = [false; 100];
        assert_eq!(tirer(&f, &mut t, 5), Tir::Eau);
        assert_eq!(tirer(&f, &mut t, 0), Tir::Touche);
        assert_eq!(tirer(&f, &mut t, 1), Tir::Coule(vec![0, 1]));
        assert!(!tout_coule(&f, &t));
        let mut radar = [0u8; 100];
        radar[55] = 2;
        radar[56] = 2;
        let k = cible_ordi(&radar, &mut r);
        assert!(k == 54 || k == 57);
        // l'ordi finit toujours par tout couler en moins de 100 coups
        let f = placer_flotte(&mut r);
        let (mut t, mut radar) = ([false; 100], [0u8; 100]);
        let mut n = 0;
        while !tout_coule(&f, &t) {
            let k = cible_ordi(&radar, &mut r);
            assert_eq!(radar[k], 0);
            let x = tirer(&f, &mut t, k);
            marquer(&mut radar, k, &x);
            n += 1;
        }
        assert!(n <= 100);
    }

    #[test]
    fn snake() {
        let mut r = rng(3);
        let mut a = Arene::new(&mut r);
        a.pomme = (7, SH / 2 - 4);
        a.pas(&mut r);
        assert_eq!(a.score[0], 1);
        assert_eq!(a.corps[0].len(), 5);
        a.tourner(0, (-1, 0)); // demi-tour interdit
        a.pas(&mut r);
        assert_eq!(a.corps[0][0], (8, SH / 2 - 4));
        let t = a.vers_texte();
        let b = Arene::depuis_texte(&t).unwrap();
        assert_eq!((b.corps.clone(), b.score, b.pomme), (a.corps.clone(), a.score, a.pomme));
        for _ in 0..100 {
            a.pas(&mut r);
        }
        assert!(a.fini()); // les murs finissent par arrêter les serpents
    }

    #[test]
    fn dames() {
        let d = damier_depart();
        assert_eq!(d.iter().filter(|x| **x == 1).count(), 20);
        assert_eq!(coups(&d, 1).len(), 9);
        // prise obligatoire et rafle maximale
        let mut d = [0i8; 100];
        d[61] = 1;
        d[52] = -1;
        d[32] = -1;
        d[67] = 1;
        d[56] = -1;
        let v = coups(&d, 1);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].prises.len(), 2);
        assert_eq!(v[0].chemin, vec![61, 43, 21]);
        // prise en arrière pour un pion
        let mut d = [0i8; 100];
        d[43] = 1;
        d[54] = -1;
        assert_eq!(coups(&d, 1)[0].chemin, vec![43, 65]);
        // dame volante
        let mut d = [0i8; 100];
        d[90] = 2;
        d[45] = -1;
        let v = coups(&d, 1);
        assert!(v.iter().all(|m| m.prises == vec![45]));
        assert_eq!(v.len(), 4); // atterrit sur 36, 27, 18 ou 09
        // promotion
        let mut d = [0i8; 100];
        d[12] = 1;
        let m = coups(&d, 1).into_iter().next().unwrap();
        appliquer(&mut d, &m);
        assert!(d.contains(&2));
        // l'ordi prend une pièce offerte
        let mut d = [0i8; 100];
        d[23] = -1;
        d[34] = 1;
        d[90] = 1;
        let m = meilleur_coup(&d, -1, 3, &mut rng(1)).unwrap();
        assert_eq!(m.prises, vec![34]);
    }

    #[test]
    fn sudoku_et_quiz() {
        let (g, s) = sudoku_du_jour(20_000);
        let (g2, _) = sudoku_du_jour(20_000);
        assert_eq!(g, g2);
        assert!(s.iter().all(|v| *v > 0));
        assert!(g.iter().zip(s.iter()).all(|(a, b)| *a == 0 || a == b));
        assert_ne!(sudoku_du_jour(20_001).0, g);
        let mut r = rng(9);
        for _ in 0..50 {
            let (b, c) = question(&mut r, &[]);
            assert!(c.contains(&b));
            let mut x = c.to_vec();
            x.dedup();
            x.sort();
            x.dedup();
            assert_eq!(x.len(), 4);
        }
        let mut codes: Vec<&str> = PAYS.iter().map(|p| p.0).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), PAYS.len());
        assert_eq!(avance("bonjour", "bonsoir"), 3);
        assert_eq!(mpm(250, 60_000), 50);
    }
}
