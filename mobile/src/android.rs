//! Accès au téléphone : presse-papiers, notifications, permissions, lampe, vibreur, fichiers, Wi-Fi,
//! applis installées (JNI), capteurs, micro et appareil photo (NDK).
//! Hors Android (essai sur PC), chaque fonction a un remplaçant inoffensif.

#[derive(Clone, Debug, Default)]
pub struct InfosWifi {
    pub ssid: String,
    pub rssi: i32,
    pub vitesse: i32,
    pub frequence: i32,
    pub ip: String,
    pub passerelle: String,
    pub masque: String,
    pub dns: String,
}

#[derive(Clone, Debug, Default)]
pub struct Appli {
    pub paquet: String,
    pub nom: String,
    pub systeme: bool,
    pub installateur: Option<String>,
    pub permissions: Vec<String>,
    pub installee: u64, // secondes Unix
}

/// Racine du stockage partagé (Téléchargements, DCIM…).
pub const STOCKAGE: &str = "/storage/emulated/0";

#[cfg(target_os = "android")]
mod jni_ {
    use super::*;
    use jni::objects::{JObject, JObjectArray, JString, JValue};
    use jni::{JNIEnv, JavaVM};

    pub fn avec<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let activite = unsafe { JObject::from_raw(ctx.context().cast()) };
        // certains services créent un Handler : il faut un Looper Java sur ce fil
        let _ = (|| -> jni::errors::Result<()> {
            let l = env.call_static_method("android/os/Looper", "myLooper", "()Landroid/os/Looper;", &[])?.l()?;
            if l.is_null() {
                env.call_static_method("android/os/Looper", "prepare", "()V", &[])?;
            }
            Ok(())
        })();
        let _ = env.exception_clear();
        let r = env.with_local_frame(64, |env| f(env, &activite));
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        r.ok()
    }

    pub fn chaine(env: &mut JNIEnv, o: &JObject) -> jni::errors::Result<String> {
        if o.is_null() {
            return Ok(String::new());
        }
        // CharSequence → String
        let s = env.call_method(o, "toString", "()Ljava/lang/String;", &[])?.l()?;
        let js = JString::from(s);
        let r: String = env.get_string(&js)?.into();
        Ok(r)
    }

    pub fn service<'a>(env: &mut JNIEnv<'a>, act: &JObject, nom: &str) -> jni::errors::Result<JObject<'a>> {
        let n = env.new_string(nom)?;
        env.call_method(act, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[JValue::Object(&n)])?.l()
    }

    pub fn sdk(env: &mut JNIEnv) -> i32 {
        env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I").and_then(|v| v.i()).unwrap_or(26)
    }

    pub fn tableau<'a>(env: &mut JNIEnv<'a>, v: &[&str]) -> jni::errors::Result<JObjectArray<'a>> {
        let t = env.new_object_array(v.len() as i32, "java/lang/String", JObject::null())?;
        for (k, s) in v.iter().enumerate() {
            let j = env.new_string(s)?;
            env.set_object_array_element(&t, k as i32, j)?;
        }
        Ok(t)
    }

    pub fn paquet(env: &mut JNIEnv, act: &JObject) -> jni::errors::Result<String> {
        let p = env.call_method(act, "getPackageName", "()Ljava/lang/String;", &[])?.l()?;
        chaine(env, &p)
    }

    pub fn intention<'a>(env: &mut JNIEnv<'a>, action: &str, uri: Option<&str>) -> jni::errors::Result<JObject<'a>> {
        let a = env.new_string(action)?;
        let i = env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&a)])?;
        if let Some(u) = uri {
            let u = env.new_string(u)?;
            let uri = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[JValue::Object(&u)])?.l()?;
            env.call_method(&i, "setData", "(Landroid/net/Uri;)Landroid/content/Intent;", &[JValue::Object(&uri)])?;
        }
        Ok(i)
    }

    pub fn ip(v: i32) -> String {
        let b = v.to_le_bytes();
        format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
    }
}

// ---------------------------------------------------------------------------------------------------
// Presse-papiers
// ---------------------------------------------------------------------------------------------------

pub fn presse_lire() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        return jni_::avec(|env, act| {
            let cm = jni_::service(env, act, "clipboard")?;
            let clip = env.call_method(&cm, "getPrimaryClip", "()Landroid/content/ClipData;", &[])?.l()?;
            if clip.is_null() {
                return Ok(String::new());
            }
            let item = env.call_method(&clip, "getItemAt", "(I)Landroid/content/ClipData$Item;", &[JValue::Int(0)])?.l()?;
            let t = env.call_method(&item, "coerceToText", "(Landroid/content/Context;)Ljava/lang/CharSequence;", &[JValue::Object(act)])?.l()?;
            jni_::chaine(env, &t)
        });
    }
    #[allow(unreachable_code)]
    None
}

pub fn presse_ecrire(texte: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        return jni_::avec(|env, act| {
            let cm = jni_::service(env, act, "clipboard")?;
            let (l, t) = (env.new_string("Outils")?, env.new_string(texte)?);
            let clip = env.call_static_method("android/content/ClipData", "newPlainText", "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;", &[JValue::Object(&l), JValue::Object(&t)])?.l()?;
            env.call_method(&cm, "setPrimaryClip", "(Landroid/content/ClipData;)V", &[JValue::Object(&clip)])?;
            Ok(())
        })
        .is_some();
    }
    #[allow(unreachable_code)]
    {
        let _ = texte;
        false
    }
}

// ---------------------------------------------------------------------------------------------------
// Notifications et vibreur
// ---------------------------------------------------------------------------------------------------

