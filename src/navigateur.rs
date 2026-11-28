//! Navigateur intégré, le plus léger possible : il utilise le moteur web déjà présent dans Windows
//! (WebView2, celui d'Edge) au lieu d'embarquer un navigateur entier.
//! - La page « nouvel onglet » est dessinée par Outils : aucun moteur web tant qu'on n'ouvre pas de site.
//! - Les onglets en arrière-plan passent en mémoire réduite, puis s'endorment après 10 minutes
//!   (le moteur est libéré, la page revient quand on rouvre l'onglet).
//! - Bloqueur de pubs et de pisteurs : seules les requêtes vers ces domaines sont interceptées,
//!   le reste de la navigation n'est pas ralenti.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wry::{WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};

/// Domaines de pubs et de pisteurs bloqués.
pub const BLOQUES: &[&str] = &[
    "doubleclick.net", "googlesyndication.com", "googleadservices.com", "google-analytics.com", "googletagmanager.com", "googletagservices.com", "adservice.google.com",
    "adnxs.com", "adsrvr.org", "amazon-adsystem.com", "criteo.com", "criteo.net", "taboola.com", "outbrain.com", "scorecardresearch.com", "quantserve.com", "moatads.com",
    "rubiconproject.com", "pubmatic.com", "openx.net", "casalemedia.com", "smartadserver.com", "adform.net", "teads.tv", "yieldmo.com", "bidswitch.net", "3lift.com",
    "sharethrough.com", "media.net", "zedo.com", "popads.net", "propellerads.com", "exoclick.com", "hotjar.com", "mixpanel.com", "clarity.ms", "doubleverify.com",
    "adsafeprotected.com", "adcolony.com", "applovin.com", "chartbeat.com", "advertising.com", "ads.yahoo.com", "adroll.com", "outbrainimg.com", "mgid.com",
    "revcontent.com", "onetag-sys.com", "smilewanted.com", "seedtag.com", "sascdn.com", "ayads.co", "adotmob.com", "weborama.fr", "xiti.com", "at-internet.com",
    "googletag.com", "pagead2.googlesyndication.com", "securepubads.g.doubleclick.net", "stats.g.doubleclick.net", "bat.bing.com", "ads.linkedin.com", "analytics.tiktok.com",
    "ads-twitter.com", "static.ads-twitter.com", "an.facebook.com", "pixel.facebook.com",
];

/// Le domaine (ou un de ses parents) est-il bloqué ?
pub fn bloque(url: &str) -> bool {
    let hote = url.split("://").nth(1).unwrap_or(url).split(['/', ':', '?', '#']).next().unwrap_or("").to_lowercase();
    BLOQUES.iter().any(|d| hote == *d || hote.ends_with(&format!(".{d}")))
}

pub const MOTEURS: [(&str, &str); 5] = [
    ("DuckDuckGo", "https://duckduckgo.com/?q="),
    ("Google", "https://www.google.com/search?q="),
    ("Bing", "https://www.bing.com/search?q="),
    ("Qwant", "https://www.qwant.com/?q="),
    ("Ecosia", "https://www.ecosia.org/search?q="),
];

/// Ce qu'on tape dans la barre d'adresse : une adresse, ou une recherche.
pub fn vers_url(saisie: &str, moteur: usize) -> String {
    let t = saisie.trim();
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("file://") {
        return t.to_string();
    }
    let ressemble = !t.contains(' ') && (t.contains('.') && !t.ends_with('.') || t.starts_with("localhost"));
    if ressemble {
        return format!("https://{t}");
    }
    let q: String = t.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b' ' => "+".into(),
        _ => format!("%{b:02X}"),
    }).collect();
    format!("{}{q}", MOTEURS[moteur.min(MOTEURS.len() - 1)].1)
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub moteur: usize,
    pub bloqueur: bool,
    pub favoris: Vec<(String, String)>, // titre, adresse
    pub historique: Vec<(String, String)>,
    pub sombre: bool,
    pub endormir_min: u64,
}

impl Default for Reglages {
    fn default() -> Self {
        let f = |t: &str, u: &str| (t.to_string(), u.to_string());
        Reglages {
            moteur: 0,
            bloqueur: true,
            favoris: vec![f("YouTube", "https://www.youtube.com"), f("Wikipédia", "https://fr.wikipedia.org"), f("Twitch", "https://www.twitch.tv"), f("Minecraft Wiki", "https://fr.minecraft.wiki")],
            historique: Vec::new(),
            sombre: true,
            endormir_min: 10,
        }
    }
}

/// Ce que les fonctions de rappel du moteur remontent à l'onglet.
#[derive(Default)]
struct Infos {
    titre: String,
    url: String,
    charge: bool,
    nouveaux: Vec<String>, // liens « ouvrir dans un nouvel onglet »
    fini: Vec<(String, String)>, // pages chargées (pour l'historique)
}

struct Onglet {
    vue: Option<WebView>,
    infos: Rc<RefCell<Infos>>,
    adresse: String,
    prive: bool,
    vu: Instant,
    economie: bool,
}

impl Onglet {
    fn nouveau(prive: bool) -> Self {
        Onglet { vue: None, infos: Rc::default(), adresse: String::new(), prive, vu: Instant::now(), economie: false }
    }
    fn titre(&self) -> String {
        let i = self.infos.borrow();
        let t = if !i.titre.is_empty() { i.titre.clone() } else if !i.url.is_empty() { i.url.clone() } else { "Nouvel onglet".into() };
        let t: String = t.chars().take(22).collect();
        if self.prive { format!("🕶 {t}") } else { t }
    }
}

