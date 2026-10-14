use crate::connection_store::ConnectionProfile;
use anyhow::{bail, Result};
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

pub type DbClient = Client<tokio_util::compat::Compat<TcpStream>>;

/// Monta a config de conexão a partir das env vars seguindo o padrão:
///   DB_<ENV>_<SYSTEM>_SVR
///   DB_<ENV>_<SYSTEM>_USR
///   DB_<ENV>_<SYSTEM>_PWD
///   DB_<ENV>_<SYSTEM>_DB   (opcional — nome do database)
pub fn build_config(env: &str, system: &str) -> Result<Config> {
    let e = env.to_uppercase();
    let s = system.to_uppercase();
    let prefix = format!("DB_{e}_{s}");

    let server = std::env::var(format!("{prefix}_SVR")).ok();
    let user = std::env::var(format!("{prefix}_USR")).ok();
    let password = std::env::var(format!("{prefix}_PWD")).ok();
    let database = std::env::var(format!("{prefix}_DB")).ok();

    let mut missing = Vec::new();
    if server.is_none() {
        missing.push(format!("{prefix}_SVR"));
    }
    if user.is_none() {
        missing.push(format!("{prefix}_USR"));
    }
    if password.is_none() {
        missing.push(format!("{prefix}_PWD"));
    }

    if !missing.is_empty() {
        bail!(
            "Variáveis de ambiente ausentes para env=\"{}\" system=\"{}\":\n  {}",
            env,
            system,
            missing.join("\n  ")
        );
    }

    let server = server.unwrap();
    let (host, port) = match server.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(1433)),
        None => (server, 1433),
    };

    let mut config = Config::new();
    config.host(&host);
    config.port(port);
    config.authentication(AuthMethod::sql_server(user.unwrap(), password.unwrap()));
    if let Some(db) = database {
        config.database(db);
    }
    config.encryption(EncryptionLevel::Required);
    config.trust_cert();

    Ok(config)
}

pub fn build_config_from_profile(profile: &ConnectionProfile) -> Result<Config> {
    if profile.adapter != "sqlserver" {
        bail!("Adapter não suportado: {}", profile.adapter);
    }
    let mut config = Config::new();
    config.host(&profile.host);
    config.port(profile.port);
    config.authentication(AuthMethod::sql_server(
        profile.username.clone(),
        profile.password.clone(),
    ));
    if let Some(database) = &profile.database {
        config.database(database);
    }
    if profile.encrypt {
        config.encryption(EncryptionLevel::Required);
    }
    if profile.trust_certificate {
        config.trust_cert();
    }
    Ok(config)
}

pub async fn connect_profile(profile: &ConnectionProfile) -> Result<DbClient> {
    let config = build_config_from_profile(profile)?;
    let addr = config.get_addr();
    let tcp = TcpStream::connect(addr).await?;
    tcp.set_nodelay(true)?;
    Ok(Client::connect(config, tcp.compat_write()).await?)
}

pub async fn connect(env: &str, system: &str) -> Result<DbClient> {
    let config = build_config(env, system)?;

    let addr = config.get_addr();
    let tcp = TcpStream::connect(addr).await?;
    tcp.set_nodelay(true)?;

    let client = Client::connect(config, tcp.compat_write()).await?;
    Ok(client)
}
