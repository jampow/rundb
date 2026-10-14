use anyhow::{anyhow, bail, Context, Result};
use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use dialoguer::{Input, Password};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::cli::ConnectionAddArgs;

const FILE_NAME: &str = ".rundb-connections.enc";
const KEYRING_SERVICE: &str = "rundb-connections";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub name: String,
    pub adapter: String,
    pub environment: String,
    pub system: String,
    pub host: String,
    pub port: u16,
    pub database: Option<String>,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub encrypt: bool,
    #[serde(default)]
    pub trust_certificate: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Database {
    version: u8,
    connections: Vec<ConnectionProfile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    version: u8,
    method: String,
    salt: String,
    nonce: String,
    ciphertext: String,
}

pub fn store_path(scripts_dir: &Path) -> PathBuf {
    scripts_dir.join(FILE_NAME)
}

pub fn load(scripts_dir: &Path) -> Result<Vec<ConnectionProfile>> {
    let path = store_path(scripts_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let envelope: Envelope = serde_json::from_slice(&std::fs::read(&path)?)
        .with_context(|| format!("Arquivo de conexões inválido: {}", path.display()))?;
    let key = key_for_envelope(&envelope, &path)?;
    let bytes = decrypt(&envelope, &key)?;
    let database: Database = serde_json::from_slice(&bytes)
        .context("Conteúdo do arquivo de conexões inválido ou senha-mestra incorreta")?;
    Ok(database.connections)
}

pub fn save(scripts_dir: &Path, connections: &[ConnectionProfile]) -> Result<()> {
    std::fs::create_dir_all(scripts_dir)?;
    let path = store_path(scripts_dir);
    let database = Database {
        version: 1,
        connections: connections.to_vec(),
    };
    let plaintext = serde_json::to_vec(&database)?;

    let existing_method = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Envelope>(&b).ok())
        .map(|e| e.method);
    let salt = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Envelope>(&b).ok())
        .and_then(|e| hex::decode(e.salt).ok())
        .filter(|s| s.len() == 16)
        .unwrap_or_else(|| {
            let mut s = vec![0u8; 16];
            OsRng.fill_bytes(&mut s);
            s
        });
    let salt_array: [u8; 16] = salt.as_slice().try_into().unwrap();
    let (method, key) = match existing_method.as_deref() {
        Some("master-password") => (
            "master-password".to_string(),
            password_key(false, &salt_array)?,
        ),
        _ => match keyring_key(&path) {
            Ok(key) => ("keyring".to_string(), key),
            Err(_) => {
                eprintln!("Keyring indisponível; usando senha-mestra para este arquivo.");
                (
                    "master-password".to_string(),
                    password_key(true, &salt_array)?,
                )
            }
        },
    };
    let envelope = encrypt(&plaintext, &key, method, &salt)?;
    let bytes = serde_json::to_vec_pretty(&envelope)?;
    let temp = path.with_extension("enc.tmp");
    std::fs::write(&temp, bytes)?;
    set_private_permissions(&temp)?;
    std::fs::rename(&temp, &path)?;
    set_private_permissions(&path)?;
    Ok(())
}

pub fn prompt_profile(args: &ConnectionAddArgs) -> Result<ConnectionProfile> {
    let adapter = args.adapter.trim().to_lowercase();
    if adapter.to_lowercase() != "sqlserver" {
        bail!("Adapter ainda não suportado: {adapter}. O único adapter disponível é sqlserver.");
    }
    let name = value_or_prompt(args.name.clone(), "Nome da conexão")?;
    let environment = value_or_prompt(args.env.clone(), "Ambiente")?;
    let system = value_or_prompt(args.system.clone(), "Sistema/database usado pelo script")?;
    let host = value_or_prompt(args.host.clone(), "Host")?;
    let database = args.database.clone();
    let username = value_or_prompt(args.username.clone(), "Usuário")?;
    let password = Password::new().with_prompt("Senha").interact()?;
    let encrypt = !args.no_encrypt;
    let trust_certificate = !args.no_trust_certificate;
    Ok(ConnectionProfile {
        name,
        adapter: adapter.to_lowercase(),
        environment,
        system,
        host,
        port: args.port,
        database,
        username,
        password,
        encrypt,
        trust_certificate,
    })
}