/// Fenêtre d'Outils (le moteur s'y accroche comme fenêtre enfant).
struct Parent(isize);

impl wry::raw_window_handle::HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<wry::raw_window_handle::WindowHandle<'_>, wry::raw_window_handle::HandleError> {
        use wry::raw_window_handle::{RawWindowHandle, Win32WindowHandle, WindowHandle};
        let n = std::num::NonZeroIsize::new(self.0).ok_or(wry::raw_window_handle::HandleError::Unavailable)?;
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(n))) })
    }
}

fn fenetre_outils() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
    let titre: Vec<u16> = "Outils".encode_utf16().chain([0]).collect();
    let h = unsafe { FindWindowW(std::ptr::null(), titre.as_ptr()) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, &mut pid) };
    (pid == std::process::id() && !h.is_null()).then_some(h as isize)
}

/// Bloque les pubs : un filtre par domaine, pour que seules ces requêtes passent par Outils.
fn installer_bloqueur(vue: &WebView, compteur: Arc<AtomicU32>) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    let wv = vue.webview();
    let env = vue.environment();
    unsafe {
        for d in BLOQUES {
            for motif in [format!("*://{d}/*"), format!("*://*.{d}/*")] {
                let m: Vec<u16> = motif.encode_utf16().chain([0]).collect();
                let _ = wv.AddWebResourceRequestedFilter(windows_core::PCWSTR(m.as_ptr()), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL);
            }
        }
        let gestion = webview2_com::WebResourceRequestedEventHandler::create(Box::new(move |_v, args| {
            let Some(args) = args else { return Ok(()) };
            let mut p = windows_core::PWSTR::null();
            args.Request()?.Uri(&mut p)?;
            let uri = webview2_com::take_pwstr(p);
            if bloque(&uri) {
                let r = env.CreateWebResourceResponse(None, 403, windows_core::w!("Bloqué par Outils"), windows_core::w!(""))?;
                args.SetResponse(&r)?;
                compteur.fetch_add(1, Relaxed);
            }
            Ok(())
        }));
        let mut jeton = 0i64;
        let _ = wv.add_WebResourceRequested(&gestion, &mut jeton);
    }
}

pub struct Navigateur {
    r: Reglages,
    onglets: Vec<Onglet>,
    actif: usize,
    contexte: Option<wry::WebContext>,
    passe_vue: u64,
    bloques: Arc<AtomicU32>,
    erreur: String,
    derniere_url: Instant,
    panneau_ext: bool,
    ext_profil: Rc<RefCell<Vec<(String, String, bool)>>>, // id, nom, active (vu par le moteur)
    ext_saisie: String,
    ext_msg: Arc<std::sync::Mutex<String>>,
    ext_a_ajouter: Arc<std::sync::Mutex<Vec<std::path::PathBuf>>>,
    /// relevé automatique des produits d'un rayon (pour le catalogue de prix)
    releve: Option<Releve>,
}

#[derive(Clone, Copy, PartialEq)]
enum EtapeReleve {
    Defiler,
    Lire,
    Suivant,
}

/// Relevé d'un rayon : on descend la page, on lit les produits, on passe à la page suivante.
struct Releve {
    etape: EtapeReleve,
    pages: usize,
    max_pages: usize,
    defilements: usize,
    attendre: Instant,
    reponse: Arc<std::sync::Mutex<Option<String>>>,
    attente: Option<Instant>,
    urls: std::collections::HashSet<String>,
    vides: usize,
    pause: bool,
    magasin: String,
}

// ---------------------------------------------------------------------------------------------------
// Extensions (format Chrome) : les dossiers de « extensions » sont chargés par le moteur ;
// une extension désactivée est rangée dans « extensions_off » et retirée du moteur.
// ---------------------------------------------------------------------------------------------------

fn dossier_ext() -> std::path::PathBuf {
    crate::outil::base().join("navigateur").join("extensions")
}

fn dossier_off() -> std::path::PathBuf {
    crate::outil::base().join("navigateur").join("extensions_off")
}

fn profil(v: &WebView) -> windows_core::Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Profile7> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows_core::Interface;
    unsafe { v.webview().cast::<ICoreWebView2_13>()?.Profile()?.cast::<ICoreWebView2Profile7>() }
}

/// Liste des extensions vues par le moteur (id, nom, active), rangée dans `liste` quand elle arrive.
fn lister_ext(v: &WebView, liste: Rc<RefCell<Vec<(String, String, bool)>>>) {
    let Ok(p) = profil(v) else { return };
    let h = webview2_com::ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_, l| {
        let Some(l) = l else { return Ok(()) };
        let mut n = 0u32;
        let mut v = Vec::new();
        unsafe {
            l.Count(&mut n)?;
            for i in 0..n {
                let e = l.GetValueAtIndex(i)?;
                let (mut a, mut b) = (windows_core::PWSTR::null(), windows_core::PWSTR::null());
                e.Id(&mut a)?;
                e.Name(&mut b)?;
                let mut actif = windows_core::BOOL::default();
                e.IsEnabled(&mut actif)?;
                v.push((webview2_com::take_pwstr(a), webview2_com::take_pwstr(b), actif.as_bool()));
            }
        }
        *liste.borrow_mut() = v;
        Ok(())
    }));
    unsafe {
        let _ = p.GetBrowserExtensions(&h);
    }
}