/// Vraie notification Android (dans la barre du haut), qui rouvre l'appli quand on la touche.
pub fn notifier(id: i32, titre: &str, texte: &str) {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        let _ = jni_::avec(|env, act| {
            let nm = jni_::service(env, act, "notification")?;
            let canal = env.new_string("outils")?;
            if jni_::sdk(env) >= 26 {
                let nom = env.new_string("Rappels d'Outils")?;
                let c = env.new_object("android/app/NotificationChannel", "(Ljava/lang/String;Ljava/lang/CharSequence;I)V", &[JValue::Object(&canal), JValue::Object(&nom), JValue::Int(4)])?;
                env.call_method(&nm, "createNotificationChannel", "(Landroid/app/NotificationChannel;)V", &[JValue::Object(&c)])?;
            }
            let b = env.new_object("android/app/Notification$Builder", "(Landroid/content/Context;Ljava/lang/String;)V", &[JValue::Object(act), JValue::Object(&canal)])?;
            let icone = env.get_static_field("android/R$drawable", "ic_dialog_info", "I")?.i()?;
            env.call_method(&b, "setSmallIcon", "(I)Landroid/app/Notification$Builder;", &[JValue::Int(icone)])?;
            let (t, m) = (env.new_string(titre)?, env.new_string(texte)?);
            env.call_method(&b, "setContentTitle", "(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;", &[JValue::Object(&t)])?;
            env.call_method(&b, "setContentText", "(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;", &[JValue::Object(&m)])?;
            env.call_method(&b, "setAutoCancel", "(Z)Landroid/app/Notification$Builder;", &[JValue::Bool(1)])?;
            // toucher la notification rouvre l'appli
            let pm = env.call_method(act, "getPackageManager", "()Landroid/content/pm/PackageManager;", &[])?.l()?;
            let p = env.call_method(act, "getPackageName", "()Ljava/lang/String;", &[])?.l()?;
            let i = env.call_method(&pm, "getLaunchIntentForPackage", "(Ljava/lang/String;)Landroid/content/Intent;", &[JValue::Object(&p)])?.l()?;
            if !i.is_null() {
                let pi = env.call_static_method("android/app/PendingIntent", "getActivity", "(Landroid/content/Context;ILandroid/content/Intent;I)Landroid/app/PendingIntent;", &[JValue::Object(act), JValue::Int(0), JValue::Object(&i), JValue::Int(0x0400_0000 | 0x0800_0000)])?.l()?;
                env.call_method(&b, "setContentIntent", "(Landroid/app/PendingIntent;)Landroid/app/Notification$Builder;", &[JValue::Object(&pi)])?;
            }
            let n = env.call_method(&b, "build", "()Landroid/app/Notification;", &[])?.l()?;
            env.call_method(&nm, "notify", "(ILandroid/app/Notification;)V", &[JValue::Int(id), JValue::Object(&n)])?;
            Ok(())
        });
    }
    #[cfg(not(target_os = "android"))]
    log::info!("notification {id} : {titre} — {texte}");
}

pub fn vibrer(ms: i64) {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        let _ = jni_::avec(|env, act| {
            let v = jni_::service(env, act, "vibrator")?;
            let e = env.call_static_method("android/os/VibrationEffect", "createOneShot", "(JI)Landroid/os/VibrationEffect;", &[JValue::Long(ms), JValue::Int(-1)])?.l()?;
            env.call_method(&v, "vibrate", "(Landroid/os/VibrationEffect;)V", &[JValue::Object(&e)])?;
            Ok(())
        });
    }
    let _ = ms;
}

// ---------------------------------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------------------------------

pub fn a_permission(p: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        return jni_::avec(|env, act| {
            let s = env.new_string(p)?;
            Ok(env.call_method(act, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&s)])?.i()? == 0)
        })
        .unwrap_or(false);
    }
    #[allow(unreachable_code)]
    {
        let _ = p;
        true
    }
}

/// Affiche la demande d'autorisation d'Android (la réponse se lit ensuite avec `a_permission`).
pub fn demander(perms: &[&str]) {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        let _ = jni_::avec(|env, act| {
            let t = jni_::tableau(env, perms)?;
            env.call_method(act, "requestPermissions", "([Ljava/lang/String;I)V", &[JValue::Object(&t), JValue::Int(7)])?;
            Ok(())
        });
    }
    let _ = perms;
}

pub fn version_android() -> i32 {
    #[cfg(target_os = "android")]
    {
        return jni_::avec(|env, _| Ok(jni_::sdk(env))).unwrap_or(26);
    }
    #[allow(unreachable_code)]
    34
}

pub fn modele() -> String {
    #[cfg(target_os = "android")]
    {
        return jni_::avec(|env, _| {
            let m = env.get_static_field("android/os/Build", "MODEL", "Ljava/lang/String;")?.l()?;
            jni_::chaine(env, &m)
        })
        .unwrap_or_else(|| "Android".into());
    }
    #[allow(unreachable_code)]
    "Téléphone".into()
}

// ---------------------------------------------------------------------------------------------------
// Fichiers du téléphone
// ---------------------------------------------------------------------------------------------------

/// L'appli peut-elle lire et modifier tous les fichiers (Téléchargements, photos…) ?
pub fn acces_fichiers() -> bool {
    #[cfg(target_os = "android")]
    {
        return jni_::avec(|env, _| {
            if jni_::sdk(env) >= 30 {
                env.call_static_method("android/os/Environment", "isExternalStorageManager", "()Z", &[])?.z()
            } else {
                Ok(false)
            }
        })
        .unwrap_or(false)
            || (version_android() < 30 && a_permission("android.permission.WRITE_EXTERNAL_STORAGE"));
    }
    #[allow(unreachable_code)]
    std::path::Path::new(STOCKAGE).exists()
}

