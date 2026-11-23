//! Mixeur audio : volume général et volume par application, sortie par défaut (casque / enceintes),
//! raccourci Ctrl+Alt+A pour passer à la sortie suivante, profils appliqués quand un jeu démarre.
//! Tout l'audio passe par un fil dédié (les objets COM ne quittent pas leur fil).
//! Réglages dans %APPDATA%\Outils\mixeur.json.
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Profil {
    pub sortie: Option<String>,             // id du périphérique
    pub volumes: BTreeMap<String, f32>,     // exe -> volume 0..1
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub favoris: Vec<String>, // sorties parcourues par Ctrl+Alt+A
    pub raccourci: bool,
    pub profils: BTreeMap<String, Profil>, // nom du jeu -> profil
    pub profils_actifs: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { favoris: vec![], raccourci: true, profils: BTreeMap::new(), profils_actifs: true }
    }
}

fn fichier() -> PathBuf {
    crate::outil::base().join("mixeur.json")
}

#[derive(Clone, Default, Debug)]
pub struct Session {
    pub nom: String, // exe
    pub volume: f32,
    pub muet: bool,
    pub crete: f32,
}

#[derive(Clone, Default)]
pub struct Etat {
    pub sorties: Vec<(String, String)>, // (id, nom)
    pub defaut: Option<String>,
    pub maitre: f32,
    pub maitre_muet: bool,
    pub crete: f32,
    pub sessions: Vec<Session>,
    pub erreur: Option<String>,
}

pub enum Ordre {
    VolumeApp(String, f32), // par nom d'exe : toutes ses sessions
    MuetApp(String, bool),
    Maitre(f32),
    MaitreMuet(bool),
    Sortie(String),
}

/// Sortie suivante parmi les favoris (ou toutes les sorties s'il n'y a pas de favoris).
pub fn suivante(sorties: &[(String, String)], favoris: &[String], actuelle: Option<&str>) -> Option<String> {
    let liste: Vec<&String> = if favoris.len() >= 2 {
        favoris.iter().filter(|f| sorties.iter().any(|s| &s.0 == *f)).collect()
    } else {
        sorties.iter().map(|s| &s.0).collect()
    };
    if liste.is_empty() {
        return None;
    }
    let i = actuelle.and_then(|a| liste.iter().position(|x| x.as_str() == a)).map_or(0, |i| i + 1);
    Some(liste[i % liste.len()].clone())
}

/// Regroupe les sessions par exe (une appli peut en ouvrir plusieurs).
pub fn par_appli(s: &[Session]) -> Vec<Session> {
    let mut v: Vec<Session> = vec![];
    for x in s {
        match v.iter_mut().find(|y| y.nom.eq_ignore_ascii_case(&x.nom)) {
            Some(y) => {
                y.crete = y.crete.max(x.crete);
                y.muet &= x.muet;
            }
            None => v.push(x.clone()),
        }
    }
    v.sort_by_key(|x| x.nom.to_lowercase());
    v
}

mod audio {
    use super::*;
    use windows::core::{Interface, GUID, PCWSTR};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;

    pub struct Audio {
        enumerateur: IMMDeviceEnumerator,
    }

    fn nom_peripherique(d: &IMMDevice) -> String {
        unsafe { d.OpenPropertyStore(STGM_READ).and_then(|p| p.GetValue(&PKEY_Device_FriendlyName)).map(|v| v.to_string()).unwrap_or_else(|_| "Sortie".into()) }
    }

    fn id(d: &IMMDevice) -> Option<String> {
        unsafe {
            let p = d.GetId().ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        }
    }

