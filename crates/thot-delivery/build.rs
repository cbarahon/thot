use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=THOT_GDRIVE_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=THOT_GDRIVE_CLIENT_SECRET");
    println!("cargo:rerun-if-env-changed=THOT_GDRIVE_CLIENT_FILE");

    let credential_file = if let Some(path) = env::var_os("THOT_GDRIVE_CLIENT_FILE") {
        Some(PathBuf::from(path))
    } else {
        let secrets =
            PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../secrets");
        let mut candidates = fs::read_dir(&secrets)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        candidates.sort();
        match candidates.len() {
            0 => None,
            1 => candidates.pop(),
            _ => panic!(
                "multiple OAuth JSON files found in {}; set THOT_GDRIVE_CLIENT_FILE",
                secrets.display()
            ),
        }
    };

    if let Some(path) = credential_file {
        println!("cargo:rerun-if-changed={}", path.display());
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read Google OAuth client file: {error}"));
        let client: serde_json::Value = serde_json::from_str(&contents)
            .unwrap_or_else(|error| panic!("invalid Google OAuth client JSON: {error}"));
        let installed = client
            .get("installed")
            .expect("OAuth JSON must contain an installed Desktop client");
        let file_client_id = installed
            .get("client_id")
            .and_then(|value| value.as_str())
            .expect("OAuth Desktop client JSON is missing client_id");
        let file_client_secret = installed
            .get("client_secret")
            .and_then(|value| value.as_str())
            .expect("OAuth Desktop client JSON is missing client_secret");
        let client_id =
            env::var("THOT_GDRIVE_CLIENT_ID").unwrap_or_else(|_| file_client_id.to_owned());
        let client_secret =
            env::var("THOT_GDRIVE_CLIENT_SECRET").unwrap_or_else(|_| file_client_secret.to_owned());
        println!("cargo:rustc-env=THOT_GDRIVE_BUNDLED_CLIENT_ID={client_id}");
        println!("cargo:rustc-env=THOT_GDRIVE_BUNDLED_CLIENT_SECRET={client_secret}");
    } else {
        for (source, target) in [
            ("THOT_GDRIVE_CLIENT_ID", "THOT_GDRIVE_BUNDLED_CLIENT_ID"),
            (
                "THOT_GDRIVE_CLIENT_SECRET",
                "THOT_GDRIVE_BUNDLED_CLIENT_SECRET",
            ),
        ] {
            if let Ok(value) = env::var(source) {
                println!("cargo:rustc-env={target}={value}");
            }
        }
    }
}