/// Ouvre l'écran d'Android « Accès à tous les fichiers » pour Outils.
pub fn demander_acces_fichiers() {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        if version_android() < 30 {
            demander(&["android.permission.READ_EXTERNAL_STORAGE", "android.permission.WRITE_EXTERNAL_STORAGE"]);
            return;
        }
        let _ = jni_::avec(|env, act| {
            let p = jni_::paquet(env, act)?;
            let i = jni_::intention(env, "android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION", Some(&format!("package:{p}")))?;
            env.call_method(act, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&i)])?;
            Ok(())
        });
    }
}

/// Prévient la galerie qu'un fichier a été créé, déplacé ou supprimé.
pub fn signaler_media(chemins: &[String]) {
    #[cfg(target_os = "android")]
    {
        use jni::objects::{JObject, JValue};
        for lot in chemins.chunks(200) {
            let v: Vec<&str> = lot.iter().map(|s| s.as_str()).collect();
            let _ = jni_::avec(|env, act| {
                let t = jni_::tableau(env, &v)?;
                env.call_static_method("android/media/MediaScannerConnection", "scanFile", "(Landroid/content/Context;[Ljava/lang/String;[Ljava/lang/String;Landroid/media/MediaScannerConnection$OnScanCompletedListener;)V", &[JValue::Object(act), JValue::Object(&t), JValue::Object(&JObject::null()), JValue::Object(&JObject::null())])?;
                Ok(())
            });
        }
    }
    let _ = chemins;
}

// ---------------------------------------------------------------------------------------------------
// Wi-Fi et applis
// ---------------------------------------------------------------------------------------------------

pub fn wifi() -> Option<InfosWifi> {
    #[cfg(target_os = "android")]
    {
        return jni_::avec(|env, act| {
            let wm = jni_::service(env, act, "wifi")?;
            let info = env.call_method(&wm, "getConnectionInfo", "()Landroid/net/wifi/WifiInfo;", &[])?.l()?;
            let s = env.call_method(&info, "getSSID", "()Ljava/lang/String;", &[])?.l()?;
            let ssid = jni_::chaine(env, &s)?.trim_matches('"').to_string();
            let rssi = env.call_method(&info, "getRssi", "()I", &[])?.i()?;
            let vitesse = env.call_method(&info, "getLinkSpeed", "()I", &[])?.i()?;
            let frequence = env.call_method(&info, "getFrequency", "()I", &[])?.i()?;
            let ip = env.call_method(&info, "getIpAddress", "()I", &[])?.i()?;
            let d = env.call_method(&wm, "getDhcpInfo", "()Landroid/net/DhcpInfo;", &[])?.l()?;
            let g = env.get_field(&d, "gateway", "I")?.i()?;
            let m = env.get_field(&d, "netmask", "I")?.i()?;
            let n = env.get_field(&d, "dns1", "I")?.i()?;
            Ok(InfosWifi { ssid, rssi, vitesse, frequence, ip: jni_::ip(ip), passerelle: jni_::ip(g), masque: jni_::ip(m), dns: jni_::ip(n) })
        });
    }
    #[allow(unreachable_code)]
    Some(InfosWifi { ssid: "Box-Maison".into(), rssi: -58, vitesse: 433, frequence: 5180, ip: "192.168.1.42".into(), passerelle: "192.168.1.1".into(), masque: "255.255.255.0".into(), dns: "192.168.1.1".into() })
}

/// Applis installées, avec leurs permissions et d'où elles viennent.
pub fn applis() -> Vec<Appli> {
    #[cfg(target_os = "android")]
    {
        use jni::objects::{JObjectArray, JString, JValue};
        return jni_::avec(|env, act| {
            let pm = env.call_method(act, "getPackageManager", "()Landroid/content/pm/PackageManager;", &[])?.l()?;
            let liste = env.call_method(&pm, "getInstalledPackages", "(I)Ljava/util/List;", &[JValue::Int(0x1000)])?.l()?;
            let n = env.call_method(&liste, "size", "()I", &[])?.i()?;
            let mut v = vec![];
            for k in 0..n {
                let a = env.with_local_frame(32, |env| -> jni::errors::Result<Appli> {
                    let pi = env.call_method(&liste, "get", "(I)Ljava/lang/Object;", &[JValue::Int(k)])?.l()?;
                    let p = env.get_field(&pi, "packageName", "Ljava/lang/String;")?.l()?;
                    let paquet = jni_::chaine(env, &p)?;
                    let installee = (env.get_field(&pi, "firstInstallTime", "J")?.j()? / 1000) as u64;
                    let ai = env.get_field(&pi, "applicationInfo", "Landroid/content/pm/ApplicationInfo;")?.l()?;
                    let drap = env.get_field(&ai, "flags", "I")?.i()?;
                    let l = env.call_method(&pm, "getApplicationLabel", "(Landroid/content/pm/ApplicationInfo;)Ljava/lang/CharSequence;", &[JValue::Object(&ai)])?.l()?;
                    let nom = jni_::chaine(env, &l)?;
                    let ins = env.call_method(&pm, "getInstallerPackageName", "(Ljava/lang/String;)Ljava/lang/String;", &[JValue::Object(&p)]);
                    let installateur = match ins {
                        Ok(x) => {
                            let o = x.l()?;
                            if o.is_null() { None } else { Some(jni_::chaine(env, &o)?) }
                        }
                        Err(_) => {
                            let _ = env.exception_clear();
                            None
                        }
                    };
                    let rp = env.get_field(&pi, "requestedPermissions", "[Ljava/lang/String;")?.l()?;
                    let mut permissions = vec![];
                    if !rp.is_null() {
                        let t = JObjectArray::from(rp);
                        let m = env.get_array_length(&t)?;
                        for j in 0..m {
                            let e = env.get_object_array_element(&t, j)?;
                            let s: String = env.get_string(&JString::from(e))?.into();
                            permissions.push(s);
                        }
                    }
                    Ok(Appli { paquet, nom, systeme: drap & 1 != 0, installateur, permissions, installee })
                })?;
                v.push(a);
            }
            Ok(v)
        })
        .unwrap_or_default();
    }
    #[allow(unreachable_code)]
    vec![
        Appli { paquet: "com.whatsapp".into(), nom: "WhatsApp".into(), systeme: false, installateur: Some("com.android.vending".into()), permissions: vec!["android.permission.READ_CONTACTS".into()], installee: 1_700_000_000 },
        Appli { paquet: "com.free.vpn.cracked".into(), nom: "Free VPN Pro".into(), systeme: false, installateur: None, permissions: vec!["android.permission.READ_SMS".into(), "android.permission.BIND_ACCESSIBILITY_SERVICE".into(), "android.permission.REQUEST_INSTALL_PACKAGES".into()], installee: 1_760_000_000 },
    ]
}

