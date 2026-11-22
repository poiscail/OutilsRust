//! Téléphone : police du système et écriture de texte dans une image (comme `creatif.rs` du PC).
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use image::RgbaImage;

/// Police du téléphone : les noms Windows demandés sont remplacés par les polices d'Android.
pub(crate) fn police(noms: &[&str]) -> Option<FontVec> {
    let symboles = noms.iter().any(|n| n.contains("sym") || n.contains("emj") || n.contains("uni") || n.contains("DejaVu"));
    let liste: &[&str] = if symboles {
        &["NotoSansSymbols-Regular-Subsetted.ttf", "NotoSansSymbols-Regular-Subsetted2.ttf", "NotoSansSymbols2-Regular.ttf", "DejaVuSans.ttf", "DroidSansFallback.ttf", "Roboto-Regular.ttf"]
    } else {
        &["Roboto-Bold.ttf", "Roboto-Black.ttf", "RobotoStatic-Regular.ttf", "Roboto-Regular.ttf", "DroidSans-Bold.ttf", "DroidSans.ttf"]
    };
    liste.iter().find_map(|n| FontVec::try_from_vec(std::fs::read(format!("/system/fonts/{n}")).ok()?).ok())
}

pub(crate) fn largeur(f: &FontVec, taille: f32, t: &str) -> f32 {
    let s = f.as_scaled(PxScale::from(taille));
    t.chars().map(|c| s.h_advance(f.glyph_id(c))).sum()
}

pub fn ecrire(img: &mut RgbaImage, f: &FontVec, taille: f32, x: f32, y: f32, t: &str, couleur: [u8; 3], contour: Option<([u8; 3], i32)>) {
    let s = f.as_scaled(PxScale::from(taille));
    let mut passes: Vec<(i32, i32, [u8; 3])> = vec![];
    if let Some((c, e)) = contour {
        for dy in -e..=e {
            for dx in -e..=e {
                if dx * dx + dy * dy <= e * e && (dx, dy) != (0, 0) {
                    passes.push((dx, dy, c));
                }
            }
        }
    }
    passes.push((0, 0, couleur));
    for (dx, dy, c) in passes {
        let mut cx = x;
        for ch in t.chars() {
            let id = f.glyph_id(ch);
            let g = id.with_scale_and_position(PxScale::from(taille), ab_glyph::point(cx + dx as f32, y + s.ascent() + dy as f32));
            if let Some(o) = f.outline_glyph(g) {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() && cov > 0.0 {
                        let p = img.get_pixel_mut(px as u32, py as u32);
                        for k in 0..3 {
                            p[k] = (p[k] as f32 * (1.0 - cov) + c[k] as f32 * cov) as u8;
                        }
                        p[3] = p[3].max((cov * 255.0) as u8);
                    }
                });
            }
            cx += s.h_advance(id);
        }
    }
}
