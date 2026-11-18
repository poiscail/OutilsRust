//! Coffre de fichiers : chiffre un dossier ou un fichier en un seul fichier « .coffre »
//! (zip puis AES-256-GCM par blocs de 1 Mio, clé tirée du mot de passe par Argon2id), et le rouvre.
//! Blocs numérotés + drapeau « dernier bloc » dans le nonce : impossible de réordonner ou tronquer sans erreur.
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use eframe::egui::{self, Color32};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAGIE: &[u8; 8] = b"OUTILSCF";
const VERSION: u8 = 1;
const BLOC: usize = 1 << 20;
const ENTETE: usize = 8 + 1 + 16 + 12 + 7; // magie, version, sel, paramètres argon2 (3 × u32), préfixe de nonce

fn cle(mdp: &str, sel: &[u8], m: u32, t: u32, p: u32) -> Result<[u8; 32], String> {
    let params = argon2::Params::new(m, t, p, Some(32)).map_err(|e| e.to_string())?;
    let mut k = [0u8; 32];
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params).hash_password_into(mdp.as_bytes(), sel, &mut k).map_err(|e| e.to_string())?;
    Ok(k)
}

fn nonce(prefixe: &[u8], n: u32, dernier: bool) -> [u8; 12] {
    let mut x = [0u8; 12];
    x[..7].copy_from_slice(prefixe);
    x[7..11].copy_from_slice(&n.to_be_bytes());
    x[11] = dernier as u8;
    x
}