    /// Interface non documentée de Windows pour changer la sortie par défaut (utilisée par tous les outils du genre).
    #[repr(C)]
    struct VtPolitique {
        _iunknown: [usize; 3],
        _autres: [usize; 10],
        set_default_endpoint: unsafe extern "system" fn(*mut core::ffi::c_void, PCWSTR, ERole) -> windows::core::HRESULT,
    }
    const CLSID_POLITIQUE: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const IID_POLITIQUE: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);

    impl Audio {
        pub fn new() -> windows::core::Result<Audio> {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                Ok(Audio { enumerateur: CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? })
            }
        }

        fn defaut(&self) -> windows::core::Result<IMMDevice> {
            unsafe { self.enumerateur.GetDefaultAudioEndpoint(eRender, eMultimedia) }
        }

        pub fn etat(&self) -> windows::core::Result<Etat> {
            unsafe {
                let mut e = Etat::default();
                let col = self.enumerateur.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
                for i in 0..col.GetCount()? {
                    let d = col.Item(i)?;
                    if let Some(id) = id(&d) {
                        e.sorties.push((id, nom_peripherique(&d)));
                    }
                }
                let d = self.defaut()?;
                e.defaut = id(&d);
                let vol: IAudioEndpointVolume = d.Activate(CLSCTX_ALL, None)?;
                e.maitre = vol.GetMasterVolumeLevelScalar()?;
                e.maitre_muet = vol.GetMute()?.as_bool();
                if let Ok(m) = d.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) {
                    e.crete = m.GetPeakValue().unwrap_or(0.0);
                }
                let gest: IAudioSessionManager2 = d.Activate(CLSCTX_ALL, None)?;
                let sessions = gest.GetSessionEnumerator()?;
                for i in 0..sessions.GetCount()? {
                    let Ok(s) = sessions.GetSession(i) else { continue };
                    let Ok(s2) = s.cast::<IAudioSessionControl2>() else { continue };
                    if s.GetState().is_ok_and(|x| x == AudioSessionStateExpired) {
                        continue;
                    }
                    let pid = s2.GetProcessId().unwrap_or(0);
                    let systeme = s2.IsSystemSoundsSession() == windows::core::HRESULT(0);
                    let Ok(v) = s.cast::<ISimpleAudioVolume>() else { continue };
                    let crete = s.cast::<IAudioMeterInformation>().and_then(|m| m.GetPeakValue()).unwrap_or(0.0);
                    let nom = if systeme || pid == 0 { "Sons système".to_string() } else { crate::reseau::nom_processus(pid) };
                    e.sessions.push(Session { nom, volume: v.GetMasterVolume().unwrap_or(1.0), muet: v.GetMute().map(|b| b.as_bool()).unwrap_or(false), crete });
                }
                Ok(e)
            }
        }

        /// Applique `f` au volume de toutes les sessions de cette appli.
        fn pour_appli(&self, exe: &str, f: impl Fn(&ISimpleAudioVolume)) -> windows::core::Result<()> {
            unsafe {
                let gest: IAudioSessionManager2 = self.defaut()?.Activate(CLSCTX_ALL, None)?;
                let sessions = gest.GetSessionEnumerator()?;
                for i in 0..sessions.GetCount()? {
                    let Ok(s) = sessions.GetSession(i) else { continue };
                    let Ok(s2) = s.cast::<IAudioSessionControl2>() else { continue };
                    let pid = s2.GetProcessId().unwrap_or(0);
                    let systeme = s2.IsSystemSoundsSession() == windows::core::HRESULT(0);
                    let nom = if systeme || pid == 0 { "Sons système".to_string() } else { crate::reseau::nom_processus(pid) };
                    if nom.eq_ignore_ascii_case(exe) {
                        if let Ok(v) = s.cast::<ISimpleAudioVolume>() {
                            f(&v);
                        }
                    }
                }
                Ok(())
            }
        }

        pub fn executer(&self, o: Ordre) -> windows::core::Result<()> {
            unsafe {
                match o {
                    Ordre::VolumeApp(exe, v) => self.pour_appli(&exe, |s| {
                        let _ = s.SetMasterVolume(v.clamp(0.0, 1.0), std::ptr::null());
                    }),
                    Ordre::MuetApp(exe, m) => self.pour_appli(&exe, |s| {
                        let _ = s.SetMute(m, std::ptr::null());
                    }),
                    Ordre::Maitre(v) => {
                        let vol: IAudioEndpointVolume = self.defaut()?.Activate(CLSCTX_ALL, None)?;
                        vol.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null())
                    }
                    Ordre::MaitreMuet(m) => {
                        let vol: IAudioEndpointVolume = self.defaut()?.Activate(CLSCTX_ALL, None)?;
                        vol.SetMute(m, std::ptr::null())
                    }
                    Ordre::Sortie(id) => {
                        let unk: windows::core::IUnknown = CoCreateInstance(&CLSID_POLITIQUE, None, CLSCTX_ALL)?;
                        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
                        unk.query(&IID_POLITIQUE, &mut p).ok()?;
                        let vt = &**(p as *mut *const VtPolitique);
                        let large: Vec<u16> = id.encode_utf16().chain([0]).collect();
                        let mut r = Ok(());
                        for role in [eConsole, eMultimedia, eCommunications] {
                            r = (vt.set_default_endpoint)(p, PCWSTR(large.as_ptr()), role).ok();
                        }
                        // libère la référence obtenue par QueryInterface
                        let release: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 = std::mem::transmute(vt._iunknown[2]);
                        release(p);
                        r
                    }
                }
            }
        }
    }
}

