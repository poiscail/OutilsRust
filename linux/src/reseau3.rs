//! Fuites de mots de passe (k-anonymat, Have I Been Pwned) : mêmes fonctions que sous Windows.

/// (préfixe envoyé, suffixe gardé) du SHA-1 en majuscules.
pub fn decouper_sha1(mdp: &str) -> (String, String) {
    use sha1::Digest;
    let h: String = sha1::Sha1::digest(mdp.as_bytes()).iter().map(|b| format!("{b:02X}")).collect();
    (h[..5].to_string(), h[5..].to_string())
}

/// Nombre de fuites dans la réponse de l'API (lignes « SUFFIXE:N »).
pub fn compter_fuites(reponse: &str, suffixe: &str) -> u64 {
    reponse.lines().find_map(|l| {
        let (s, n) = l.trim().split_once(':')?;
        s.eq_ignore_ascii_case(suffixe).then(|| n.trim().parse().unwrap_or(0))
    }).unwrap_or(0)
}