/// Lit jusqu'à remplir `buf` (ou la fin du fichier) ; renvoie le nombre d'octets lus.
fn remplir(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

pub fn chiffrer_flux(src: &mut impl Read, dest: &mut impl Write, mdp: &str, params: (u32, u32, u32)) -> Result<(), String> {
    let mut sel = [0u8; 16];
    let mut prefixe = [0u8; 7];
    getrandom::getrandom(&mut sel).map_err(|e| e.to_string())?;
    getrandom::getrandom(&mut prefixe).map_err(|e| e.to_string())?;
    let mut entete = Vec::with_capacity(ENTETE);
    entete.extend(MAGIE);
    entete.push(VERSION);
    entete.extend(sel);
    for v in [params.0, params.1, params.2] {
        entete.extend(v.to_le_bytes());
    }
    entete.extend(prefixe);
    let k = cle(mdp, &sel, params.0, params.1, params.2)?;
    let c = Aes256Gcm::new_from_slice(&k).map_err(|e| e.to_string())?;
    dest.write_all(&entete).map_err(|e| e.to_string())?;
    let mut courant = vec![0u8; BLOC];
    let mut suivant = vec![0u8; BLOC];
    let mut n_courant = remplir(src, &mut courant).map_err(|e| e.to_string())?;
    let mut i = 0u32;
    loop {
        let n_suivant = if n_courant == BLOC { remplir(src, &mut suivant).map_err(|e| e.to_string())? } else { 0 };
        let dernier = n_suivant == 0;
        let ct = c.encrypt(Nonce::from_slice(&nonce(&prefixe, i, dernier)), Payload { msg: &courant[..n_courant], aad: &entete }).map_err(|_| "chiffrement impossible")?;
        dest.write_all(&(ct.len() as u32).to_le_bytes()).and_then(|_| dest.write_all(&ct)).map_err(|e| e.to_string())?;
        if dernier {
            return Ok(());
        }
        std::mem::swap(&mut courant, &mut suivant);
        n_courant = n_suivant;
        i = i.checked_add(1).ok_or("fichier trop gros")?;
    }
}

pub fn dechiffrer_flux(src: &mut impl Read, dest: &mut impl Write, mdp: &str) -> Result<(), String> {
    let mut entete = [0u8; ENTETE];
    src.read_exact(&mut entete).map_err(|_| "fichier trop court")?;
    if &entete[..8] != MAGIE || entete[8] != VERSION {
        return Err("ce n'est pas un fichier .coffre d'Outils".into());
    }
    let sel = &entete[9..25];
    let u = |o: usize| u32::from_le_bytes(entete[o..o + 4].try_into().unwrap());
    let (m, t, p) = (u(25), u(29), u(33));
    if m > 1 << 21 || t > 64 || p > 64 {
        return Err("paramètres invalides".into());
    }
    let prefixe = &entete[37..44];
    let k = cle(mdp, sel, m, t, p)?;
    let c = Aes256Gcm::new_from_slice(&k).map_err(|e| e.to_string())?;
    let mut i = 0u32;
    loop {
        let mut l = [0u8; 4];
        if remplir(src, &mut l).map_err(|e| e.to_string())? != 4 {
            return Err("fichier tronqué".into());
        }
        let l = u32::from_le_bytes(l) as usize;
        if l > BLOC + 16 {
            return Err("fichier abîmé".into());
        }
        let mut ct = vec![0u8; l];
        src.read_exact(&mut ct).map_err(|_| "fichier tronqué")?;
        // on essaie « bloc normal », puis « dernier bloc »
        let essai = |dernier| c.decrypt(Nonce::from_slice(&nonce(prefixe, i, dernier)), Payload { msg: &ct, aad: &entete }).ok();
        if let Some(clair) = essai(false) {
            dest.write_all(&clair).map_err(|e| e.to_string())?;
        } else if let Some(clair) = essai(true) {
            dest.write_all(&clair).map_err(|e| e.to_string())?;
            let mut reste = [0u8; 1];
            if src.read(&mut reste).map_err(|e| e.to_string())? != 0 {
                return Err("données en trop après la fin".into());
            }
            return Ok(());
        } else {
            return Err(if i == 0 { "mot de passe incorrect".into() } else { "fichier abîmé ou modifié".into() });
        }
        i += 1;
    }
}

const PARAMS: (u32, u32, u32) = (65536, 3, 4); // 64 Mio, 3 passes, 4 voies (comme le Coffre)

/// Zippe `src` (dossier ou fichier) puis chiffre dans `src.coffre`.
pub fn chiffrer(src: &Path, mdp: &str) -> Result<PathBuf, String> {
    let parent = src.parent().ok_or("chemin invalide")?;
    let nom = src.file_name().ok_or("chemin invalide")?.to_string_lossy().into_owned();
    let dest = parent.join(format!("{nom}.coffre"));
    if dest.exists() {
        return Err(format!("{} existe déjà", dest.display()));
    }
    let tmp = std::env::temp_dir().join(format!("outils_coffre_{}.zip", std::process::id()));
    if src.is_dir() {
        crate::minecraft::zipper(src, parent, &tmp)?;
    } else {
        let mut z = zip::ZipWriter::new(File::create(&tmp).map_err(|e| e.to_string())?);
        z.start_file(nom.clone(), zip::write::SimpleFileOptions::default().large_file(true)).map_err(|e| e.to_string())?;
        std::io::copy(&mut File::open(src).map_err(|e| e.to_string())?, &mut z).map_err(|e| e.to_string())?;
        z.finish().map_err(|e| e.to_string())?;
    }
    let r = (|| {
        let mut entree = std::io::BufReader::new(File::open(&tmp).map_err(|e| e.to_string())?);
        let mut sortie = std::io::BufWriter::new(File::create(&dest).map_err(|e| e.to_string())?);
        chiffrer_flux(&mut entree, &mut sortie, mdp, PARAMS)?;
        sortie.flush().map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(&tmp);
    if r.is_err() {
        let _ = fs::remove_file(&dest);
    }
    r.map(|_| dest)
}

/// Déchiffre `coffre` et extrait son contenu à côté (dans « … (ouvert) » si le nom est pris).
pub fn ouvrir(coffre: &Path, mdp: &str) -> Result<PathBuf, String> {
    let tmp = std::env::temp_dir().join(format!("outils_ouvre_{}.zip", std::process::id()));
    let r = (|| {
        let mut entree = std::io::BufReader::new(File::open(coffre).map_err(|e| e.to_string())?);
        let mut sortie = std::io::BufWriter::new(File::create(&tmp).map_err(|e| e.to_string())?);
        dechiffrer_flux(&mut entree, &mut sortie, mdp)?;
        sortie.flush().map_err(|e| e.to_string())?;
        drop(sortie);
        let parent = coffre.parent().ok_or("chemin invalide")?;
        let nom = coffre.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let mut dest = parent.to_path_buf();
        if parent.join(&nom).exists() {
            dest = parent.join(format!("{nom} (ouvert)"));
        }
        let mut z = zip::ZipArchive::new(File::open(&tmp).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        z.extract(&dest).map_err(|e| e.to_string())?;
        Ok(if dest == parent { parent.join(&nom) } else { dest })
    })();
    let _ = fs::remove_file(&tmp);
    r
}

#[derive(Default)]
struct Etat {
    occupe: bool,
    message: String,
    resultat: Option<PathBuf>,
}

pub struct CoffreFichiers {
    cible: Option<PathBuf>,
    mdp: String,
    mdp2: String,
    supprimer: bool,
    e: Arc<Mutex<Etat>>,
}

impl CoffreFichiers {
    pub fn new() -> Self {
        CoffreFichiers { cible: None, mdp: String::new(), mdp2: String::new(), supprimer: false, e: Arc::default() }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Coffre de fichiers");
        ui.weak("Chiffre un dossier ou un fichier en un seul fichier .coffre (AES-256, mot de passe). Sans le mot de passe, personne ne peut l'ouvrir, toi compris.");
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.cible = Some(p);
        }
        let (occupe, message, resultat) = {
            let x = self.e.lock().unwrap();
            (x.occupe, x.message.clone(), x.resultat.clone())
        };
        ui.horizontal(|ui| {
            if ui.button("📁 Dossier…").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.cible = Some(p);
                }
            }
            if ui.button("📄 Fichier ou .coffre…").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    self.cible = Some(p);
                }
            }
            ui.weak("ou glisse-le ici");
        });
        let Some(cible) = self.cible.clone() else { return };
        let ouvrir_mode = cible.extension().is_some_and(|e| e.eq_ignore_ascii_case("coffre"));
        ui.separator();
        ui.strong(cible.display().to_string());
        ui.add(egui::TextEdit::singleline(&mut self.mdp).password(true).hint_text("mot de passe").desired_width(260.0));
        if !ouvrir_mode {
            ui.add(egui::TextEdit::singleline(&mut self.mdp2).password(true).hint_text("confirme le mot de passe").desired_width(260.0));
            ui.checkbox(&mut self.supprimer, "Mettre l'original à la corbeille une fois chiffré");
        }
        let pret = !self.mdp.is_empty() && (ouvrir_mode || (self.mdp == self.mdp2 && self.mdp.chars().count() >= 8));
        if !ouvrir_mode && !self.mdp.is_empty() && self.mdp.chars().count() < 8 {
            ui.colored_label(Color32::LIGHT_RED, "8 caractères minimum.");
        } else if !ouvrir_mode && !self.mdp2.is_empty() && self.mdp != self.mdp2 {
            ui.colored_label(Color32::LIGHT_RED, "Les deux mots de passe diffèrent.");
        }
        ui.horizontal(|ui| {
            let texte = if ouvrir_mode { "🔓 Ouvrir" } else { "🔒 Chiffrer" };
            if ui.add_enabled(pret && !occupe, egui::Button::new(texte)).clicked() {
                let (e, mdp, sup) = (self.e.clone(), std::mem::take(&mut self.mdp), self.supprimer);
                self.mdp2.clear();
                {
                    let mut x = e.lock().unwrap();
                    x.occupe = true;
                    x.message = if ouvrir_mode { "Ouverture…".into() } else { "Chiffrement…".into() };
                    x.resultat = None;
                }
                std::thread::spawn(move || {
                    let r = if ouvrir_mode { ouvrir(&cible, &mdp) } else { chiffrer(&cible, &mdp) };
                    let mut x = e.lock().unwrap();
                    x.occupe = false;
                    match r {
                        Ok(p) => {
                            x.message = if ouvrir_mode { format!("Ouvert dans {}", p.display()) } else { format!("Chiffré : {}", p.display()) };
                            if !ouvrir_mode && sup {
                                if crate::disque::corbeille(&cible) {
                                    x.message += " · original à la corbeille";
                                } else {
                                    x.message += " · l'original n'a pas pu être supprimé";
                                }
                            }
                            x.resultat = Some(p);
                        }
                        Err(m) => x.message = format!("Échec : {m}"),
                    }
                });
            }
            if occupe {
                ui.spinner();
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            }
        });
        if !message.is_empty() {
            ui.label(message);
        }
        if let Some(p) = resultat {
            if ui.button("📂 Montrer").clicked() {
                crate::disque::montrer_dans_explorateur(&p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAPIDE: (u32, u32, u32) = (64, 1, 1);

    fn aller_retour(taille: usize) {
        let clair: Vec<u8> = (0..taille).map(|i| (i * 7 % 251) as u8).collect();
        let mut ct = vec![];
        chiffrer_flux(&mut &clair[..], &mut ct, "secret", RAPIDE).unwrap();
        let mut sortie = vec![];
        dechiffrer_flux(&mut &ct[..], &mut sortie, "secret").unwrap();
        assert_eq!(sortie, clair);
        let mut x = vec![];
        assert_eq!(dechiffrer_flux(&mut &ct[..], &mut x, "faux").unwrap_err(), "mot de passe incorrect");
    }

    #[test]
    fn tailles() {
        aller_retour(0);
        aller_retour(10);
        aller_retour(BLOC);
        aller_retour(BLOC * 2 + 5);
    }

    #[test]
    fn troncature_detectee() {
        let clair = vec![1u8; BLOC + 10];
        let mut ct = vec![];
        chiffrer_flux(&mut &clair[..], &mut ct, "s", RAPIDE).unwrap();
        // on garde l'en-tête et le premier bloc seulement
        let premier = u32::from_le_bytes(ct[ENTETE..ENTETE + 4].try_into().unwrap()) as usize;
        let tronque = &ct[..ENTETE + 4 + premier];
        let mut x = vec![];
        assert!(dechiffrer_flux(&mut &tronque[..], &mut x, "s").is_err());
    }

    #[test]
    fn dossier() {
        let base = std::env::temp_dir().join(format!("outils_cf_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let d = base.join("Secret");
        fs::create_dir_all(d.join("sous")).unwrap();
        fs::write(d.join("sous/a.txt"), "bonjour").unwrap();
        let c = chiffrer(&d, "motdepasse").unwrap();
        fs::remove_dir_all(&d).unwrap();
        let o = ouvrir(&c, "motdepasse").unwrap();
        assert_eq!(fs::read_to_string(o.join("sous/a.txt")).unwrap(), "bonjour");
        let _ = fs::remove_dir_all(&base);
    }
}
