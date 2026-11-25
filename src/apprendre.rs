//! Apprendre : aide aux maths (10) — équations pas à pas, courbes, fractions, nombres premiers —
//! et tableau périodique interactif (12) avec masse molaire et quiz.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, FontId, Pos2, RichText, Stroke};

// ---------------------------------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Nombre(f64),
    X,
    Neg(Box<Expr>),
    Op(char, Box<Expr>, Box<Expr>),
    Fonction(String, Box<Expr>),
}

impl Expr {
    pub fn eval(&self, x: f64) -> f64 {
        match self {
            Expr::Nombre(v) => *v,
            Expr::X => x,
            Expr::Neg(a) => -a.eval(x),
            Expr::Op(o, a, b) => {
                let (a, b) = (a.eval(x), b.eval(x));
                match o {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => a.powf(b),
                }
            }
            Expr::Fonction(f, a) => {
                let v = a.eval(x);
                match f.as_str() {
                    "sin" => v.sin(),
                    "cos" => v.cos(),
                    "tan" => v.tan(),
                    "sqrt" | "racine" => v.sqrt(),
                    "abs" => v.abs(),
                    "ln" => v.ln(),
                    "log" => v.log10(),
                    "exp" => v.exp(),
                    _ => f64::NAN,
                }
            }
        }
    }
}

struct Lecteur {
    c: Vec<char>,
    i: usize,
}

impl Lecteur {
    fn espace(&mut self) {
        while self.c.get(self.i).is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }
    fn voir(&mut self) -> Option<char> {
        self.espace();
        self.c.get(self.i).copied()
    }
    fn somme(&mut self) -> Result<Expr, String> {
        let mut a = self.produit()?;
        while let Some(o @ ('+' | '-')) = self.voir() {
            self.i += 1;
            a = Expr::Op(o, Box::new(a), Box::new(self.produit()?));
        }
        Ok(a)
    }
    fn produit(&mut self) -> Result<Expr, String> {
        let mut a = self.unaire()?;
        loop {
            match self.voir() {
                Some(o @ ('*' | '/' | '×' | '÷')) => {
                    self.i += 1;
                    let o = if o == '×' { '*' } else if o == '÷' { '/' } else { o };
                    a = Expr::Op(o, Box::new(a), Box::new(self.unaire()?));
                }
                // multiplication implicite : 2x, 3(x+1), x(x-1), 2sin(x)
                Some(c) if c == '(' || c.is_alphabetic() || (c.is_ascii_digit() && matches!(a, Expr::X)) => {
                    a = Expr::Op('*', Box::new(a), Box::new(self.puissance()?));
                }
                _ => return Ok(a),
            }
        }
    }
    fn unaire(&mut self) -> Result<Expr, String> {
        match self.voir() {
            Some('-') => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unaire()?)))
            }
            Some('+') => {
                self.i += 1;
                self.unaire()
            }
            _ => self.puissance(),
        }
    }
    fn puissance(&mut self) -> Result<Expr, String> {
        let base = self.atome()?;
        match self.voir() {
            Some('^') => {
                self.i += 1;
                Ok(Expr::Op('^', Box::new(base), Box::new(self.unaire()?)))
            }
            Some('²') => {
                self.i += 1;
                Ok(Expr::Op('^', Box::new(base), Box::new(Expr::Nombre(2.0))))
            }
            Some('³') => {
                self.i += 1;
                Ok(Expr::Op('^', Box::new(base), Box::new(Expr::Nombre(3.0))))
            }
            _ => Ok(base),
        }
    }
    fn atome(&mut self) -> Result<Expr, String> {
        match self.voir() {
            Some('(') => {
                self.i += 1;
                let e = self.somme()?;
                if self.voir() != Some(')') {
                    return Err("parenthèse fermante manquante".into());
                }
                self.i += 1;
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() || c == '.' || c == ',' => {
                let d = self.i;
                while self.c.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == '.' || *c == ',') {
                    self.i += 1;
                }
                let t: String = self.c[d..self.i].iter().collect::<String>().replace(',', ".");
                t.parse().map(Expr::Nombre).map_err(|_| format!("nombre « {t} » illisible"))
            }
            Some(c) if c.is_alphabetic() => {
                let d = self.i;
                while self.c.get(self.i).is_some_and(|c| c.is_alphabetic()) {
                    self.i += 1;
                }
                let m: String = self.c[d..self.i].iter().collect::<String>().to_lowercase();
                match m.as_str() {
                    "x" => Ok(Expr::X),
                    "pi" | "π" => Ok(Expr::Nombre(std::f64::consts::PI)),
                    "e" => Ok(Expr::Nombre(std::f64::consts::E)),
                    "sin" | "cos" | "tan" | "sqrt" | "racine" | "abs" | "ln" | "log" | "exp" => Ok(Expr::Fonction(m, Box::new(self.puissance()?))),
                    _ => {
                        // « xx », « 2xy »… : on ne connaît que x
                        if m.chars().all(|c| c == 'x') {
                            let mut e = Expr::X;
                            for _ in 1..m.len() {
                                e = Expr::Op('*', Box::new(e), Box::new(Expr::X));
                            }
                            Ok(e)
                        } else {
                            Err(format!("« {m} » inconnu (utilise x, sin, cos, sqrt, pi…)"))
                        }
                    }
                }
            }
            Some(c) => Err(format!("« {c} » inattendu")),
            None => Err("expression incomplète".into()),
        }
    }
}

pub fn lire(t: &str) -> Result<Expr, String> {
    let mut l = Lecteur { c: t.chars().collect(), i: 0 };
    let e = l.somme()?;
    if l.voir().is_some() {
        return Err(format!("je ne comprends pas la fin : « {} »", l.c[l.i..].iter().collect::<String>()));
    }
    Ok(e)
}