/// Services d'accessibilité actifs (peuvent lire l'écran) : « paquet/service:… ».
pub fn accessibilite_active() -> String {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        return jni_::avec(|env, act| {
            let cr = env.call_method(act, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
            let k = env.new_string("enabled_accessibility_services")?;
            let s = env.call_static_method("android/provider/Settings$Secure", "getString", "(Landroid/content/ContentResolver;Ljava/lang/String;)Ljava/lang/String;", &[JValue::Object(&cr), JValue::Object(&k)])?.l()?;
            jni_::chaine(env, &s)
        })
        .unwrap_or_default();
    }
    #[allow(unreachable_code)]
    "com.free.vpn.cracked/.Service".into()
}

/// Ouvre l'écran de désinstallation d'une appli.
pub fn desinstaller(paquet: &str) {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        let _ = jni_::avec(|env, act| {
            let i = jni_::intention(env, "android.intent.action.DELETE", Some(&format!("package:{paquet}")))?;
            env.call_method(act, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&i)])?;
            Ok(())
        });
    }
    let _ = paquet;
}

/// Ouvre la fiche de l'appli dans les réglages d'Android.
pub fn fiche_appli(paquet: &str) {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        let _ = jni_::avec(|env, act| {
            let i = jni_::intention(env, "android.settings.APPLICATION_DETAILS_SETTINGS", Some(&format!("package:{paquet}")))?;
            env.call_method(act, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&i)])?;
            Ok(())
        });
    }
    let _ = paquet;
}

// ---------------------------------------------------------------------------------------------------
// Lampe torche
// ---------------------------------------------------------------------------------------------------