fn value_or_prompt(value: Option<String>, prompt: &str) -> Result<String> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value),
        Some(_) => bail!("{prompt} não pode ser vazio"),
        None => required(prompt),
    }
}

fn required(prompt: &str) -> Result<String> {
    let value: String = Input::new().with_prompt(prompt).interact_text()?;
    if value.trim().is_empty() {
        bail!("{prompt} não pode ser vazio")
    }
    Ok(value)
}

fn keyring_user(path: &Path) -> String {
    format!(
        "{}:{}",
        std::env::var("USER").unwrap_or_default(),
        path.display()
    )
}

fn keyring_key(path: &Path) -> Result<[u8; 32]> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(path))?;
    match entry.get_secret() {
        Ok(secret) => secret
            .try_into()
            .map_err(|_| anyhow!("Chave do keyring tem tamanho inválido")),
        Err(_) => {
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            entry.set_secret(&key)?;
            Ok(key)
        }
    }
}

fn existing_keyring_key(path: &Path) -> Result<[u8; 32]> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(path))?;
    let secret = entry
        .get_secret()
        .context("Chave não encontrada no cofre de credenciais")?;
    secret
        .try_into()
        .map_err(|_| anyhow!("Chave do keyring tem tamanho inválido"))
}

fn key_for_envelope(envelope: &Envelope, path: &Path) -> Result<[u8; 32]> {
    match envelope.method.as_str() {
        "keyring" => {
            existing_keyring_key(path).with_context(|| {
                format!(
                    "O arquivo de conexões depende do cofre do sistema, mas ele não está disponível. No WSL, instale/inicie um Secret Service ou remova o arquivo {} e cadastre novamente para usar a senha-mestra",
                    path.display()
                )
            })
        }
        "master-password" => {
            let salt = hex::decode(&envelope.salt).context("Salt da senha-mestra inválido")?;
            let salt: [u8; 16] = salt
                .try_into()
                .map_err(|_| anyhow!("Salt da senha-mestra inválido"))?;
            password_key(false, &salt)
        }
        other => bail!("Método de proteção desconhecido: {other}"),
    }
}

fn password_key(confirm: bool, salt: &[u8; 16]) -> Result<[u8; 32]> {
    let mut prompt = Password::new().with_prompt("Senha-mestra");
    if confirm {
        prompt = prompt.with_confirmation("Confirme a senha-mestra", "As senhas não conferem");
    }
    let password = prompt.interact()?;
    if password.is_empty() {
        bail!("A senha-mestra não pode ser vazia")
    }
    derive_password(&password, salt)
}

fn derive_password(password: &str, salt: &[u8; 16]) -> Result<[u8; 32]> {
    let mut output = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut output)
        .map_err(|e| anyhow!("Falha ao derivar chave: {e}"))?;
    Ok(output)
}

fn encrypt(plaintext: &[u8], key: &[u8; 32], method: String, salt: &[u8]) -> Result<Envelope> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|_| anyhow!("Falha ao criptografar conexões"))?;
    Ok(Envelope {
        version: 1,
        method,
        salt: hex::encode(salt),
        nonce: hex::encode(nonce),
        ciphertext: hex::encode(ciphertext),
    })
}

fn decrypt(envelope: &Envelope, key: &[u8; 32]) -> Result<Vec<u8>> {
    let nonce = hex::decode(&envelope.nonce).context("Nonce inválido")?;
    let ciphertext =
        hex::decode(&envelope.ciphertext).context("Conteúdo criptografado inválido")?;
    ChaCha20Poly1305::new(Key::from_slice(key)).decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("Não foi possível descriptografar as conexões; senha-mestra incorreta ou arquivo adulterado"))
}

fn set_private_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