/// Fil audio : exécute les ordres et publie l'état (souvent si l'onglet est visible, rarement sinon).
fn fil(rx: Receiver<Ordre>, etat: Arc<Mutex<Etat>>, rapide: Arc<Mutex<Instant>>) {
    std::thread::spawn(move || {
        let audio = match audio::Audio::new() {
            Ok(a) => a,
            Err(e) => {
                etat.lock().unwrap().erreur = Some(format!("Audio indisponible : {e}"));
                return;
            }
        };
        loop {
            let delai = if rapide.lock().unwrap().elapsed() < Duration::from_secs(2) { Duration::from_millis(120) } else { Duration::from_secs(5) };
            match rx.recv_timeout(delai) {
                Ok(o) => {
                    if let Err(e) = audio.executer(o) {
                        etat.lock().unwrap().erreur = Some(e.message());
                    }
                    while let Ok(o) = rx.try_recv() {
                        let _ = audio.executer(o);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(_) => {}
            }
            if let Ok(e) = audio.etat() {
                *etat.lock().unwrap() = e;
            }
        }
    });
}

/// Raccourci Ctrl+Alt+A dans son propre fil.
fn fil_raccourci(tx: Sender<()>) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
    let (ok_tx, ok_rx) = channel();
    std::thread::spawn(move || unsafe {
        let ok = crate::outil::raccourci(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'A' as u32) != 0;
        let _ = ok_tx.send(ok);
        if !ok {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_HOTKEY && tx.send(()).is_err() {
                return;
            }
        }
    });
    ok_rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
}

pub struct Mixeur {
    r: Reglages,
    etat: Arc<Mutex<Etat>>,
    rapide: Arc<Mutex<Instant>>,
    tx: Sender<Ordre>,
    raccourci: Option<Receiver<()>>,
    raccourci_pris: bool,
    dernier_jeu: Option<String>,
    message: String,
    glisse: BTreeMap<String, f32>, // volumes en cours de réglage (évite le retour en arrière pendant le glisser)
}

impl Mixeur {
    pub fn new() -> Self {
        let r: Reglages = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let (tx, rx) = channel();
        let etat: Arc<Mutex<Etat>> = Arc::default();
        let rapide = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(60)));
        fil(rx, etat.clone(), rapide.clone());
        let mut m = Mixeur { r, etat, rapide, tx, raccourci: None, raccourci_pris: false, dernier_jeu: None, message: String::new(), glisse: BTreeMap::new() };
        m.regler_raccourci();
        m
    }

    fn regler_raccourci(&mut self) {
        if self.r.raccourci && self.raccourci.is_none() {
            let (tx, rx) = channel();
            if fil_raccourci(tx) {
                self.raccourci = Some(rx);
                self.raccourci_pris = false;
            } else {
                self.raccourci_pris = true;
            }
        }
        // désactivé : on ignore simplement les appuis (le fil reste, sans effet)
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&self.r).unwrap_or_default());
    }

    fn sortie_suivante(&mut self) {
        let e = self.etat.lock().unwrap().clone();
        if let Some(id) = suivante(&e.sorties, &self.r.favoris, e.defaut.as_deref()) {
            let nom = e.sorties.iter().find(|s| s.0 == id).map(|s| s.1.clone()).unwrap_or_default();
            let _ = self.tx.send(Ordre::Sortie(id));
            crate::systeme::notifier("Sortie audio", &nom);
            self.message = format!("Sortie : {nom}");
        }
    }

    fn appliquer_profil(&mut self, jeu: &str) {
        let Some(p) = self.r.profils.get(jeu).cloned() else { return };
        if let Some(s) = p.sortie {
            let _ = self.tx.send(Ordre::Sortie(s));
        }
        for (exe, v) in p.volumes {
            let _ = self.tx.send(Ordre::VolumeApp(exe, v));
        }
        self.message = format!("Profil audio « {jeu} » appliqué.");
    }

    /// Pour les autres outils (Ne pas déranger) : envoie un ordre au fil audio.
    pub fn envoyer(&self, o: Ordre) {
        let _ = self.tx.send(o);
    }

    /// Applis qui jouent du son, regroupées par exe.
    pub fn applis(&self) -> Vec<Session> {
        par_appli(&self.etat.lock().unwrap().sessions)
    }

    pub fn resume(&self) -> String {
        let e = self.etat.lock().unwrap();
        let sortie = e.defaut.as_ref().and_then(|d| e.sorties.iter().find(|s| &s.0 == d)).map(|s| s.1.clone()).unwrap_or_else(|| "?".into());
        format!("{sortie} · {:.0} %{}", e.maitre * 100.0, if e.maitre_muet { " (muet)" } else { "" })
    }

    /// À chaque image : raccourci et profils de jeu.
    pub fn tic(&mut self, jeu: Option<&str>) {
        let appuis = self.raccourci.as_ref().map(|r| r.try_iter().count()).unwrap_or(0);
        if appuis > 0 && self.r.raccourci {
            self.sortie_suivante();
        }
        if jeu.map(str::to_string) != self.dernier_jeu {
            self.dernier_jeu = jeu.map(str::to_string);
            if let (Some(j), true) = (jeu, self.r.profils_actifs) {
                self.appliquer_profil(j);
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        *self.rapide.lock().unwrap() = Instant::now();
        ui.ctx().request_repaint_after(Duration::from_millis(150));
        ui.heading("Mixeur audio");
        let e = self.etat.lock().unwrap().clone();
        if let Some(err) = &e.erreur {
            ui.colored_label(Color32::LIGHT_RED, err);
        }
        let avant = self.r.clone();
        // ---- sortie ----
        ui.horizontal(|ui| {
            ui.strong("Sortie");
            let actuelle = e.defaut.as_ref().and_then(|d| e.sorties.iter().find(|s| &s.0 == d)).map(|s| s.1.clone()).unwrap_or_else(|| "?".into());
            egui::ComboBox::from_id_salt("sortie").selected_text(actuelle).width(280.0).show_ui(ui, |ui| {
                for (id, nom) in &e.sorties {
                    if ui.selectable_label(e.defaut.as_ref() == Some(id), nom).clicked() {
                        let _ = self.tx.send(Ordre::Sortie(id.clone()));
                    }
                }
            });
            if ui.button("⇄ Suivante").on_hover_text("Ctrl+Alt+A").clicked() {
                self.sortie_suivante();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.weak("Ctrl+Alt+A passe de l'une à l'autre parmi :");
            for (id, nom) in &e.sorties {
                let mut fav = self.r.favoris.contains(id);
                if ui.checkbox(&mut fav, nom).changed() {
                    if fav {
                        self.r.favoris.push(id.clone());
                    } else {
                        self.r.favoris.retain(|f| f != id);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.r.raccourci, "Raccourci Ctrl+Alt+A");
            if self.raccourci_pris {
                ui.colored_label(Color32::LIGHT_RED, "déjà pris par une autre appli");
            }
        });
        ui.separator();
        // ---- volumes ----
        let barre = |ui: &mut egui::Ui, crete: f32| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(90.0, 8.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 3.0, Color32::from_black_alpha(80));
            let mut p = r;
            p.set_width(r.width() * crete.clamp(0.0, 1.0));
            let c = if crete > 0.9 { Color32::from_rgb(255, 90, 90) } else { Color32::from_rgb(110, 220, 140) };
            ui.painter().rect_filled(p, 3.0, c);
        };
        egui::Grid::new("volumes").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.strong("🔊 Général");
            let mut v = self.glisse.get("__maitre").copied().unwrap_or(e.maitre) * 100.0;
            let rep = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix(" %").fixed_decimals(0));
            if rep.changed() {
                self.glisse.insert("__maitre".into(), v / 100.0);
                let _ = self.tx.send(Ordre::Maitre(v / 100.0));
            }
            if !rep.dragged() {
                self.glisse.remove("__maitre");
            }
            if ui.button(if e.maitre_muet { "🔇" } else { "🔈" }).clicked() {
                let _ = self.tx.send(Ordre::MaitreMuet(!e.maitre_muet));
            }
            barre(ui, e.crete);
            ui.end_row();
            for s in par_appli(&e.sessions) {
                ui.label(s.nom.trim_end_matches(".exe"));
                let mut v = self.glisse.get(&s.nom).copied().unwrap_or(s.volume) * 100.0;
                let rep = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix(" %").fixed_decimals(0));
                if rep.changed() {
                    self.glisse.insert(s.nom.clone(), v / 100.0);
                    let _ = self.tx.send(Ordre::VolumeApp(s.nom.clone(), v / 100.0));
                }
                if !rep.dragged() {
                    self.glisse.remove(&s.nom);
                }
                if ui.button(if s.muet { "🔇" } else { "🔈" }).clicked() {
                    let _ = self.tx.send(Ordre::MuetApp(s.nom.clone(), !s.muet));
                }
                barre(ui, s.crete);
                ui.end_row();
            }
        });
        if e.sessions.is_empty() {
            ui.weak("Aucune appli ne joue de son pour l'instant.");
        }
        ui.separator();
        // ---- profils ----
        ui.strong("Profils par jeu");
        ui.checkbox(&mut self.r.profils_actifs, "Appliquer le profil quand le jeu démarre (détecté via le Lanceur)");
        match &self.dernier_jeu {
            Some(j) => {
                if ui.button(format!("💾 Enregistrer les réglages actuels pour « {j} »")).clicked() {
                    let p = Profil { sortie: e.defaut.clone(), volumes: par_appli(&e.sessions).into_iter().map(|s| (s.nom, s.volume)).collect() };
                    self.r.profils.insert(j.clone(), p);
                    self.message = format!("Profil enregistré pour {j}.");
                }
            }
            None => {
                ui.weak("Lance un jeu puis règle les volumes : tu pourras les enregistrer pour ce jeu.");
            }
        }
        let mut retirer = None;
        for (jeu, p) in &self.r.profils {
            ui.horizontal(|ui| {
                ui.label(format!("🎮 {jeu}"));
                let sortie = p.sortie.as_ref().and_then(|d| e.sorties.iter().find(|s| &s.0 == d)).map(|s| s.1.as_str()).unwrap_or("sortie inchangée");
                ui.weak(format!("{sortie} · {} appli(s)", p.volumes.len()));
                if ui.small_button("Appliquer").clicked() {
                    retirer = Some((jeu.clone(), false));
                }
                if ui.small_button("🗑").clicked() {
                    retirer = Some((jeu.clone(), true));
                }
            });
        }
        if let Some((j, supprimer)) = retirer {
            if supprimer {
                self.r.profils.remove(&j);
            } else {
                self.appliquer_profil(&j);
            }
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if self.r != avant {
            self.regler_raccourci();
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sortie_suivante() {
        let s = vec![("a".to_string(), "Casque".to_string()), ("b".into(), "Enceintes".into()), ("c".into(), "Écran".into())];
        assert_eq!(suivante(&s, &[], Some("a")), Some("b".into()));
        assert_eq!(suivante(&s, &[], Some("c")), Some("a".into()));
        let fav = vec!["a".to_string(), "c".to_string()];
        assert_eq!(suivante(&s, &fav, Some("a")), Some("c".into()));
        assert_eq!(suivante(&s, &fav, Some("b")), Some("a".into()));
        assert_eq!(suivante(&[], &fav, None), None);
    }

    #[test]
    fn regroupement() {
        let s = vec![
            Session { nom: "chrome.exe".into(), volume: 0.5, muet: false, crete: 0.2 },
            Session { nom: "Chrome.exe".into(), volume: 0.5, muet: true, crete: 0.7 },
            Session { nom: "Discord.exe".into(), volume: 1.0, muet: false, crete: 0.0 },
        ];
        let v = par_appli(&s);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].nom, "chrome.exe");
        assert!((v[0].crete - 0.7).abs() < 1e-6 && !v[0].muet);
    }
}