pub fn lampe(allumer: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use jni::objects::{JObjectArray, JValue};
        return jni_::avec(|env, act| {
            let cm = jni_::service(env, act, "camera")?;
            let ids = JObjectArray::from(env.call_method(&cm, "getCameraIdList", "()[Ljava/lang/String;", &[])?.l()?);
            let n = env.get_array_length(&ids)?;
            for k in 0..n {
                let id = env.get_object_array_element(&ids, k)?;
                if env.call_method(&cm, "setTorchMode", "(Ljava/lang/String;Z)V", &[JValue::Object(&id), JValue::Bool(allumer as u8)]).is_ok() {
                    return Ok(true);
                }
                let _ = env.exception_clear();
            }
            Ok(false)
        })
        .filter(|ok| *ok)
        .map(|_| ())
        .ok_or_else(|| "pas de flash sur ce téléphone (ou appareil photo déjà utilisé)".into());
    }
    #[allow(unreachable_code)]
    {
        let _ = allumer;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------
// Capteurs (accéléromètre, boussole, podomètre)
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct Mesures {
    pub acc: [f32; 3],
    pub mag: [f32; 3],
    pub pas: Option<f32>,
    pub recu: bool,
}

pub struct Capteurs {
    pub m: std::sync::Arc<std::sync::Mutex<Mesures>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Capteurs {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Capteurs {
    /// Démarre la lecture des capteurs demandés (arrêtée quand l'objet disparaît).
    pub fn demarrer(acc: bool, mag: bool, pas: bool) -> Capteurs {
        let c = Capteurs { m: Default::default(), stop: Default::default() };
        #[cfg(target_os = "android")]
        {
            let (m, stop) = (c.m.clone(), c.stop.clone());
            std::thread::spawn(move || unsafe {
                use ndk_sys::*;
                let mgr = ASensorManager_getInstanceForPackage(c"fr.poiscail.outils".as_ptr());
                if mgr.is_null() {
                    return;
                }
                let looper = ALooper_prepare(ALOOPER_PREPARE_ALLOW_NON_CALLBACKS as i32);
                let q = ASensorManager_createEventQueue(mgr, looper, 3, None, std::ptr::null_mut());
                let mut actifs = vec![];
                for (voulu, t, us) in [(acc, ASENSOR_TYPE_ACCELEROMETER, 20_000), (mag, ASENSOR_TYPE_MAGNETIC_FIELD, 40_000), (pas, ASENSOR_TYPE_STEP_COUNTER, 200_000)] {
                    if !voulu {
                        continue;
                    }
                    let s = ASensorManager_getDefaultSensor(mgr, t as i32);
                    if !s.is_null() {
                        ASensorEventQueue_enableSensor(q, s);
                        ASensorEventQueue_setEventRate(q, s, us);
                        actifs.push(s);
                    }
                }
                let mut ev: [ASensorEvent; 16] = std::mem::zeroed();
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    ALooper_pollOnce(200, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                    loop {
                        let n = ASensorEventQueue_getEvents(q, ev.as_mut_ptr(), 16);
                        if n <= 0 {
                            break;
                        }
                        let mut x = m.lock().unwrap();
                        for e in &ev[..n as usize] {
                            let d = e.__bindgen_anon_1.__bindgen_anon_1.data;
                            match e.type_ as u32 {
                                t if t == ASENSOR_TYPE_ACCELEROMETER as u32 => x.acc = [d[0], d[1], d[2]],
                                t if t == ASENSOR_TYPE_MAGNETIC_FIELD as u32 => x.mag = [d[0], d[1], d[2]],
                                t if t == ASENSOR_TYPE_STEP_COUNTER as u32 => x.pas = Some(d[0]),
                                _ => {}
                            }
                            x.recu = true;
                        }
                    }
                }
                for s in actifs {
                    ASensorEventQueue_disableSensor(q, s);
                }
                ASensorManager_destroyEventQueue(mgr, q);
            });
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (acc, mag, pas);
            let mut x = c.m.lock().unwrap();
            *x = Mesures { acc: [0.8, 1.2, 9.6], mag: [12.0, -30.0, -40.0], pas: Some(4321.0), recu: true };
        }
        c
    }
}

/// Cap magnétique en degrés (0 = nord) à partir de la gravité et du champ magnétique.
pub fn cap(acc: [f32; 3], mag: [f32; 3]) -> Option<f32> {
    let (a, e) = (acc, mag);
    // H = E × A ; M = A × H (méthode de getRotationMatrix d'Android)
    let h = [e[1] * a[2] - e[2] * a[1], e[2] * a[0] - e[0] * a[2], e[0] * a[1] - e[1] * a[0]];
    let nh = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt();
    let na = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if nh < 0.1 || na < 0.1 {
        return None;
    }
    let h = [h[0] / nh, h[1] / nh, h[2] / nh];
    let a = [a[0] / na, a[1] / na, a[2] / na];
    let m = [a[1] * h[2] - a[2] * h[1], a[2] * h[0] - a[0] * h[2], a[0] * h[1] - a[1] * h[0]];
    let az = h[1].atan2(m[1]).to_degrees();
    Some((az + 360.0) % 360.0)
}

// ---------------------------------------------------------------------------------------------------
// Micro (niveau sonore)
// ---------------------------------------------------------------------------------------------------

pub struct Micro {
    pub niveau: std::sync::Arc<std::sync::Mutex<Vec<f32>>>, // dB des dernières mesures
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub erreur: std::sync::Arc<std::sync::Mutex<String>>,
}

impl Drop for Micro {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Niveau en dB (référence arbitraire : silence ≈ 20 dB, voix ≈ 60 dB, cri ≈ 90 dB).
pub fn decibels(echantillons: &[f32]) -> f32 {
    if echantillons.is_empty() {
        return 0.0;
    }
    let rms = (echantillons.iter().map(|x| x * x).sum::<f32>() / echantillons.len() as f32).sqrt();
    (20.0 * rms.max(1e-6).log10() + 100.0).clamp(0.0, 120.0)
}

impl Micro {
    pub fn demarrer() -> Micro {
        let m = Micro { niveau: Default::default(), stop: Default::default(), erreur: Default::default() };
        #[cfg(target_os = "android")]
        {
            let (n, stop, err) = (m.niveau.clone(), m.stop.clone(), m.erreur.clone());
            std::thread::spawn(move || {
                use ndk::audio::*;
                let flux = AudioStreamBuilder::new().map(|b| b.direction(AudioDirection::Input).channel_count(1).sample_rate(44_100).format(AudioFormat::PCM_Float)).and_then(|b| b.open_stream());
                let flux = match flux {
                    Ok(f) => f,
                    Err(e) => {
                        *err.lock().unwrap() = format!("micro indisponible : {e:?}");
                        return;
                    }
                };
                if let Err(e) = flux.request_start() {
                    *err.lock().unwrap() = format!("micro indisponible : {e:?}");
                    return;
                }
                let mut tampon = vec![0f32; 4410];
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let lus = unsafe { flux.read(tampon.as_mut_ptr().cast(), tampon.len() as i32, 500_000_000) };
                    if let Ok(k) = lus {
                        if k > 0 {
                            let mut v = n.lock().unwrap();
                            v.push(decibels(&tampon[..k as usize]));
                            if v.len() > 300 {
                                v.remove(0);
                            }
                        }
                    }
                }
                let _ = flux.request_stop();
            });
        }
        #[cfg(not(target_os = "android"))]
        {
            *m.niveau.lock().unwrap() = (0..120).map(|k| 45.0 + 15.0 * ((k as f32) / 9.0).sin()).collect();
        }
        m
    }
}

// ---------------------------------------------------------------------------------------------------
// Appareil photo (pour lire les QR codes) : images en niveaux de gris
// ---------------------------------------------------------------------------------------------------

pub struct Camera {
    /// dernière image : (largeur, hauteur, luminance)
    pub image: std::sync::Arc<std::sync::Mutex<Option<(usize, usize, Vec<u8>)>>>,
    pub erreur: std::sync::Arc<std::sync::Mutex<String>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(target_os = "android")]
#[link(name = "camera2ndk")]
extern "C" {}
#[cfg(target_os = "android")]
#[link(name = "mediandk")]
extern "C" {}

#[cfg(target_os = "android")]
mod cam {
    use ndk_sys::*;
    use std::os::raw::c_void;
    pub unsafe extern "C" fn rien_dev(_: *mut c_void, _: *mut ACameraDevice) {}
    pub unsafe extern "C" fn rien_err(_: *mut c_void, _: *mut ACameraDevice, _: i32) {}
    pub unsafe extern "C" fn rien_ses(_: *mut c_void, _: *mut ACameraCaptureSession) {}
}

impl Camera {
    pub fn demarrer() -> Camera {
        let c = Camera { image: Default::default(), erreur: Default::default(), stop: Default::default() };
        #[cfg(target_os = "android")]
        {
            let (img, err, stop) = (c.image.clone(), c.erreur.clone(), c.stop.clone());
            std::thread::spawn(move || unsafe {
                use ndk_sys::*;
                let echec = |m: &str| *err.lock().unwrap() = m.to_string();
                let mgr = ACameraManager_create();
                let mut ids: *mut ACameraIdList = std::ptr::null_mut();
                if ACameraManager_getCameraIdList(mgr, &mut ids).0 != 0 || ids.is_null() || (*ids).numCameras < 1 {
                    echec("aucun appareil photo");
                    ACameraManager_delete(mgr);
                    return;
                }
                let id = *(*ids).cameraIds; // la première : l'appareil photo arrière
                let mut rappels = ACameraDevice_StateCallbacks { context: std::ptr::null_mut(), onDisconnected: Some(cam::rien_dev), onError: Some(cam::rien_err) };
                let mut dev: *mut ACameraDevice = std::ptr::null_mut();
                if ACameraManager_openCamera(mgr, id, &mut rappels, &mut dev).0 != 0 {
                    echec("appareil photo refusé (autorise l'appareil photo pour Outils)");
                    ACameraManager_deleteCameraIdList(ids);
                    ACameraManager_delete(mgr);
                    return;
                }
                let (l, h) = (640, 480);
                let mut lecteur: *mut AImageReader = std::ptr::null_mut();
                AImageReader_new(l, h, AIMAGE_FORMATS::AIMAGE_FORMAT_YUV_420_888.0 as i32, 2, &mut lecteur);
                let mut fenetre: *mut ANativeWindow = std::ptr::null_mut();
                AImageReader_getWindow(lecteur, &mut fenetre);
                let mut conteneur: *mut ACaptureSessionOutputContainer = std::ptr::null_mut();
                ACaptureSessionOutputContainer_create(&mut conteneur);
                let mut sortie: *mut ACaptureSessionOutput = std::ptr::null_mut();
                ACaptureSessionOutput_create(fenetre, &mut sortie);
                ACaptureSessionOutputContainer_add(conteneur, sortie);
                let mut req: *mut ACaptureRequest = std::ptr::null_mut();
                ACameraDevice_createCaptureRequest(dev, ACameraDevice_request_template::TEMPLATE_PREVIEW, &mut req);
                let mut cible: *mut ACameraOutputTarget = std::ptr::null_mut();
                ACameraOutputTarget_create(fenetre, &mut cible);
                ACaptureRequest_addTarget(req, cible);
                let mut rs = ACameraCaptureSession_stateCallbacks { context: std::ptr::null_mut(), onClosed: Some(cam::rien_ses), onReady: Some(cam::rien_ses), onActive: Some(cam::rien_ses) };
                let mut session: *mut ACameraCaptureSession = std::ptr::null_mut();
                if ACameraDevice_createCaptureSession(dev, conteneur, &mut rs, &mut session).0 != 0 {
                    echec("impossible de démarrer l'appareil photo");
                } else {
                    ACameraCaptureSession_setRepeatingRequest(session, std::ptr::null_mut(), 1, &mut req, std::ptr::null_mut());
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(120));
                        let mut im: *mut AImage = std::ptr::null_mut();
                        if AImageReader_acquireLatestImage(lecteur, &mut im).0 != 0 || im.is_null() {
                            continue;
                        }
                        let (mut w, mut hh, mut pas, mut lon) = (0, 0, 0, 0);
                        let mut donnees: *mut u8 = std::ptr::null_mut();
                        AImage_getWidth(im, &mut w);
                        AImage_getHeight(im, &mut hh);
                        AImage_getPlaneRowStride(im, 0, &mut pas);
                        AImage_getPlaneData(im, 0, &mut donnees, &mut lon);
                        if !donnees.is_null() && w > 0 && hh > 0 {
                            let src = std::slice::from_raw_parts(donnees, lon as usize);
                            let (w, hh, pas) = (w as usize, hh as usize, pas as usize);
                            let mut y = vec![0u8; w * hh];
                            for r in 0..hh {
                                if r * pas + w <= src.len() {
                                    y[r * w..r * w + w].copy_from_slice(&src[r * pas..r * pas + w]);
                                }
                            }
                            *img.lock().unwrap() = Some((w, hh, y));
                        }
                        AImage_delete(im);
                    }
                    ACameraCaptureSession_stopRepeating(session);
                    ACameraCaptureSession_close(session);
                }
                ACaptureRequest_free(req);
                ACameraOutputTarget_free(cible);
                ACaptureSessionOutputContainer_free(conteneur);
                ACaptureSessionOutput_free(sortie);
                ACameraDevice_close(dev);
                AImageReader_delete(lecteur);
                ACameraManager_deleteCameraIdList(ids);
                ACameraManager_delete(mgr);
            });
        }
        #[cfg(not(target_os = "android"))]
        {
            *c.erreur.lock().unwrap() = "Pas d'appareil photo sur ce PC.".into();
        }
        c
    }
}

/// Lit un QR code dans une image en niveaux de gris.
pub fn lire_qr(l: usize, h: usize, y: &[u8]) -> Option<String> {
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(l, h, |x, yy| y[yy * l + x]);
    img.detect_grids().into_iter().find_map(|g| g.decode().ok().map(|(_, t)| t))
}

// ---------------------------------------------------------------------------------------------------
// Réseaux Wi-Fi alentour, réseau mobile, consommation de données, réglages d'Android
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReseauVoisin {
    pub ssid: String,
    pub bssid: String,
    pub rssi: i32,
    pub frequence: i32,
    pub largeur: i32, // MHz
}

/// Réseaux Wi-Fi visibles (demande la localisation, activée dans Android).
pub fn scan_wifi() -> Vec<ReseauVoisin> {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        return jni_::avec(|env, act| {
            let wm = jni_::service(env, act, "wifi")?;
            let _ = env.call_method(&wm, "startScan", "()Z", &[]);
            let _ = env.exception_clear();
            let l = env.call_method(&wm, "getScanResults", "()Ljava/util/List;", &[])?.l()?;
            let n = env.call_method(&l, "size", "()I", &[])?.i()?;
            let mut v = vec![];
            for k in 0..n {
                let r = env.with_local_frame(16, |env| -> jni::errors::Result<ReseauVoisin> {
                    let s = env.call_method(&l, "get", "(I)Ljava/lang/Object;", &[JValue::Int(k)])?.l()?;
                    let ssid = env.get_field(&s, "SSID", "Ljava/lang/String;")?.l()?;
                    let bssid = env.get_field(&s, "BSSID", "Ljava/lang/String;")?.l()?;
                    let largeur = match env.get_field(&s, "channelWidth", "I")?.i()? {
                        1 => 40,
                        2 => 80,
                        3 | 4 => 160,
                        _ => 20,
                    };
                    Ok(ReseauVoisin {
                        ssid: jni_::chaine(env, &ssid)?,
                        bssid: jni_::chaine(env, &bssid)?,
                        rssi: env.get_field(&s, "level", "I")?.i()?,
                        frequence: env.get_field(&s, "frequency", "I")?.i()?,
                        largeur,
                    })
                })?;
                v.push(r);
            }
            Ok(v)
        })
        .unwrap_or_default();
    }
    #[allow(unreachable_code)]
    vec![
        ReseauVoisin { ssid: "Box-Maison".into(), bssid: "aa".into(), rssi: -48, frequence: 2437, largeur: 20 },
        ReseauVoisin { ssid: "Voisin1".into(), bssid: "bb".into(), rssi: -60, frequence: 2437, largeur: 20 },
        ReseauVoisin { ssid: "Voisin2".into(), bssid: "cc".into(), rssi: -70, frequence: 2412, largeur: 40 },
        ReseauVoisin { ssid: "Box-Maison".into(), bssid: "dd".into(), rssi: -55, frequence: 5180, largeur: 80 },
        ReseauVoisin { ssid: "Voisin3".into(), bssid: "ee".into(), rssi: -75, frequence: 5500, largeur: 80 },
    ]
}