fn ajouter_ext(v: &WebView, dossier: &std::path::Path) {
    let Ok(p) = profil(v) else { return };
    let chemin = windows_core::HSTRING::from(dossier.to_string_lossy().as_ref());
    let h = webview2_com::ProfileAddBrowserExtensionCompletedHandler::create(Box::new(|_, _| Ok(())));
    unsafe {
        let _ = p.AddBrowserExtension(&chemin, &h);
    }
}

/// Retire du moteur l'extension d'identifiant `id` (ou, à défaut, de ce nom).
fn retirer_ext(v: &WebView, id: String, nom: String) {
    let Ok(p) = profil(v) else { return };
    let h = webview2_com::ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_, l| {
        let Some(l) = l else { return Ok(()) };
        let mut n = 0u32;
        unsafe {
            l.Count(&mut n)?;
            for i in 0..n {
                let e = l.GetValueAtIndex(i)?;
                let (mut a, mut b) = (windows_core::PWSTR::null(), windows_core::PWSTR::null());
                e.Id(&mut a)?;
                e.Name(&mut b)?;
                let (ei, en) = (webview2_com::take_pwstr(a), webview2_com::take_pwstr(b));
                if ei == id || en == nom {
                    let _ = e.Remove(&webview2_com::BrowserExtensionRemoveCompletedHandler::create(Box::new(|_| Ok(()))));
                }
            }
        }
        Ok(())
    }));
    unsafe {
        let _ = p.GetBrowserExtensions(&h);
    }
}

fn copier_dossier(de: &std::path::Path, vers: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(vers)?;
    for e in std::fs::read_dir(de)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() {
            copier_dossier(&p, &vers.join(e.file_name()))?;
        } else {
            std::fs::copy(&p, vers.join(e.file_name()))?;
        }
    }
    Ok(())
}

impl Navigateur {
    pub fn new() -> Self {
        Navigateur { r: charger("navigateur"), onglets: vec![Onglet::nouveau(false)], actif: 0, contexte: None, passe_vue: 0, bloques: Arc::default(), erreur: String::new(), derniere_url: Instant::now(), panneau_ext: false, ext_profil: Rc::default(), ext_saisie: String::new(), ext_msg: Arc::default(), ext_a_ajouter: Arc::default(), releve: None }
    }

    /// Une étape du relevé automatique, à chaque image.
    fn avancer_releve(&mut self, ctx: &egui::Context) {
        use crate::catalogue_prix::{lire_releve, rayon_de_titre, recevoir_releve, JS_DEFILER, JS_LIRE, JS_SUIVANT};
        let Some(r) = &mut self.releve else { return };
        ctx.request_repaint_after(Duration::from_millis(250));
        if r.pause || Instant::now() < r.attendre {
            return;
        }
        let Some(v) = self.onglets[self.actif].vue.as_ref() else { return };
        let mut fin: Option<String> = None;
        if let Some(depuis) = r.attente {
            let rep = r.reponse.lock().unwrap().take();
            let Some(j) = rep else {
                if depuis.elapsed() > Duration::from_secs(20) {
                    r.attente = None; // la page a changé pendant le script : on recommence l'étape
                }
                return;
            };
            r.attente = None;
            match r.etape {
                EtapeReleve::Defiler => {
                    r.defilements += 1;
                    if j.trim() == "true" || r.defilements >= 40 {
                        r.etape = EtapeReleve::Lire;
                    }
                    r.attendre = Instant::now() + Duration::from_millis(900);
                }
                EtapeReleve::Lire => {
                    let (t, verif) = lire_releve(&j);
                    if t.is_empty() && verif {
                        r.pause = true;
                        return;
                    }
                    let titre = serde_json::from_str::<serde_json::Value>(&j).ok().and_then(|x| x["titre"].as_str().map(String::from)).unwrap_or_default();
                    let nouveaux: Vec<_> = t.into_iter().filter(|x| r.urls.insert(x.url.clone())).collect();
                    if nouveaux.is_empty() {
                        r.vides += 1;
                    } else {
                        r.vides = 0;
                        recevoir_releve(&r.magasin, &rayon_de_titre(&titre), nouveaux);
                    }
                    if r.pages + 1 >= r.max_pages || r.vides >= 2 {
                        fin = Some(format!("✔ Relevé fini : {} produits envoyés au « Catalogue de prix »", r.urls.len()));
                    } else {
                        r.etape = EtapeReleve::Suivant;
                        r.attendre = Instant::now() + Duration::from_millis(800);
                    }
                }
                EtapeReleve::Suivant => {
                    if j.trim() == "true" {
                        r.pages += 1;
                        r.etape = EtapeReleve::Defiler;
                        r.defilements = 0;
                        r.attendre = Instant::now() + Duration::from_millis(3500);
                    } else {
                        fin = Some(format!("✔ Relevé fini ({} page(s)) : {} produits envoyés au « Catalogue de prix »", r.pages + 1, r.urls.len()));
                    }
                }
            }
        } else {
            let js = match r.etape {
                EtapeReleve::Defiler => JS_DEFILER,
                EtapeReleve::Lire => JS_LIRE,
                EtapeReleve::Suivant => JS_SUIVANT,
            };
            let rep = r.reponse.clone();
            r.attente = Some(Instant::now());
            let _ = v.evaluate_script_with_callback(js, move |s| *rep.lock().unwrap() = Some(s));
        }
        if let Some(m) = fin {
            *self.ext_msg.lock().unwrap() = m;
            self.releve = None;
        }
    }

