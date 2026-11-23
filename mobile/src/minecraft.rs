//! Heure locale « AAAAMMJJ_HHMMSS » (comme sous Windows).
pub fn horodatage() -> String {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!("{:04}{:02}{:02}_{:02}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// Heure locale « AAAA-MM-JJ_HH-MM-SS » (pour les dates qu'on relit et compare).
pub fn horodatage_iso() -> String {
    let h = horodatage();
    let g = |a: usize, b: usize| h.get(a..b).unwrap_or("00");
    format!("{}-{}-{}_{}-{}-{}", g(0, 4), g(4, 6), g(6, 8), g(9, 11), g(11, 13), g(13, 15))
}

/// Date et heure à afficher : « 23/11/2026 10:15 ».
pub fn date_lisible() -> String {
    let h = horodatage();
    let g = |a: usize, b: usize| h.get(a..b).unwrap_or("00").to_string();
    format!("{}/{}/{} {}:{}", g(6, 8), g(4, 6), g(0, 4), g(9, 11), g(11, 13))
}