#[derive(Clone, Debug, Default)]
pub struct SignalMobile {
    pub operateur: String,
    pub generation: String, // « 5G », « 4G »…
    pub dbm: i32,
    pub niveau: i32, // 0 à 4
}

/// Signal du réseau mobile (aucune autorisation nécessaire).
pub fn signal_mobile() -> Option<SignalMobile> {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        return jni_::avec(|env, act| {
            let tm = jni_::service(env, act, "phone")?;
            let o = env.call_method(&tm, "getNetworkOperatorName", "()Ljava/lang/String;", &[])?.l()?;
            let operateur = jni_::chaine(env, &o)?;
            let ss = env.call_method(&tm, "getSignalStrength", "()Landroid/telephony/SignalStrength;", &[])?.l()?;
            if ss.is_null() {
                return Ok(SignalMobile { operateur, generation: "aucun réseau".into(), dbm: 0, niveau: 0 });
            }
            let l = env.call_method(&ss, "getCellSignalStrengths", "()Ljava/util/List;", &[])?.l()?;
            let n = env.call_method(&l, "size", "()I", &[])?.i()?;
            let mut meilleur: Option<SignalMobile> = None;
            for k in 0..n {
                let c = env.call_method(&l, "get", "(I)Ljava/lang/Object;", &[JValue::Int(k)])?.l()?;
                let classe = env.call_method(&c, "getClass", "()Ljava/lang/Class;", &[])?.l()?;
                let nom = env.call_method(&classe, "getSimpleName", "()Ljava/lang/String;", &[])?.l()?;
                let nom = jni_::chaine(env, &nom)?;
                let generation = match nom.as_str() {
                    "CellSignalStrengthNr" => "5G",
                    "CellSignalStrengthLte" => "4G",
                    "CellSignalStrengthWcdma" | "CellSignalStrengthTdscdma" | "CellSignalStrengthCdma" => "3G",
                    "CellSignalStrengthGsm" => "2G",
                    _ => "?",
                };
                let dbm = env.call_method(&c, "getDbm", "()I", &[])?.i()?;
                let niveau = env.call_method(&c, "getLevel", "()I", &[])?.i()?;
                // la 5G « non autonome » s'ajoute à la 4G : on garde la génération la plus récente
                if meilleur.as_ref().is_none_or(|m| generation < m.generation.as_str() || m.generation == "?") {
                    meilleur = Some(SignalMobile { operateur: operateur.clone(), generation: generation.into(), dbm, niveau });
                }
            }
            Ok(meilleur.unwrap_or(SignalMobile { operateur, generation: "aucun réseau".into(), dbm: 0, niveau: 0 }))
        });
    }
    #[allow(unreachable_code)]
    Some(SignalMobile { operateur: "Opérateur".into(), generation: "4G".into(), dbm: -94, niveau: 2 })
}