    fn creer_vue(&mut self, k: usize, url: &str, zone: wry::Rect) {
        let Some(h) = fenetre_outils() else {
            self.erreur = "fenêtre d'Outils introuvable".into();
            return;
        };
        let ctx = self.contexte.get_or_insert_with(|| wry::WebContext::new(Some(crate::outil::base().join("navigateur"))));
        let infos = self.onglets[k].infos.clone();
        let (i1, i2, i3) = (infos.clone(), infos.clone(), infos.clone());
        let b = WebViewBuilder::new_with_web_context(ctx)
            .with_url(url)
            .with_bounds(zone)
            .with_incognito(self.onglets[k].prive)
            .with_theme(if self.r.sombre { wry::Theme::Dark } else { wry::Theme::Light })
            // moins de services d'arrière-plan d'Edge : plus léger
            .with_browser_extensions_enabled(true)
            .with_extensions_path(dossier_ext())
            .with_additional_browser_args("--disable-background-networking --disable-component-update --disable-features=msSmartScreenProtection,msEdgeShoppingAssistant,msWebOOUI")
            .with_document_title_changed_handler(move |t| i1.borrow_mut().titre = t)
            .with_on_page_load_handler(move |e, u| {
                let mut i = i2.borrow_mut();
                i.url = u.clone();
                i.charge = matches!(e, wry::PageLoadEvent::Started);
                if matches!(e, wry::PageLoadEvent::Finished) {
                    let t = i.titre.clone();
                    i.fini.push((t, u));
                }
            })
            .with_new_window_req_handler(move |u, _| {
                i3.borrow_mut().nouveaux.push(u);
                wry::NewWindowResponse::Deny
            })
            .with_download_completed_handler(|_, chemin, ok| {
                if ok {
                    let n = chemin.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    crate::systeme::notifier("Téléchargement terminé", &n);
                }
            });
        match b.build_as_child(&Parent(h)) {
            Ok(v) => {
                if self.r.bloqueur {
                    installer_bloqueur(&v, self.bloques.clone());
                }
                infos.borrow_mut().url = url.to_string();
                self.onglets[k].vue = Some(v);
            }
            Err(e) => self.erreur = format!("Le moteur web de Windows (WebView2) ne démarre pas : {e}"),
        }
    }

    fn aller(&mut self, url: String, zone: wry::Rect) {
        let k = self.actif;
        self.onglets[k].adresse = url.clone();
        match &self.onglets[k].vue {
            Some(v) => {
                let _ = v.load_url(&url);
            }
            None => self.creer_vue(k, &url, zone),
        }
    }

    fn cacher_tout(&mut self) {
        for o in &mut self.onglets {
            if let Some(v) = &o.vue {
                let _ = v.set_visible(false);
            }
        }
    }

    fn fermer(&mut self, k: usize) {
        self.onglets.remove(k);
        if self.onglets.is_empty() {
            self.onglets.push(Onglet::nouveau(false));
        }
        self.actif = self.actif.min(self.onglets.len() - 1);
    }

    fn une_vue(&self) -> Option<&WebView> {
        self.onglets.iter().find_map(|o| o.vue.as_ref())
    }

