//! Vraie manette Xbox virtuelle (31) : les boutons du téléphone deviennent une manette Xbox 360 que
//! Windows et les jeux reconnaissent (pilote ViGEmBus). Commandes reçues du téléphone :
//! « k xbox:a down » (boutons) et « x lx 0.5 » (axes : lx ly rx ry de −1 à 1, lt rt de 0 à 1).
use std::sync::Mutex;
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

/// État affiché dans Outils (« branchée », erreur…).
pub static ETAT: Mutex<String> = Mutex::new(String::new());

/// Bit de bouton d'un nom envoyé par le téléphone.
pub fn bit(nom: &str) -> Option<u16> {
    Some(match nom {
        "a" => XButtons::A,
        "b" => XButtons::B,
        "x" => XButtons::X,
        "y" => XButtons::Y,
        "lb" => XButtons::LB,
        "rb" => XButtons::RB,
        "start" => XButtons::START,
        "back" => XButtons::BACK,
        "l3" => XButtons::LTHUMB,
        "r3" => XButtons::RTHUMB,
        "guide" => XButtons::GUIDE,
        "haut" => XButtons::UP,
        "bas" => XButtons::DOWN,
        "gauche" => XButtons::LEFT,
        "droite" => XButtons::RIGHT,
        _ => return None,
    })
}

pub struct Manette {
    cible: Option<Xbox360Wired<Client>>,
    pad: XGamepad,
    essai: Option<std::time::Instant>,
}

impl Default for Manette {
    fn default() -> Self {
        Self::new()
    }
}

impl Manette {
    pub fn new() -> Self {
        Manette { cible: None, pad: XGamepad::default(), essai: None }
    }

    fn brancher(&mut self) -> bool {
        if self.cible.is_some() {
            return true;
        }
        // on ne réessaie pas en boucle si le pilote manque
        if self.essai.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5)) {
            return false;
        }
        self.essai = Some(std::time::Instant::now());
        let r = (|| -> Result<Xbox360Wired<Client>, String> {
            let c = Client::connect().map_err(|_| "pilote ViGEmBus absent".to_string())?;
            let mut t = Xbox360Wired::new(c, TargetId::XBOX360_WIRED);
            t.plugin().map_err(|e| format!("{e:?}"))?;
            t.wait_ready().map_err(|e| format!("{e:?}"))?;
            Ok(t)
        })();
        match r {
            Ok(t) => {
                self.cible = Some(t);
                *ETAT.lock().unwrap() = "🎮 Manette Xbox virtuelle branchée".into();
                true
            }
            Err(e) => {
                *ETAT.lock().unwrap() = format!("✖ {e} : installe ViGEmBus (bouton ci-dessous)");
                false
            }
        }
    }

    fn envoyer(&mut self) {
        if self.brancher() {
            let pad = self.pad;
            if let Some(t) = &mut self.cible {
                if t.update(&pad).is_err() {
                    self.cible = None;
                }
            }
        }
    }

    /// Bouton (ou gâchette / stick poussé à fond) appuyé ou relâché.
    pub fn bouton(&mut self, nom: &str, appuye: bool) {
        let p = &mut self.pad;
        match nom {
            "lt" => p.left_trigger = if appuye { 255 } else { 0 },
            "rt" => p.right_trigger = if appuye { 255 } else { 0 },
            "stick_haut" => p.thumb_ly = if appuye { i16::MAX } else { 0 },
            "stick_bas" => p.thumb_ly = if appuye { i16::MIN } else { 0 },
            "stick_gauche" => p.thumb_lx = if appuye { i16::MIN } else { 0 },
            "stick_droite" => p.thumb_lx = if appuye { i16::MAX } else { 0 },
            n => {
                let Some(b) = bit(n) else { return };
                if appuye {
                    p.buttons.raw |= b;
                } else {
                    p.buttons.raw &= !b;
                }
            }
        }
        self.envoyer();
    }

    /// Axe analogique.
    pub fn axe(&mut self, nom: &str, v: f32) {
        let s = |v: f32| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        let g = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
        let p = &mut self.pad;
        match nom {
            "lx" => p.thumb_lx = s(v),
            "ly" => p.thumb_ly = s(v),
            "rx" => p.thumb_rx = s(v),
            "ry" => p.thumb_ry = s(v),
            "lt" => p.left_trigger = g(v),
            "rt" => p.right_trigger = g(v),
            _ => return,
        }
        self.envoyer();
    }

    /// Tout relâcher (téléphone déconnecté).
    pub fn relacher(&mut self) {
        if self.pad != XGamepad::default() {
            self.pad = XGamepad::default();
            self.envoyer();
        }
    }
}

/// Installe le pilote ViGEmBus (winget, demande l'autorisation).
pub fn installer_pilote() -> Result<String, String> {
    crate::majlogiciels::winget(&["install", "--id", "ViGEm.ViGEmBus", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manette() {
        assert_eq!(bit("a"), Some(0x1000));
        assert_eq!(bit("haut"), Some(0x0001));
        assert_eq!(bit("zz"), None);
        // sans pilote : l'état reste neutre, pas de plantage
        let mut m = Manette { cible: None, pad: XGamepad::default(), essai: Some(std::time::Instant::now()) };
        m.bouton("a", true);
        assert_eq!(m.pad.buttons.raw, 0x1000);
        m.axe("lx", -1.0);
        assert_eq!(m.pad.thumb_lx, -i16::MAX);
        m.bouton("rt", true);
        assert_eq!(m.pad.right_trigger, 255);
    }
}