/// Réseau utilisé en ce moment : « Wi-Fi », « mobile » ou « aucun ».
pub fn reseau_actif() -> &'static str {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        return jni_::avec(|env, act| {
            let cm = jni_::service(env, act, "connectivity")?;
            let n = env.call_method(&cm, "getActiveNetwork", "()Landroid/net/Network;", &[])?.l()?;
            if n.is_null() {
                return Ok("aucun");
            }
            let c = env.call_method(&cm, "getNetworkCapabilities", "(Landroid/net/Network;)Landroid/net/NetworkCapabilities;", &[JValue::Object(&n)])?.l()?;
            if c.is_null() {
                return Ok("aucun");
            }
            let a = |env: &mut jni::JNIEnv, t: i32| env.call_method(&c, "hasTransport", "(I)Z", &[JValue::Int(t)]).and_then(|v| v.z());
            Ok(if a(env, 1)? { "Wi-Fi" } else if a(env, 0)? { "mobile" } else { "autre" })
        })
        .unwrap_or("aucun");
    }
    #[allow(unreachable_code)]
    "Wi-Fi"
}

/// L'appli a-t-elle l'accès aux statistiques d'utilisation (pour la consommation de données) ?
pub fn acces_statistiques() -> bool {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        return jni_::avec(|env, act| {
            let ao = jni_::service(env, act, "appops")?;
            let uid = env.call_static_method("android/os/Process", "myUid", "()I", &[])?.i()?;
            let p = env.call_method(act, "getPackageName", "()Ljava/lang/String;", &[])?.l()?;
            let op = env.new_string("android:get_usage_stats")?;
            Ok(env.call_method(&ao, "checkOpNoThrow", "(Ljava/lang/String;ILjava/lang/String;)I", &[JValue::Object(&op), JValue::Int(uid), JValue::Object(&p)])?.i()? == 0)
        })
        .unwrap_or(false);
    }
    #[allow(unreachable_code)]
    true
}