    /// Panneau des extensions (dessiné par Outils, la page est cachée le temps de l'afficher).
    fn panneau_extensions(&mut self, ui: &mut egui::Ui) -> Option<String> {
        use crate::nav_extensions::*;
        let mut ouvrir = None;
        let _ = std::fs::create_dir_all(dossier_ext());
        ui.heading("🧩 Extensions");
        ui.weak("Les extensions du Chrome Web Store s'installent ici. Celles qui changent les pages (bloqueurs, mode sombre, YouTube…) marchent en général ; celles qui ont besoin de la barre d'outils de Chrome s'ouvrent dans un onglet (bouton 🪟).");
        // extensions d'Outils
        ui.add_space(6.0);
        ui.strong("Extensions d'Outils");
        for (dos, nom, desc, _) in MAISON {
            let actif = dossier_ext().join(dos).is_dir();
            let mut on = actif;
            ui.horizontal_wrapped(|ui| {
                if ui.checkbox(&mut on, RichText::new(*nom).strong()).changed() {
                    let d = dossier_ext().join(dos);
                    if on {
                        match ecrire_maison(dos, &d) {
                            Ok(()) => self.ext_a_ajouter.lock().unwrap().push(d),
                            Err(e) => *self.ext_msg.lock().unwrap() = e,
                        }
                    } else {
                        let n = Manifeste::lire(&d).map(|m| m.nom).unwrap_or_default();
                        let _ = std::fs::remove_dir_all(&d);
                        if let Some(v) = self.une_vue() {
                            retirer_ext(v, dos.to_string(), n);
                        }
                    }
                }
                ui.weak(*desc);
            });
        }
        // installer
        ui.add_space(10.0);
        ui.strong("Ajouter une extension du Chrome Web Store");
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.ext_saisie).hint_text("colle le lien de la page de l'extension").desired_width(380.0));
            let id = id_extension(&self.ext_saisie);
            if ui.add_enabled(id.is_some(), egui::Button::new("📥 Installer")).clicked() {
                let (id, m, q) = (id.unwrap(), self.ext_msg.clone(), self.ext_a_ajouter.clone());
                *m.lock().unwrap() = "Téléchargement de l'extension…".into();
                self.ext_saisie.clear();
                std::thread::spawn(move || {
                    *m.lock().unwrap() = match telecharger(&id, &dossier_ext()) {
                        Ok(n) => {
                            q.lock().unwrap().push(dossier_ext().join(&id));
                            format!("✔ « {n} » installée")
                        }
                        Err(e) => format!("✖ {e}"),
                    };
                });
            }
            if ui.button("🛒 Ouvrir le Chrome Web Store").clicked() {
                ouvrir = Some("https://chromewebstore.google.com/".to_string());
                self.panneau_ext = false;
            }
            if ui.button("📂 Extension décompressée…").on_hover_text("Un dossier contenant manifest.json").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    if d.join("manifest.json").is_file() {
                        let dest = dossier_ext().join(d.file_name().unwrap_or_default());
                        match copier_dossier(&d, &dest) {
                            Ok(()) => self.ext_a_ajouter.lock().unwrap().push(dest),
                            Err(e) => *self.ext_msg.lock().unwrap() = e.to_string(),
                        }
                    } else {
                        *self.ext_msg.lock().unwrap() = "Ce dossier ne contient pas de manifest.json".into();
                    }
                }
            }
        });
        ui.weak("Astuce : sur une page du Chrome Web Store ouverte dans le navigateur, le bouton « 🧩 + » de la barre d'adresse l'installe directement.");
        let m = self.ext_msg.lock().unwrap().clone();
        if !m.is_empty() {
            ui.label(m);
        }
        // installées
        let profil_liste = self.ext_profil.borrow().clone();
        let maison: Vec<&str> = MAISON.iter().map(|m| m.0).collect();
        let actives: Vec<(std::path::PathBuf, Manifeste)> = extensions_dans(&dossier_ext()).into_iter().filter(|(p, _)| !maison.iter().any(|n| p.ends_with(n))).collect();
        let off = extensions_dans(&dossier_off());
        if !actives.is_empty() || !off.is_empty() {
            ui.add_space(10.0);
            ui.strong("Mes extensions");
        }
        for (p, m) in actives {
            let dos = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            let id_moteur = profil_liste.iter().find(|x| x.0 == dos || x.1 == m.nom).map(|x| x.0.clone());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("🧩 {}", m.nom)).strong());
                ui.weak(&m.version);
                if let (Some(id), Some(f)) = (&id_moteur, &m.fenetre) {
                    if ui.small_button("🪟 Ouvrir").on_hover_text("La fenêtre de l'extension, dans un onglet").clicked() {
                        ouvrir = Some(format!("chrome-extension://{id}/{}", f.trim_start_matches('/')));
                    }
                }
                if let (Some(id), Some(f)) = (&id_moteur, &m.reglages) {
                    if ui.small_button("⚙ Réglages").clicked() {
                        ouvrir = Some(format!("chrome-extension://{id}/{}", f.trim_start_matches('/')));
                    }
                }
                if ui.small_button("⏸ Désactiver").clicked() {
                    let _ = std::fs::create_dir_all(dossier_off());
                    let _ = std::fs::rename(&p, dossier_off().join(&dos));
                    if let Some(v) = self.une_vue() {
                        retirer_ext(v, id_moteur.clone().unwrap_or(dos.clone()), m.nom.clone());
                    }
                }
                if ui.small_button("🗑").on_hover_text("Désinstaller").clicked() {
                    let _ = std::fs::remove_dir_all(&p);
                    if let Some(v) = self.une_vue() {
                        retirer_ext(v, id_moteur.clone().unwrap_or(dos.clone()), m.nom.clone());
                    }
                }
            });
        }
        for (p, m) in off {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("⏸ {}", m.nom)).weak());
                if ui.small_button("▶ Activer").clicked() {
                    let dest = dossier_ext().join(p.file_name().unwrap_or_default());
                    if std::fs::rename(&p, &dest).is_ok() {
                        self.ext_a_ajouter.lock().unwrap().push(dest);
                    }
                }
                if ui.small_button("🗑").clicked() {
                    let _ = std::fs::remove_dir_all(&p);
                }
            });
        }
        if ui.button("✔ Fermer").clicked() {
            self.panneau_ext = false;
        }
        ouvrir
    }

    /// Page « nouvel onglet », dessinée par Outils (aucun moteur web).
    fn page_accueil(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut aller = None;
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("🌐").size(48.0));
            ui.add_space(10.0);
            let mut recherche = std::mem::take(&mut self.onglets[self.actif].adresse);
            let r = ui.add(egui::TextEdit::singleline(&mut recherche).hint_text(format!("Rechercher avec {} ou taper une adresse", MOTEURS[self.r.moteur].0)).desired_width(520.0).font(egui::TextStyle::Heading));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !recherche.trim().is_empty() {
                aller = Some(vers_url(&recherche, self.r.moteur));
            }
            self.onglets[self.actif].adresse = recherche;
            ui.add_space(20.0);
        });
        ui.horizontal_wrapped(|ui| {
            for (t, u) in self.r.favoris.clone() {
                let b = egui::Button::new(RichText::new(format!("⭐\n{t}")).size(16.0)).min_size(egui::vec2(130.0, 70.0)).wrap();
                let rep = ui.add(b).on_hover_text(&u);
                if rep.clicked() {
                    aller = Some(u.clone());
                }
                rep.context_menu(|ui| {
                    if ui.button("🗑 Retirer des favoris").clicked() {
                        self.r.favoris.retain(|f| f.1 != u);
                        ui.close_menu();
                    }
                });
            }
        });
        if !self.r.historique.is_empty() {
            ui.add_space(16.0);
            ui.strong("🕘 Récemment");
            for (t, u) in self.r.historique.iter().rev().take(10) {
                if ui.link(if t.is_empty() { u.as_str() } else { t.as_str() }).on_hover_text(u).clicked() {
                    aller = Some(u.clone());
                }
            }
            if ui.small_button("Effacer l'historique").clicked() {
                self.r.historique.clear();
            }
        }
        ui.add_space(16.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Moteur de recherche :");
            for (k, (n, _)) in MOTEURS.iter().enumerate() {
                ui.selectable_value(&mut self.r.moteur, k, *n);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.r.bloqueur, "🛡 Bloquer les pubs et les pisteurs (onglets suivants)");
            ui.checkbox(&mut self.r.sombre, "🌙 Sites en mode sombre quand ils le proposent");
            ui.label("Endormir les onglets après");
            ui.add(egui::DragValue::new(&mut self.r.endormir_min).range(1..=120).suffix(" min"));
        });
        aller
    }
}

