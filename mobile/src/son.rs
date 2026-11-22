//! Petit synthétiseur (jeu de rythme) : grosse caisse, charleston et notes, mixés en direct.
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Son {
    Caisse,
    Charley,
    Note(f32),
}

struct Voix {
    son: Son,
    t: f32,
}

#[derive(Clone)]
pub struct Synthe {
    file: Arc<Mutex<Vec<Son>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Synthe {
    fn drop(&mut self) {
        if Arc::strong_count(&self.stop) == 1 {
            self.stop.store(true, Relaxed);
        }
    }
}

/// Échantillon d'une voix au temps `t` (secondes) ; None quand elle est finie.
pub fn echantillon(s: Son, t: f32, bruit: &mut u32) -> Option<f32> {
    let env = |d: f32| if t > d { None } else { Some((1.0 - t / d).powi(2)) };
    match s {
        Son::Caisse => {
            let f = 150.0 * (-t * 25.0).exp() + 45.0;
            env(0.25).map(|e| (t * f * std::f32::consts::TAU).sin() * e * 0.9)
        }
        Son::Charley => {
            *bruit ^= *bruit << 13;
            *bruit ^= *bruit >> 17;
            *bruit ^= *bruit << 5;
            env(0.05).map(|e| (*bruit as f32 / u32::MAX as f32 * 2.0 - 1.0) * e * 0.3)
        }
        Son::Note(f) => env(0.3).map(|e| ((t * f * std::f32::consts::TAU).sin() + 0.3 * (t * f * 2.0 * std::f32::consts::TAU).sin()) * e * 0.35),
    }
}

impl Synthe {
    pub fn demarrer() -> Synthe {
        let s = Synthe { file: Arc::default(), stop: Arc::default() };
        #[cfg(target_os = "android")]
        {
            let (file, stop) = (s.file.clone(), s.stop.clone());
            std::thread::spawn(move || {
                use ndk::audio::*;
                let Ok(flux) = AudioStreamBuilder::new().map(|b| b.direction(AudioDirection::Output).channel_count(1).sample_rate(48_000).format(AudioFormat::PCM_Float).performance_mode(AudioPerformanceMode::LowLatency)).and_then(|b| b.open_stream()) else { return };
                if flux.request_start().is_err() {
                    return;
                }
                let mut voix: Vec<Voix> = vec![];
                let mut bruit = 0x1234_5678u32;
                let mut tampon = vec![0f32; 256];
                while !stop.load(Relaxed) {
                    voix.extend(file.lock().unwrap().drain(..).map(|son| Voix { son, t: 0.0 }));
                    for x in tampon.iter_mut() {
                        let mut v = 0.0;
                        voix.retain_mut(|vo| {
                            let e = echantillon(vo.son, vo.t, &mut bruit);
                            vo.t += 1.0 / 48_000.0;
                            if let Some(e) = e {
                                v += e;
                            }
                            e.is_some()
                        });
                        *x = v.clamp(-1.0, 1.0);
                    }
                    let _ = unsafe { flux.write(tampon.as_ptr().cast(), tampon.len() as i32, 100_000_000) };
                }
                let _ = flux.request_stop();
            });
        }
        s
    }

    pub fn jouer(&self, s: Son) {
        if let Ok(mut f) = self.file.lock() {
            f.push(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voix() {
        let mut b = 1;
        assert!(echantillon(Son::Caisse, 0.01, &mut b).is_some());
        assert!(echantillon(Son::Caisse, 0.3, &mut b).is_none());
        assert!(echantillon(Son::Charley, 0.06, &mut b).is_none());
        let n: f32 = (0..100).map(|k| echantillon(Son::Note(440.0), k as f32 / 48_000.0, &mut b).unwrap().abs()).sum();
        assert!(n > 1.0);
    }
}