/// Nombre affiché simplement : entier, fraction simple, ou décimal arrondi.
pub fn joli(v: f64) -> String {
    if !v.is_finite() {
        return "impossible".into();
    }
    if (v - v.round()).abs() < 1e-9 {
        return format!("{}", v.round() as i64);
    }
    for d in 2..=100i64 {
        let n = v * d as f64;
        if (n - n.round()).abs() < 1e-9 {
            return format!("{}/{d}", n.round() as i64);
        }
    }
    format!("{:.4}", v).trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

/// Résolution pas à pas de « gauche = droite ».
pub fn resoudre(eq: &str) -> Result<Vec<String>, String> {
    let (g, d) = eq.split_once('=').ok_or("il manque le signe =")?;
    let (eg, ed) = (lire(g)?, lire(d)?);
    let f = |x: f64| eg.eval(x) - ed.eval(x);
    let mut etapes = vec![format!("Équation : {} = {}", g.trim(), d.trim())];
    // degré 2 au plus ? (différences finies)
    let (f0, f1, f2, f3, f7) = (f(0.0), f(1.0), f(2.0), f(3.0), f(7.0));
    let a = (f2 - 2.0 * f1 + f0) / 2.0;
    let b = f1 - f0 - a;
    let c = f0;
    let poly = |x: f64| a * x * x + b * x + c;
    let polynome = [f3, f7].iter().zip([3.0, 7.0]).all(|(v, x)| (v - poly(x)).abs() < 1e-6 * (1.0 + v.abs())) && f0.is_finite();
    if polynome {
        let terme = |coef: f64, puis: &str, premier: bool| -> String {
            if coef.abs() < 1e-12 {
                return String::new();
            }
            let signe = if coef < 0.0 { if premier { "-" } else { " - " } } else if premier { "" } else { " + " };
            let v = coef.abs();
            let n = if puis.is_empty() || (v - 1.0).abs() > 1e-12 { joli(v) } else { String::new() };
            format!("{signe}{n}{puis}")
        };
        let mut forme = String::new();
        for (co, p) in [(a, "x²"), (b, "x"), (c, "")] {
            let t = terme(co, p, forme.is_empty());
            forme += &t;
        }
        etapes.push(format!("On passe tout du même côté : {} = 0", if forme.is_empty() { "0".into() } else { forme }));
        if a.abs() < 1e-12 {
            if b.abs() < 1e-12 {
                etapes.push(if c.abs() < 1e-12 { "0 = 0 : toujours vrai, tous les nombres sont solutions.".into() } else { format!("{} = 0 : impossible, pas de solution.", joli(c)) });
                return Ok(etapes);
            }
            etapes.push(format!("On isole x : {}x = {}", joli(b), joli(-c)));
            etapes.push(format!("On divise par {} : x = {} / {}", joli(b), joli(-c), joli(b)));
            etapes.push(format!("✔ x = {}", joli(-c / b)));
        } else {
            let delta = b * b - 4.0 * a * c;
            etapes.push(format!("C'est du second degré : a = {}, b = {}, c = {}", joli(a), joli(b), joli(c)));
            etapes.push(format!("Discriminant : Δ = b² − 4ac = {} − {} = {}", joli(b * b), joli(4.0 * a * c), joli(delta)));
            if delta < -1e-12 {
                etapes.push("Δ < 0 : pas de solution réelle.".into());
            } else if delta.abs() <= 1e-12 {
                etapes.push(format!("Δ = 0 : une seule solution, x = −b / 2a = {}", joli(-b / (2.0 * a))));
            } else {
                let r = delta.sqrt();
                etapes.push(format!("Δ > 0 : deux solutions, x = (−b ± √Δ) / 2a  (√Δ ≈ {})", joli(r)));
                etapes.push(format!("✔ x₁ = {}   et   x₂ = {}", joli((-b - r) / (2.0 * a)), joli((-b + r) / (2.0 * a))));
            }
        }
        return Ok(etapes);
    }
    // sinon : recherche numérique des solutions entre −100 et 100
    etapes.push("Ce n'est pas un polynôme simple : je cherche les solutions par approximation.".into());
    let mut sols: Vec<f64> = Vec::new();
    let pas = 0.01;
    let mut x = -100.0;
    while x < 100.0 {
        let (u, v) = (f(x), f(x + pas));
        if u.is_finite() && v.is_finite() && (u == 0.0 || u.signum() != v.signum()) && (u - v).abs() < 1e3 {
            let (mut lo, mut hi) = (x, x + pas);
            for _ in 0..60 {
                let m = (lo + hi) / 2.0;
                if f(lo).signum() == f(m).signum() {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            if sols.last().is_none_or(|s| (s - lo).abs() > 1e-6) {
                sols.push(lo);
            }
        }
        x += pas;
    }
    if sols.is_empty() {
        etapes.push("Aucune solution trouvée entre −100 et 100.".into());
    } else {
        etapes.push(format!("✔ x ≈ {}", sols.iter().take(12).map(|s| joli((s * 10000.0).round() / 10000.0)).collect::<Vec<_>>().join(" ; ")));
    }
    Ok(etapes)
}

// --- fractions ---

fn pgcd(a: i128, b: i128) -> i128 {
    if b == 0 { a.abs() } else { pgcd(b, a % b) }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frac(pub i128, pub i128);

impl Frac {
    fn simple(n: i128, d: i128) -> Result<Frac, String> {
        if d == 0 {
            return Err("division par zéro".into());
        }
        let g = pgcd(n, d).max(1) * if d < 0 { -1 } else { 1 };
        Ok(Frac(n / g, d / g))
    }
    pub fn texte(&self) -> String {
        if self.1 == 1 { self.0.to_string() } else { format!("{}/{}", self.0, self.1) }
    }
}

/// Calcul exact avec des fractions : « 3/4 + 5/6 × 2 ».
pub fn calcul_fractions(t: &str) -> Result<Frac, String> {
    fn expr(c: &[char], i: &mut usize) -> Result<Frac, String> {
        let mut a = terme(c, i)?;
        loop {
            sauter(c, i);
            match c.get(*i) {
                Some('+') => {
                    *i += 1;
                    let b = terme(c, i)?;
                    a = Frac::simple(a.0 * b.1 + b.0 * a.1, a.1 * b.1)?;
                }
                Some('-') => {
                    *i += 1;
                    let b = terme(c, i)?;
                    a = Frac::simple(a.0 * b.1 - b.0 * a.1, a.1 * b.1)?;
                }
                _ => return Ok(a),
            }
        }
    }
    fn terme(c: &[char], i: &mut usize) -> Result<Frac, String> {
        let mut a = facteur(c, i)?;
        loop {
            sauter(c, i);
            match c.get(*i) {
                Some('*' | '×') => {
                    *i += 1;
                    let b = facteur(c, i)?;
                    a = Frac::simple(a.0 * b.0, a.1 * b.1)?;
                }
                Some('/' | '÷' | ':') => {
                    *i += 1;
                    let b = facteur(c, i)?;
                    a = Frac::simple(a.0 * b.1, a.1 * b.0)?;
                }
                _ => return Ok(a),
            }
        }
    }
    fn facteur(c: &[char], i: &mut usize) -> Result<Frac, String> {
        sauter(c, i);
        match c.get(*i) {
            Some('-') => {
                *i += 1;
                let f = facteur(c, i)?;
                Ok(Frac(-f.0, f.1))
            }
            Some('(') => {
                *i += 1;
                let e = expr(c, i)?;
                sauter(c, i);
                if c.get(*i) != Some(&')') {
                    return Err("parenthèse fermante manquante".into());
                }
                *i += 1;
                Ok(e)
            }
            Some(d) if d.is_ascii_digit() => {
                let mut n: i128 = 0;
                while let Some(d) = c.get(*i).and_then(|d| d.to_digit(10)) {
                    n = n.checked_mul(10).and_then(|n| n.checked_add(d as i128)).ok_or("nombre trop grand")?;
                    *i += 1;
                }
                // « 7/8 » écrit d'un bloc est une fraction : elle passe avant les autres opérations
                if c.get(*i) == Some(&'/') && c.get(*i + 1).is_some_and(|d| d.is_ascii_digit()) {
                    *i += 1;
                    let mut d: i128 = 0;
                    while let Some(x) = c.get(*i).and_then(|x| x.to_digit(10)) {
                        d = d.checked_mul(10).and_then(|d| d.checked_add(x as i128)).ok_or("nombre trop grand")?;
                        *i += 1;
                    }
                    return Frac::simple(n, d);
                }
                Ok(Frac(n, 1))
            }
            _ => Err("nombre attendu".into()),
        }
    }
    fn sauter(c: &[char], i: &mut usize) {
        while c.get(*i).is_some_and(|x| x.is_whitespace()) {
            *i += 1;
        }
    }
    let c: Vec<char> = t.chars().collect();
    let mut i = 0;
    let r = expr(&c, &mut i)?;
    sauter(&c, &mut i);
    if i < c.len() {
        return Err(format!("« {} » inattendu", c[i]));
    }
    Ok(r)
}

pub fn facteurs_premiers(mut n: u64) -> Vec<(u64, u32)> {
    let mut v = Vec::new();
    let mut d = 2;
    while d * d <= n {
        let mut k = 0;
        while n % d == 0 {
            n /= d;
            k += 1;
        }
        if k > 0 {
            v.push((d, k));
        }
        d += if d == 2 { 1 } else { 2 };
    }
    if n > 1 {
        v.push((n, 1));
    }
    v
}

/// Étapes de l'algorithme d'Euclide.
pub fn euclide(mut a: u64, mut b: u64) -> (u64, Vec<String>) {
    let mut v = Vec::new();
    while b != 0 {
        v.push(format!("{a} = {} × {b} + {}", a / b, a % b));
        (a, b) = (b, a % b);
    }
    (a, v)
}

pub struct Maths {
    onglet: u8,
    equation: String,
    fonctions: [String; 3],
    zoom: f64,
    centre: (f64, f64),
    fraction: String,
    nombres: (String, String),
}

impl Maths {
    pub fn new() -> Self {
        Maths { onglet: 0, equation: "x² - 5x + 6 = 0".into(), fonctions: ["x^2 - 2".into(), "sin(x)".into(), String::new()], zoom: 10.0, centre: (0.0, 0.0), fraction: "3/4 + 5/6".into(), nombres: ("84".into(), "36".into()) }
    }

    fn courbes(&mut self, ui: &mut egui::Ui) {
        let couleurs = [Color32::from_rgb(90, 170, 255), Color32::from_rgb(255, 120, 90), Color32::from_rgb(120, 220, 120)];
        let mut exprs = Vec::new();
        for (k, f) in self.fonctions.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.colored_label(couleurs[k], format!("f{}(x) =", k + 1));
                ui.add(egui::TextEdit::singleline(f).desired_width(260.0).hint_text("ex. 2x + 1"));
                if !f.trim().is_empty() {
                    match lire(f) {
                        Ok(e) => exprs.push((k, e)),
                        Err(m) => {
                            ui.colored_label(Color32::LIGHT_RED, m);
                        }
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("Zoom");
            ui.add(egui::Slider::new(&mut self.zoom, 1.0..=200.0).logarithmic(true).show_value(false));
            if ui.small_button("Recentrer").clicked() {
                self.centre = (0.0, 0.0);
                self.zoom = 10.0;
            }
            ui.weak("Glisse le graphique pour te déplacer.");
        });
        let (r, rep) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 380.0), egui::Sense::drag());
        if rep.dragged() {
            let d = rep.drag_delta();
            let u = r.width() as f64 / (2.0 * self.zoom);
            self.centre.0 -= d.x as f64 / u;
            self.centre.1 += d.y as f64 / u;
        }
        let p = ui.painter_at(r);
        p.rect_filled(r, 6.0, Color32::from_rgb(20, 22, 30));
        let u = r.width() as f64 / (2.0 * self.zoom);
        let ecran = |x: f64, y: f64| Pos2::new(r.center().x + ((x - self.centre.0) * u) as f32, r.center().y - ((y - self.centre.1) * u) as f32);
        let (xmin, xmax) = (self.centre.0 - self.zoom, self.centre.0 + self.zoom);
        let hy = r.height() as f64 / (2.0 * u);
        let (ymin, ymax) = (self.centre.1 - hy, self.centre.1 + hy);
        // quadrillage
        let pas = 10f64.powf((self.zoom / 5.0).log10().floor());
        let mut g = (xmin / pas).floor() * pas;
        while g <= xmax {
            let c = if g.abs() < pas / 2.0 { Color32::from_gray(150) } else { Color32::from_gray(45) };
            p.line_segment([ecran(g, ymin), ecran(g, ymax)], Stroke::new(1.0_f32, c));
            if g.abs() > pas / 2.0 {
                p.text(ecran(g, 0.0) + egui::vec2(2.0, 2.0), egui::Align2::LEFT_TOP, joli((g * 1e6).round() / 1e6), FontId::proportional(10.0), Color32::from_gray(120));
            }
            g += pas;
        }
        let mut g = (ymin / pas).floor() * pas;
        while g <= ymax {
            let c = if g.abs() < pas / 2.0 { Color32::from_gray(150) } else { Color32::from_gray(45) };
            p.line_segment([ecran(xmin, g), ecran(xmax, g)], Stroke::new(1.0_f32, c));
            if g.abs() > pas / 2.0 {
                p.text(ecran(0.0, g) + egui::vec2(3.0, 0.0), egui::Align2::LEFT_CENTER, joli((g * 1e6).round() / 1e6), FontId::proportional(10.0), Color32::from_gray(120));
            }
            g += pas;
        }
        for (k, e) in &exprs {
            let mut morceau: Vec<Pos2> = Vec::new();
            let n = r.width() as usize;
            for i in 0..=n {
                let x = xmin + (xmax - xmin) * i as f64 / n as f64;
                let y = e.eval(x);
                if y.is_finite() && y > ymin - (ymax - ymin) && y < ymax + (ymax - ymin) {
                    morceau.push(ecran(x, y));
                } else if morceau.len() > 1 {
                    p.add(egui::Shape::line(std::mem::take(&mut morceau), Stroke::new(2.5_f32, couleurs[*k])));
                } else {
                    morceau.clear();
                }
            }
            if morceau.len() > 1 {
                p.add(egui::Shape::line(morceau, Stroke::new(2.5_f32, couleurs[*k])));
            }
        }
        if let Some(h) = rep.hover_pos() {
            let x = self.centre.0 + (h.x - r.center().x) as f64 / u;
            let mut t = format!("x = {}", joli((x * 100.0).round() / 100.0));
            for (k, e) in &exprs {
                t += &format!("   f{}(x) = {}", k + 1, joli((e.eval(x) * 1000.0).round() / 1000.0));
            }
            p.text(r.left_top() + egui::vec2(8.0, 8.0), egui::Align2::LEFT_TOP, t, FontId::proportional(13.0), Color32::WHITE);
        }
        // tableau de valeurs
        if let Some((k, e)) = exprs.first() {
            ui.label(RichText::new(format!("Tableau de valeurs de f{}", k + 1)).strong());
            egui::Grid::new("valeurs").striped(true).show(ui, |ui| {
                ui.label("x");
                for x in -3..=3 {
                    ui.label(x.to_string());
                }
                ui.end_row();
                ui.label("f(x)");
                for x in -3..=3 {
                    ui.label(joli((e.eval(x as f64) * 1000.0).round() / 1000.0));
                }
            });
        }
    }
}

impl Outil for Maths {
    fn nom(&self) -> &'static str {
        "🧮 Aide aux maths"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Aide aux maths");
        ui.horizontal(|ui| {
            for (k, n) in ["📝 Équations", "📈 Courbes", "➗ Fractions", "🔢 Nombres"].iter().enumerate() {
                ui.selectable_value(&mut self.onglet, k as u8, *n);
            }
        });
        ui.separator();
        match self.onglet {
            0 => {
                ui.weak("Écris une équation avec x : 3x + 5 = 20, x² − 4 = 0, 2(x − 1) = x + 7, sqrt(x) = 3…");
                ui.add(egui::TextEdit::singleline(&mut self.equation).font(egui::TextStyle::Heading).desired_width(f32::INFINITY));
                match resoudre(&self.equation) {
                    Ok(e) => {
                        for (k, l) in e.iter().enumerate() {
                            let t = RichText::new(format!("{}. {l}", k + 1)).size(17.0);
                            ui.label(if l.starts_with('✔') { t.strong().color(Color32::LIGHT_GREEN) } else { t });
                        }
                    }
                    Err(m) => {
                        ui.colored_label(Color32::LIGHT_RED, m);
                    }
                }
            }
            1 => self.courbes(ui),
            2 => {
                ui.weak("Calcul exact avec des fractions : 3/4 + 5/6, (2/3 − 1/4) × 6, 7/8 : 1/2…");
                ui.add(egui::TextEdit::singleline(&mut self.fraction).font(egui::TextStyle::Heading).desired_width(f32::INFINITY));
                match calcul_fractions(&self.fraction) {
                    Ok(f) => {
                        ui.label(RichText::new(format!("= {}", f.texte())).size(26.0).strong());
                        if f.1 != 1 {
                            let ent = f.0 / f.1;
                            if ent != 0 {
                                ui.label(format!("= {ent} et {}/{}", (f.0 % f.1).abs(), f.1));
                            }
                            ui.label(format!("≈ {}", joli((f.0 as f64 / f.1 as f64 * 10000.0).round() / 10000.0)));
                        }
                    }
                    Err(m) => {
                        ui.colored_label(Color32::LIGHT_RED, m);
                    }
                }
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.label("a =");
                    ui.add(egui::TextEdit::singleline(&mut self.nombres.0).desired_width(120.0));
                    ui.label("b =");
                    ui.add(egui::TextEdit::singleline(&mut self.nombres.1).desired_width(120.0));
                });
                let a = self.nombres.0.trim().parse::<u64>().ok().filter(|n| *n > 0 && *n < 1_000_000_000_000);
                let b = self.nombres.1.trim().parse::<u64>().ok().filter(|n| *n > 0 && *n < 1_000_000_000_000);
                for (n, v) in [("a", a), ("b", b)] {
                    if let Some(v) = v {
                        let f = facteurs_premiers(v);
                        let t = f.iter().map(|(p, k)| if *k > 1 { format!("{p}^{k}") } else { p.to_string() }).collect::<Vec<_>>().join(" × ");
                        ui.label(format!("{n} = {v} = {t}{}", if f.len() == 1 && f[0].1 == 1 { "   (nombre premier !)" } else { "" }));
                    }
                }
                if let (Some(a), Some(b)) = (a, b) {
                    let (g, etapes) = euclide(a.max(b), a.min(b));
                    ui.separator();
                    ui.strong("PGCD par l'algorithme d'Euclide :");
                    for e in etapes {
                        ui.monospace(e);
                    }
                    ui.label(RichText::new(format!("PGCD({a}, {b}) = {g}    PPCM({a}, {b}) = {}", a / g * b)).size(18.0).strong());
                    if g > 1 {
                        ui.label(format!("{a}/{b} se simplifie en {}/{}", a / g, b / g));
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 12 · Tableau périodique
// ---------------------------------------------------------------------------------------------------

/// (symbole, nom, masse molaire)
pub const ELEMENTS: [(&str, &str, f64); 118] = [
    ("H", "Hydrogène", 1.008), ("He", "Hélium", 4.0026), ("Li", "Lithium", 6.94), ("Be", "Béryllium", 9.0122), ("B", "Bore", 10.81), ("C", "Carbone", 12.011), ("N", "Azote", 14.007), ("O", "Oxygène", 15.999), ("F", "Fluor", 18.998), ("Ne", "Néon", 20.180),
    ("Na", "Sodium", 22.990), ("Mg", "Magnésium", 24.305), ("Al", "Aluminium", 26.982), ("Si", "Silicium", 28.085), ("P", "Phosphore", 30.974), ("S", "Soufre", 32.06), ("Cl", "Chlore", 35.45), ("Ar", "Argon", 39.948), ("K", "Potassium", 39.098), ("Ca", "Calcium", 40.078),
    ("Sc", "Scandium", 44.956), ("Ti", "Titane", 47.867), ("V", "Vanadium", 50.942), ("Cr", "Chrome", 51.996), ("Mn", "Manganèse", 54.938), ("Fe", "Fer", 55.845), ("Co", "Cobalt", 58.933), ("Ni", "Nickel", 58.693), ("Cu", "Cuivre", 63.546), ("Zn", "Zinc", 65.38),
    ("Ga", "Gallium", 69.723), ("Ge", "Germanium", 72.630), ("As", "Arsenic", 74.922), ("Se", "Sélénium", 78.971), ("Br", "Brome", 79.904), ("Kr", "Krypton", 83.798), ("Rb", "Rubidium", 85.468), ("Sr", "Strontium", 87.62), ("Y", "Yttrium", 88.906), ("Zr", "Zirconium", 91.224),
    ("Nb", "Niobium", 92.906), ("Mo", "Molybdène", 95.95), ("Tc", "Technétium", 98.0), ("Ru", "Ruthénium", 101.07), ("Rh", "Rhodium", 102.91), ("Pd", "Palladium", 106.42), ("Ag", "Argent", 107.87), ("Cd", "Cadmium", 112.41), ("In", "Indium", 114.82), ("Sn", "Étain", 118.71),
    ("Sb", "Antimoine", 121.76), ("Te", "Tellure", 127.60), ("I", "Iode", 126.90), ("Xe", "Xénon", 131.29), ("Cs", "Césium", 132.91), ("Ba", "Baryum", 137.33), ("La", "Lanthane", 138.91), ("Ce", "Cérium", 140.12), ("Pr", "Praséodyme", 140.91), ("Nd", "Néodyme", 144.24),
    ("Pm", "Prométhium", 145.0), ("Sm", "Samarium", 150.36), ("Eu", "Europium", 151.96), ("Gd", "Gadolinium", 157.25), ("Tb", "Terbium", 158.93), ("Dy", "Dysprosium", 162.50), ("Ho", "Holmium", 164.93), ("Er", "Erbium", 167.26), ("Tm", "Thulium", 168.93), ("Yb", "Ytterbium", 173.05),
    ("Lu", "Lutécium", 174.97), ("Hf", "Hafnium", 178.49), ("Ta", "Tantale", 180.95), ("W", "Tungstène", 183.84), ("Re", "Rhénium", 186.21), ("Os", "Osmium", 190.23), ("Ir", "Iridium", 192.22), ("Pt", "Platine", 195.08), ("Au", "Or", 196.97), ("Hg", "Mercure", 200.59),
    ("Tl", "Thallium", 204.38), ("Pb", "Plomb", 207.2), ("Bi", "Bismuth", 208.98), ("Po", "Polonium", 209.0), ("At", "Astate", 210.0), ("Rn", "Radon", 222.0), ("Fr", "Francium", 223.0), ("Ra", "Radium", 226.0), ("Ac", "Actinium", 227.0), ("Th", "Thorium", 232.04),
    ("Pa", "Protactinium", 231.04), ("U", "Uranium", 238.03), ("Np", "Neptunium", 237.0), ("Pu", "Plutonium", 244.0), ("Am", "Américium", 243.0), ("Cm", "Curium", 247.0), ("Bk", "Berkélium", 247.0), ("Cf", "Californium", 251.0), ("Es", "Einsteinium", 252.0), ("Fm", "Fermium", 257.0),
    ("Md", "Mendélévium", 258.0), ("No", "Nobélium", 259.0), ("Lr", "Lawrencium", 266.0), ("Rf", "Rutherfordium", 267.0), ("Db", "Dubnium", 268.0), ("Sg", "Seaborgium", 269.0), ("Bh", "Bohrium", 270.0), ("Hs", "Hassium", 277.0), ("Mt", "Meitnérium", 278.0), ("Ds", "Darmstadtium", 281.0),
    ("Rg", "Roentgenium", 282.0), ("Cn", "Copernicium", 285.0), ("Nh", "Nihonium", 286.0), ("Fl", "Flérovium", 289.0), ("Mc", "Moscovium", 290.0), ("Lv", "Livermorium", 293.0), ("Ts", "Tennesse", 294.0), ("Og", "Oganesson", 294.0),
];

pub const FAMILLES: [(&str, [u8; 3]); 10] = [
    ("Métaux alcalins", [235, 90, 80]),
    ("Alcalino-terreux", [240, 160, 70]),
    ("Métaux de transition", [230, 200, 90]),
    ("Métaux pauvres", [140, 190, 140]),
    ("Métalloïdes", [100, 180, 170]),
    ("Non-métaux", [110, 160, 230]),
    ("Halogènes", [160, 130, 230]),
    ("Gaz nobles", [220, 120, 200]),
    ("Lanthanides", [200, 150, 120]),
    ("Actinides", [190, 120, 140]),
];

pub fn famille(z: u32) -> usize {
    match z {
        3 | 11 | 19 | 37 | 55 | 87 => 0,
        4 | 12 | 20 | 38 | 56 | 88 => 1,
        57..=71 => 8,
        89..=103 => 9,
        21..=30 | 39..=48 | 72..=80 | 104..=112 => 2,
        13 | 31 | 49 | 50 | 81..=84 | 113..=116 => 3,
        5 | 14 | 32 | 33 | 51 | 52 => 4,
        9 | 17 | 35 | 53 | 85 | 117 => 6,
        2 | 10 | 18 | 36 | 54 | 86 | 118 => 7,
        _ => 5,
    }
}

/// (période, groupe) ; les lanthanides et actinides ont le groupe 3 et une ligne à part.
pub fn position(z: u32) -> (u32, u32) {
    let fins = [2, 10, 18, 36, 54, 86, 118];
    let periode = fins.iter().position(|f| z <= *f).unwrap() as u32 + 1;
    let debut = if periode == 1 { 0 } else { fins[periode as usize - 2] };
    let k = z - debut;
    let groupe = match periode {
        1 => if z == 1 { 1 } else { 18 },
        2 | 3 => if k <= 2 { k } else { k + 10 },
        4 | 5 => k,
        _ => if k <= 2 { k } else if k <= 17 { 3 } else { k - 14 },
    };
    (periode, groupe)
}

/// Case dans la grille affichée (ligne, colonne), lanthanides et actinides en dessous.
pub fn case(z: u32) -> (u32, u32) {
    match z {
        57..=71 => (8, z - 57 + 3),
        89..=103 => (9, z - 89 + 3),
        _ => {
            let (p, g) = position(z);
            (p - 1, g - 1)
        }
    }
}

pub fn etat(z: u32) -> &'static str {
    match z {
        35 | 80 => "liquide",
        1 | 2 | 7 | 8 | 9 | 10 | 17 | 18 | 36 | 54 | 86 => "gaz",
        _ => "solide",
    }
}

/// Configuration électronique (règle de Klechkowski, sans les quelques exceptions).
pub fn configuration(z: u32) -> String {
    const ORDRE: [(&str, u32); 19] = [("1s", 2), ("2s", 2), ("2p", 6), ("3s", 2), ("3p", 6), ("4s", 2), ("3d", 10), ("4p", 6), ("5s", 2), ("4d", 10), ("5p", 6), ("6s", 2), ("4f", 14), ("5d", 10), ("6p", 6), ("7s", 2), ("5f", 14), ("6d", 10), ("7p", 6)];
    let mut reste = z;
    let mut v = Vec::new();
    for (n, c) in ORDRE {
        if reste == 0 {
            break;
        }
        let e = reste.min(c);
        v.push(format!("{n}{}", exposant(e)));
        reste -= e;
    }
    v.join(" ")
}

fn exposant(n: u32) -> String {
    n.to_string().chars().map(|c| ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'][c.to_digit(10).unwrap() as usize]).collect()
}

/// Masse molaire d'une formule : H2O, Ca(OH)2, CuSO4·5H2O…
pub fn masse_molaire(f: &str) -> Result<(f64, Vec<(String, u32)>), String> {
    fn groupe(c: &[char], i: &mut usize, compte: &mut Vec<(String, u32)>, mult: u32) -> Result<(), String> {
        while *i < c.len() {
            let ch = c[*i];
            if ch == '(' || ch == '[' {
                *i += 1;
                let mut sous = Vec::new();
                groupe(c, i, &mut sous, 1)?;
                if *i >= c.len() || (c[*i] != ')' && c[*i] != ']') {
                    return Err("parenthèse non fermée".into());
                }
                *i += 1;
                let n = nombre(c, i).unwrap_or(1);
                for (e, k) in sous {
                    ajouter(compte, &e, k * n * mult);
                }
            } else if ch == ')' || ch == ']' {
                return Ok(());
            } else if ch.is_ascii_uppercase() {
                let mut s = ch.to_string();
                *i += 1;
                while *i < c.len() && c[*i].is_ascii_lowercase() {
                    s.push(c[*i]);
                    *i += 1;
                }
                if !ELEMENTS.iter().any(|e| e.0 == s) {
                    return Err(format!("élément « {s} » inconnu"));
                }
                let n = nombre(c, i).unwrap_or(1);
                ajouter(compte, &s, n * mult);
            } else if ch.is_whitespace() {
                *i += 1;
            } else {
                return Err(format!("« {ch} » inattendu (les symboles commencent par une majuscule)"));
            }
        }
        Ok(())
    }
    fn nombre(c: &[char], i: &mut usize) -> Option<u32> {
        let d = *i;
        while *i < c.len() && c[*i].is_ascii_digit() {
            *i += 1;
        }
        c[d..*i].iter().collect::<String>().parse().ok()
    }
    fn ajouter(v: &mut Vec<(String, u32)>, e: &str, n: u32) {
        match v.iter_mut().find(|x| x.0 == e) {
            Some(x) => x.1 += n,
            None => v.push((e.to_string(), n)),
        }
    }
    let mut total = Vec::new();
    for partie in f.split(['·', '.', '*']) {
        let c: Vec<char> = partie.trim().chars().collect();
        let mut i = 0;
        let k = nombre(&c, &mut i).unwrap_or(1);
        let mut sous = Vec::new();
        groupe(&c, &mut i, &mut sous, 1)?;
        if i < c.len() {
            return Err("parenthèse fermante en trop".into());
        }
        for (e, n) in sous {
            ajouter(&mut total, &e, n * k);
        }
    }
    if total.is_empty() {
        return Err("formule vide".into());
    }
    let m = total.iter().map(|(e, n)| ELEMENTS.iter().find(|x| x.0 == e).map(|x| x.2).unwrap_or(0.0) * *n as f64).sum();
    Ok((m, total))
}

pub struct Periodique {
    choisi: u32,
    formule: String,
    quiz: Option<(u32, Vec<u32>, Option<u32>)>,
    points: (u32, u32),
}

impl Periodique {
    pub fn new() -> Self {
        Periodique { choisi: 6, formule: "H2O".into(), quiz: None, points: (0, 0) }
    }

    fn nouvelle_question(&mut self) {
        let z = 1 + crate::outil::hasard(56) as u32; // les 56 premiers : les plus connus
        let mut choix = vec![z];
        while choix.len() < 4 {
            let c = 1 + crate::outil::hasard(56) as u32;
            if !choix.contains(&c) {
                choix.push(c);
            }
        }
        let k = crate::outil::hasard(4) as usize;
        choix.swap(0, k);
        self.quiz = Some((z, choix, None));
    }
}

impl Outil for Periodique {
    fn nom(&self) -> &'static str {
        "🧪 Tableau périodique"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tableau périodique");
        let cote = ((ui.available_width() - 20.0) / 18.0).clamp(26.0, 52.0);
        let (r, _) = ui.allocate_exact_size(egui::vec2(cote * 18.0, cote * 10.3), egui::Sense::hover());
        let p = ui.painter_at(r);
        for z in 1..=118u32 {
            let (l, c) = case(z);
            let y = r.top() + l as f32 * cote + if l >= 8 { cote * 0.3 } else { 0.0 };
            let rect = egui::Rect::from_min_size(Pos2::new(r.left() + c as f32 * cote, y), egui::vec2(cote - 2.0, cote - 2.0));
            let f = FAMILLES[famille(z)].1;
            let choisi = z == self.choisi;
            p.rect_filled(rect, 3.0, Color32::from_rgb(f[0], f[1], f[2]).gamma_multiply(if choisi { 1.0 } else { 0.75 }));
            if choisi {
                p.rect_stroke(rect, 3.0, Stroke::new(2.5_f32, Color32::WHITE), egui::StrokeKind::Outside);
            }
            p.text(rect.center() + egui::vec2(0.0, cote * 0.08), egui::Align2::CENTER_CENTER, ELEMENTS[z as usize - 1].0, FontId::proportional(cote * 0.38), Color32::BLACK);
            p.text(rect.left_top() + egui::vec2(2.0, 1.0), egui::Align2::LEFT_TOP, z.to_string(), FontId::proportional(cote * 0.2), Color32::from_black_alpha(180));
            let rep = ui.interact(rect, ui.id().with(("el", z)), egui::Sense::click());
            if rep.clicked() {
                self.choisi = z;
            }
            rep.on_hover_text(ELEMENTS[z as usize - 1].1);
        }
        let z = self.choisi;
        let (s, n, m) = ELEMENTS[z as usize - 1];
        let (per, gr) = position(z);
        ui.horizontal_wrapped(|ui| {
            let f = FAMILLES[famille(z)];
            ui.label(RichText::new(format!("{s} — {n}")).size(24.0).strong());
            ui.colored_label(Color32::from_rgb(f.1[0], f.1[1], f.1[2]), f.0);
        });
        ui.label(format!("Numéro atomique {z} · masse molaire {} g/mol · période {per}, groupe {gr} · {} à 25 °C", joli((m * 1000.0).round() / 1000.0), etat(z)));
        ui.label(format!("{z} protons, {z} électrons, environ {} neutrons · configuration : {}", (m.round() as i64 - z as i64).max(0), configuration(z)));
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("⚖ Masse molaire");
            ui.add(egui::TextEdit::singleline(&mut self.formule).desired_width(180.0).hint_text("H2SO4, Ca(OH)2…"));
            match masse_molaire(&self.formule) {
                Ok((m, d)) => {
                    ui.label(RichText::new(format!("= {} g/mol", joli((m * 1000.0).round() / 1000.0))).strong());
                    ui.weak(d.iter().map(|(e, n)| format!("{e}×{n}")).collect::<Vec<_>>().join(" + "));
                }
                Err(e) => {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong("🎯 Quiz");
            if ui.button(if self.quiz.is_none() { "Commencer" } else { "Question suivante" }).clicked() {
                self.nouvelle_question();
            }
            if self.points.1 > 0 {
                ui.label(format!("{} / {}", self.points.0, self.points.1));
            }
        });
        if let Some((bon, choix, reponse)) = self.quiz.clone() {
            ui.label(format!("Quel est le symbole de « {} » ?", ELEMENTS[bon as usize - 1].1));
            ui.horizontal(|ui| {
                for c in &choix {
                    let mut b = egui::Button::new(RichText::new(ELEMENTS[*c as usize - 1].0).size(20.0)).min_size(egui::vec2(60.0, 40.0));
                    if let Some(r) = reponse {
                        if *c == bon {
                            b = b.fill(Color32::from_rgb(40, 140, 60));
                        } else if *c == r {
                            b = b.fill(Color32::from_rgb(160, 50, 50));
                        }
                    }
                    if ui.add(b).clicked() && reponse.is_none() {
                        self.points.1 += 1;
                        if *c == bon {
                            self.points.0 += 1;
                        }
                        self.quiz = Some((bon, choix.clone(), Some(*c)));
                        self.choisi = bon;
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equations() {
        let e = resoudre("3x + 5 = 20").unwrap();
        assert!(e.last().unwrap().contains("x = 5"), "{e:?}");
        let e = resoudre("x² - 5x + 6 = 0").unwrap();
        assert!(e.last().unwrap().contains("x₁ = 2") && e.last().unwrap().contains("x₂ = 3"), "{e:?}");
        let e = resoudre("2(x - 1) = x + 7").unwrap();
        assert!(e.last().unwrap().contains("x = 9"), "{e:?}");
        assert!(resoudre("x^2 + 1 = 0").unwrap().last().unwrap().contains("pas de solution"));
        let e = resoudre("sqrt(x) = 3").unwrap();
        assert!(e.last().unwrap().contains("≈ 9"), "{e:?}");
        assert!(resoudre("2x = 1").unwrap().last().unwrap().contains("1/2"));
        assert!(lire("2x(x+1)").is_ok() && lire("y + 1").is_err());
        assert!((lire("2sin(pi/2) + 3^2").unwrap().eval(0.0) - 11.0).abs() < 1e-9);
        assert_eq!(calcul_fractions("3/4 + 5/6").unwrap(), Frac(19, 12));
        assert_eq!(calcul_fractions("(2/3 - 1/4) * 6").unwrap(), Frac(5, 2));
        assert_eq!(calcul_fractions("7/8 : 1/2").unwrap(), Frac(7, 4));
        assert!(calcul_fractions("1/0").is_err());
        assert_eq!(facteurs_premiers(84), vec![(2, 2), (3, 1), (7, 1)]);
        assert_eq!(euclide(84, 36).0, 12);
    }

    #[test]
    fn periodique() {
        assert_eq!(position(1), (1, 1));
        assert_eq!(position(2), (1, 18));
        assert_eq!(position(8), (2, 16));
        assert_eq!(position(26), (4, 8));
        assert_eq!(position(79), (6, 11));
        assert_eq!(position(86), (6, 18));
        assert_eq!(position(118), (7, 18));
        assert_eq!(case(58), (8, 4));
        // toutes les cases sont différentes
        let mut v: Vec<(u32, u32)> = (1..=118).map(case).collect();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), 118);
        assert_eq!(configuration(8), "1s² 2s² 2p⁴");
        let (m, _) = masse_molaire("H2O").unwrap();
        assert!((m - 18.015).abs() < 0.01);
        let (m, _) = masse_molaire("Ca(OH)2").unwrap();
        assert!((m - 74.09).abs() < 0.02);
        let (m, _) = masse_molaire("CuSO4·5H2O").unwrap();
        assert!((m - 249.68).abs() < 0.05, "{m}");
        assert!(masse_molaire("Xy2").is_err());
        assert_eq!(famille(26), 2);
        assert_eq!(ELEMENTS[25].0, "Fe");
        assert_eq!(ELEMENTS[78].0, "Au");
        assert_eq!(ELEMENTS[117].0, "Og");
    }
}