impl Outil for Navigateur {
    fn nom(&self) -> &'static str {
        "🌐 Navigateur"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn tic(&mut self, ctx: &egui::Context) {
        // l'outil n'est plus affiché : les pages passent derrière, en mémoire réduite
        let affiche = ctx.cumulative_pass_nr() <= self.passe_vue + 1;
        if !affiche {
            for o in &mut self.onglets {
                if let Some(v) = &o.vue {
                    let _ = v.set_visible(false);
                    if !o.economie {
                        let _ = v.set_memory_usage_level(wry::MemoryUsageLevel::Low);
                        o.economie = true;
                    }
                }
            }
        }
        // onglets endormis : le moteur est libéré, l'adresse est gardée
        let limite = Duration::from_secs(self.r.endormir_min.max(1) * 60);
        for (k, o) in self.onglets.iter_mut().enumerate() {
            if o.vue.is_some() && (k != self.actif || !affiche) && o.vu.elapsed() > limite {
                o.adresse = o.infos.borrow().url.clone();
                o.vue = None;
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.passe_vue = ui.ctx().cumulative_pass_nr();
        let avant = self.r.clone();
        // nouvelles fenêtres demandées par les pages, pages chargées (historique)
        let mut nouveaux = Vec::new();
        for o in &self.onglets {
            let mut i = o.infos.borrow_mut();
            nouveaux.append(&mut i.nouveaux);
            if !o.prive {
                for (t, u) in i.fini.drain(..) {
                    self.r.historique.retain(|h| h.1 != u);
                    self.r.historique.push((t, u));
                }
            } else {
                i.fini.clear();
            }
        }
        if let Some(u) = crate::catalogue_prix::OUVRIR_DANS_NAVIGATEUR.lock().unwrap().take() {
            nouveaux.push(u);
        }
        if self.r.historique.len() > 300 {
            let n = self.r.historique.len() - 300;
            self.r.historique.drain(..n);
        }
        // barre d'onglets
        let mut fermer = None;
        let mut changer = None;
        ui.horizontal_wrapped(|ui| {
            for k in 0..self.onglets.len() {
                let t = self.onglets[k].titre();
                let endormi = self.onglets[k].vue.is_none() && !self.onglets[k].adresse.is_empty() && self.onglets[k].infos.borrow().url.len() > 0;
                let r = ui.selectable_label(k == self.actif, if endormi { format!("💤 {t}") } else { t });
                if r.clicked() {
                    changer = Some(k);
                }
                if r.middle_clicked() {
                    fermer = Some(k);
                }
                if k == self.actif && ui.small_button("✖").clicked() {
                    fermer = Some(k);
                }
            }
            if ui.small_button("➕").on_hover_text("Nouvel onglet (Ctrl+T)").clicked() || ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::T)) {
                self.onglets.push(Onglet::nouveau(false));
                changer = Some(self.onglets.len() - 1);
            }
            if ui.small_button("🕶").on_hover_text("Onglet privé : rien n'est gardé").clicked() {
                self.onglets.push(Onglet::nouveau(true));
                changer = Some(self.onglets.len() - 1);
            }
        });
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::W)) {
            fermer = Some(self.actif);
        }
        if let Some(k) = fermer {
            self.fermer(k);
            self.cacher_tout();
        }
        if let Some(k) = changer {
            self.cacher_tout();
            self.actif = k.min(self.onglets.len() - 1);
        }
        let k = self.actif;
        let a_vue = self.onglets[k].vue.is_some();
        let endormi = !a_vue && !self.onglets[k].infos.borrow().url.is_empty() && self.erreur.is_empty();
        // barre d'adresse
        let mut aller: Option<String> = None;
        if a_vue || endormi {
            // la vue est sortie de l'onglet le temps de la barre (on modifie l'onglet en même temps)
            let vue_sortie = self.onglets[k].vue.take();
            let mut accueil = false;
            ui.horizontal(|ui| {
                let vue = vue_sortie.as_ref();
                if ui.button("◀").on_hover_text("Page précédente").clicked() {
                    if let Some(v) = vue {
                        let _ = v.evaluate_script("history.back()");
                    }
                }
                if ui.button("▶").on_hover_text("Page suivante").clicked() {
                    if let Some(v) = vue {
                        let _ = v.evaluate_script("history.forward()");
                    }
                }
                let charge = self.onglets[k].infos.borrow().charge;
                if ui.button(if charge { "✖" } else { "⟳" }).on_hover_text(if charge { "Arrêter" } else { "Recharger" }).clicked() {
                    if let Some(v) = vue {
                        let _ = if charge { v.evaluate_script("window.stop()") } else { v.reload() };
                    }
                }
                if ui.button("🏠").on_hover_text("Nouvel onglet").clicked() {
                    accueil = true;
                    *self.onglets[k].infos.borrow_mut() = Infos::default();
                    self.onglets[k].adresse.clear();
                }
                let url_actuelle = self.onglets[k].infos.borrow().url.clone();
                let favori = self.r.favoris.iter().any(|f| f.1 == url_actuelle);
                let largeur = ui.available_width() - 170.0;
                let r = ui.add(egui::TextEdit::singleline(&mut self.onglets[k].adresse).desired_width(largeur.max(120.0)));
                if r.gained_focus() || r.clicked() {
                    // le clavier revient à Outils (sinon la page garde les touches)
                    if let Some(v) = vue {
                        let _ = v.focus_parent();
                    }
                }
                if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::L)) {
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    aller = Some(vers_url(&self.onglets[k].adresse, self.r.moteur));
                } else if !r.has_focus() && self.derniere_url.elapsed() > Duration::from_millis(500) {
                    // adresse à jour (les sites qui changent d'adresse sans recharger)
                    self.derniere_url = Instant::now();
                    if let Some(u) = vue.and_then(|v| v.url().ok()) {
                        if !u.is_empty() && u != "about:blank" {
                            self.onglets[k].infos.borrow_mut().url.clone_from(&u);
                            self.onglets[k].adresse = u;
                        }
                    }
                }
                if ui.button(if favori { "⭐" } else { "☆" }).on_hover_text("Favori").clicked() {
                    if favori {
                        self.r.favoris.retain(|f| f.1 != url_actuelle);
                    } else if !url_actuelle.is_empty() {
                        let t = self.onglets[k].infos.borrow().titre.clone();
                        self.r.favoris.push((if t.is_empty() { url_actuelle.clone() } else { t.chars().take(30).collect() }, url_actuelle.clone()));
                    }
                }
                // page d'un produit : l'envoyer au catalogue de prix (marche même sur les sites qui bloquent les robots)
                if url_actuelle.starts_with("http") && self.releve.is_none() && ui.button("🛒 Tout le rayon").on_hover_text("Relève tous les produits de ce rayon (page après page) pour le catalogue de prix").clicked() {
                    self.releve = Some(Releve {
                        etape: EtapeReleve::Defiler,
                        pages: 0,
                        max_pages: 40,
                        defilements: 0,
                        attendre: Instant::now(),
                        reponse: Arc::default(),
                        attente: None,
                        urls: Default::default(),
                        vides: 0,
                        pause: false,
                        magasin: crate::catalogue_prix::magasin_de(&url_actuelle),
                    });
                }
                if url_actuelle.starts_with("http") && ui.button("🛒").on_hover_text("Ajouter ce produit au catalogue de prix").clicked() {
                    if let Some(v) = vue {
                        let u = url_actuelle.clone();
                        let _ = v.evaluate_script_with_callback("document.documentElement.outerHTML", move |json| {
                            let html: String = serde_json::from_str(&json).unwrap_or_default();
                            if !html.is_empty() {
                                crate::catalogue_prix::recevoir(&u, html);
                            }
                        });
                        *self.ext_msg.lock().unwrap() = "🛒 Produit envoyé au « Catalogue de prix »".into();
                    }
                }
                // page du Chrome Web Store : installation directe
                if let Some(id) = url_actuelle.contains("chromewebstore.google.com").then(|| crate::nav_extensions::id_extension(&url_actuelle)).flatten() {
                    if ui.button("🧩 +").on_hover_text("Ajouter cette extension au navigateur d'Outils").clicked() {
                        let (m, q) = (self.ext_msg.clone(), self.ext_a_ajouter.clone());
                        *m.lock().unwrap() = "Téléchargement de l'extension…".into();
                        std::thread::spawn(move || {
                            *m.lock().unwrap() = match crate::nav_extensions::telecharger(&id, &dossier_ext()) {
                                Ok(n) => {
                                    q.lock().unwrap().push(dossier_ext().join(&id));
                                    format!("✔ « {n} » installée")
                                }
                                Err(e) => format!("✖ {e}"),
                            };
                        });
                    }
                }
                if ui.button("🧩").on_hover_text("Extensions").clicked() {
                    self.panneau_ext = !self.panneau_ext;
                }
                let n = self.bloques.load(Relaxed);
                if self.r.bloqueur {
                    ui.label(RichText::new(format!("🛡 {n}")).small()).on_hover_text("Pubs et pisteurs bloqués depuis le lancement");
                }
            });
            if !accueil {
                self.onglets[k].vue = vue_sortie;
            }
        }
        if !self.erreur.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, &self.erreur);
            if ui.button("📥 Installer le moteur WebView2").clicked() {
                let _ = std::process::Command::new("explorer").arg("https://go.microsoft.com/fwlink/p/?LinkId=2124703").spawn();
            }
        }
        // relevé automatique en cours
        let mut arreter = false;
        if let Some(r) = &mut self.releve {
            ui.horizontal_wrapped(|ui| {
                if r.pause {
                    ui.colored_label(Color32::from_rgb(250, 190, 80), "🧩 Vérification « je ne suis pas un robot » : fais-la dans la page, puis");
                    if ui.button(RichText::new("▶ Continuer").strong()).clicked() {
                        r.pause = false;
                        r.vides = 0;
                    }
                } else {
                    ui.spinner();
                    let quoi = match r.etape {
                        EtapeReleve::Defiler => "je descends la page",
                        EtapeReleve::Lire => "je relève les produits",
                        EtapeReleve::Suivant => "page suivante",
                    };
                    ui.label(format!("🛒 Relevé : page {} · {} produits · {quoi}…", r.pages + 1, r.urls.len()));
                }
                if ui.button("⏹ Arrêter").clicked() {
                    arreter = true;
                }
            });
        }
        if arreter {
            let n = self.releve.as_ref().map_or(0, |r| r.urls.len());
            *self.ext_msg.lock().unwrap() = format!("⏹ Relevé arrêté : {n} produits envoyés au « Catalogue de prix »");
            self.releve = None;
        }
        self.avancer_releve(&ui.ctx().clone());
        // zone de la page, en pixels de la fenêtre
        let zone = ui.available_rect_before_wrap();
        let ppp = ui.ctx().pixels_per_point();
        let rect = wry::Rect {
            position: wry::dpi::PhysicalPosition::new((zone.min.x * ppp) as i32, (zone.min.y * ppp) as i32).into(),
            size: wry::dpi::PhysicalSize::new((zone.width() * ppp).max(1.0) as u32, (zone.height() * ppp).max(1.0) as u32).into(),
        };
        if endormi && aller.is_none() {
            // l'onglet se réveille : la page revient
            let u = self.onglets[k].infos.borrow().url.clone();
            self.creer_vue(k, &u, rect);
        }
        // extensions installées pendant que des pages sont ouvertes : ajoutées au moteur tout de suite
        let a_ajouter: Vec<std::path::PathBuf> = std::mem::take(&mut *self.ext_a_ajouter.lock().unwrap());
        if let Some(v) = self.une_vue() {
            for d in &a_ajouter {
                ajouter_ext(v, d);
            }
        }
        let msg_ext = self.ext_msg.lock().unwrap().clone();
        if !self.panneau_ext && !msg_ext.is_empty() {
            ui.label(&msg_ext);
        }
        if self.panneau_ext {
            if self.derniere_url.elapsed() > Duration::from_secs(1) {
                self.derniere_url = Instant::now();
                if let Some(v) = self.une_vue() {
                    lister_ext(v, self.ext_profil.clone());
                }
            }
            self.cacher_tout();
            let ouvrir = egui::ScrollArea::vertical().show(ui, |ui| self.panneau_extensions(ui)).inner;
            if let Some(u) = ouvrir {
                self.panneau_ext = false;
                self.onglets.push(Onglet::nouveau(false));
                self.actif = self.onglets.len() - 1;
                let k = self.actif;
                self.creer_vue(k, &u, rect);
            }
            if self.r != avant {
                enregistrer("navigateur", &self.r);
            }
            return;
        }
        if self.onglets[k].vue.is_none() && aller.is_none() {
            aller = egui::ScrollArea::vertical().show(ui, |ui| {
                if ui.button("🧩 Extensions").clicked() {
                    self.panneau_ext = true;
                }
                self.page_accueil(ui)
            }).inner;
        }
        if let Some(u) = aller {
            self.aller(u, rect);
        }
        for u in nouveaux {
            let mut o = Onglet::nouveau(self.onglets[self.actif].prive);
            o.adresse = u.clone();
            self.onglets.push(o);
            let n = self.onglets.len() - 1;
            self.cacher_tout();
            self.actif = n;
            self.creer_vue(n, &u, rect);
        }
        let k = self.actif;
        let o = &mut self.onglets[k];
        o.vu = Instant::now();
        if let Some(v) = &o.vue {
            let _ = v.set_bounds(rect);
            let _ = v.set_visible(true);
            if o.economie {
                let _ = v.set_memory_usage_level(wry::MemoryUsageLevel::Normal);
                o.economie = false;
            }
            ui.allocate_rect(zone, egui::Sense::hover());
        }
        // les autres onglets : cachés et en mémoire réduite
        for (j, o) in self.onglets.iter_mut().enumerate() {
            if j != k && !o.economie {
                if let Some(v) = &o.vue {
                    let _ = v.set_visible(false);
                    let _ = v.set_memory_usage_level(wry::MemoryUsageLevel::Low);
                }
                o.economie = true;
            }
        }
        if self.onglets[k].infos.borrow().charge {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        if self.r != avant {
            enregistrer("navigateur", &self.r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adresses_et_bloqueur() {
        assert_eq!(vers_url("wikipedia.org", 0), "https://wikipedia.org");
        assert_eq!(vers_url("https://x.fr/a b", 0), "https://x.fr/a b");
        assert_eq!(vers_url("chat mignon", 1), "https://www.google.com/search?q=chat+mignon");
        assert_eq!(vers_url("été", 0), "https://duckduckgo.com/?q=%C3%A9t%C3%A9");
        assert!(vers_url("localhost:8080", 0).starts_with("https://localhost"));
        assert!(bloque("https://securepubads.g.doubleclick.net/tag/js/gpt.js"));
        assert!(bloque("https://www.google-analytics.com/analytics.js"));
        assert!(!bloque("https://www.youtube.com/watch?v=1"));
        assert!(!bloque("https://notdoubleclick.net/"));
    }
}