/// Données consommées par appli entre deux dates (secondes Unix) : (nom, reçu, envoyé), triées.
/// `mobile` : données mobiles, sinon Wi-Fi.
pub fn conso_par_appli(mobile: bool, debut: u64, fin: u64) -> Vec<(String, u64, u64)> {
    #[cfg(target_os = "android")]
    {
        use jni::objects::{JObject, JValue};
        return jni_::avec(|env, act| {
            let nsm = jni_::service(env, act, "netstats")?;
            let st = env.call_method(&nsm, "querySummary", "(ILjava/lang/String;JJ)Landroid/app/usage/NetworkStats;", &[JValue::Int(if mobile { 0 } else { 1 }), JValue::Object(&JObject::null()), JValue::Long(debut as i64 * 1000), JValue::Long(fin as i64 * 1000)])?.l()?;
            let b = env.new_object("android/app/usage/NetworkStats$Bucket", "()V", &[])?;
            let mut par_uid: std::collections::HashMap<i32, (u64, u64)> = Default::default();
            while env.call_method(&st, "hasNextBucket", "()Z", &[])?.z()? {
                env.call_method(&st, "getNextBucket", "(Landroid/app/usage/NetworkStats$Bucket;)Z", &[JValue::Object(&b)])?;
                let uid = env.call_method(&b, "getUid", "()I", &[])?.i()?;
                let rx = env.call_method(&b, "getRxBytes", "()J", &[])?.j()?.max(0) as u64;
                let tx = env.call_method(&b, "getTxBytes", "()J", &[])?.j()?.max(0) as u64;
                let e = par_uid.entry(uid).or_default();
                e.0 += rx;
                e.1 += tx;
            }
            let _ = env.call_method(&st, "close", "()V", &[]);
            let pm = env.call_method(act, "getPackageManager", "()Landroid/content/pm/PackageManager;", &[])?.l()?;
            let mut v = vec![];
            for (uid, (rx, tx)) in par_uid {
                let nom = env.with_local_frame(16, |env| -> jni::errors::Result<String> {
                    Ok(match uid {
                        -4 => "Applis désinstallées".into(),
                        -5 => "Partage de connexion".into(),
                        0 | 1000 => "Système Android".into(),
                        _ => {
                            let p = env.call_method(&pm, "getNameForUid", "(I)Ljava/lang/String;", &[JValue::Int(uid)])?.l()?;
                            let paquet = jni_::chaine(env, &p)?;
                            let paquet = paquet.split(':').next().unwrap_or("").to_string();
                            let jp = env.new_string(&paquet)?;
                            match env.call_method(&pm, "getApplicationInfo", "(Ljava/lang/String;I)Landroid/content/pm/ApplicationInfo;", &[JValue::Object(&jp), JValue::Int(0)]) {
                                Ok(ai) => {
                                    let ai = ai.l()?;
                                    let l = env.call_method(&pm, "getApplicationLabel", "(Landroid/content/pm/ApplicationInfo;)Ljava/lang/CharSequence;", &[JValue::Object(&ai)])?.l()?;
                                    jni_::chaine(env, &l)?
                                }
                                Err(_) => {
                                    let _ = env.exception_clear();
                                    if paquet.is_empty() { format!("uid {uid}") } else { paquet }
                                }
                            }
                        }
                    })
                })?;
                v.push((nom, rx, tx));
            }
            v.sort_by_key(|x| std::cmp::Reverse(x.1 + x.2));
            Ok(v)
        })
        .unwrap_or_default();
    }
    #[allow(unreachable_code)]
    {
        let _ = (mobile, debut, fin);
        vec![("YouTube".into(), 2_400_000_000, 30_000_000), ("TikTok".into(), 1_100_000_000, 80_000_000), ("Snapchat".into(), 300_000_000, 200_000_000), ("Système Android".into(), 90_000_000, 20_000_000)]
    }
}

/// Ouvre un écran des réglages d'Android (« android.settings.… »), éventuellement pour Outils.
pub fn ouvrir_reglage(action: &str, pour_outils: bool) {
    #[cfg(target_os = "android")]
    {
        use jni::objects::JValue;
        let _ = jni_::avec(|env, act| {
            let p = jni_::paquet(env, act)?;
            let i = jni_::intention(env, action, pour_outils.then(|| format!("package:{p}")).as_deref())?;
            env.call_method(act, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&i)])?;
            Ok(())
        });
    }
    let _ = (action, pour_outils);
}
