use anyhow::{bail, Result};
use clap::Parser;
use rundb::cli::{Cli, Command, ConnectionAction};
use rundb::{connection, connection_store, db_script, help, params, picker, runner};
use serde_json::Value as Json;
use std::collections::HashMap;
use std::path::PathBuf;

fn scripts_dir(cli: &Cli) -> PathBuf {
    if let Some(dir) = &cli.scripts_dir {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("DB_SCRIPTS_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from("db-scripts")
}

fn build_repeat_command(
    file: &str,
    env: &str,
    params: &HashMap<String, Json>,
    json: bool,
    debug: bool,
) -> String {
    let mut parts = vec![
        "rundb".to_string(),
        format!("-f {file}"),
        format!("-e {env}"),
    ];

    for (key, value) in params {
        let serialized = match value {
            Json::String(s) => s.clone(),
            other => other.to_string(),
        };
        parts.push(format!("-p {key}={serialized}"));
    }

    if json {
        parts.push("--json".to_string());
    }
    if debug {
        parts.push("--debug".to_string());
    }

    parts.join(" ")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.help {
        help::print_help();
        return Ok(());
    }

    if let Err(e) = run(cli).await {
        eprintln!("\nErro: {e}\n");
        std::process::exit(1);
    }

    Ok(())
}

async fn run(cli: Cli) -> Result<()> {
    let dir = scripts_dir(&cli);

    if let Some(Command::Connection(args)) = &cli.command {
        return run_connection_command(&dir, &args.action).await;
    }

    let file = match &cli.file {
        Some(f) => f.clone(),
        None => {
            if !dir.exists() {
                bail!("Pasta de scripts não encontrada: {}", dir.display());
            }
            let scripts = db_script::list_scripts(&dir)?;

            if scripts.is_empty() {
                bail!("Nenhum script encontrado em {}.", dir.display());
            }

            match picker::pick_script(&scripts)? {
                Some(file) => file,
                None => return Ok(()),
            }
        }
    };

    let absolute = db_script::resolve_script_path(&file, &dir)?;
    let script = db_script::load(&absolute)?;

    let mode_label = match script.mode {
        db_script::Mode::Parallel => "parallel",
        db_script::Mode::Sequential => "sequential",
    };

    let mut databases = vec![script.database.clone()];
    for q in &script.queries {
        if let Some(db) = &q.database {
            if !databases.contains(db) {
                databases.push(db.clone());
            }
        }
    }

    println!("\nScript:    {}", absolute.display());
    println!("Databases: {}", databases.join(", "));
    println!("Ambiente:  {}", cli.env);
    println!("Modo:      {mode_label}");
    if let Some(desc) = &script.description {
        println!("\nDescrição:");
        for line in desc.lines() {
            println!("  {line}");
        }
    }

    let provided_params = params::parse_param_flags(&cli.param)?;
    let mut resolved_params = params::resolve_params(&script.params, &provided_params)?;

    if !resolved_params.is_empty() {
        println!(
            "Params:    {}",
            serde_json::to_string(&resolved_params).unwrap_or_default()
        );
    }

    let stored_connections = connection_store::load(&dir)?;
    let mut pools = HashMap::new();
    for db in &databases {
        let client = match stored_connections.iter().find(|c| {
            c.environment.eq_ignore_ascii_case(&cli.env) && c.system.eq_ignore_ascii_case(db)
        }) {
            Some(profile) => connection::connect_profile(profile).await?,
            None => connection::connect(&cli.env, db).await?,
        };
        pools.insert(db.clone(), client);
    }

    let results = match script.mode {
        db_script::Mode::Parallel => {
            runner::run_parallel(
                &mut pools,
                &script.database,
                &script,
                &script.queries,
                &resolved_params,
                cli.json,
            )
            .await?
        }
        db_script::Mode::Sequential => {
            runner::run_sequential(
                &mut pools,
                &script.database,
                &script,
                &script.queries,
                &mut resolved_params,
                cli.debug,
                cli.json,
            )
            .await?
        }
    };

    if !cli.debug {
        if let Some(last) = results.last() {
            runner::print_results(std::slice::from_ref(last), cli.json);
        }
    }

    println!("\nConcluído.\n");
    println!(
        "Para repetir:\n  {}\n",
        build_repeat_command(&file, &cli.env, &resolved_params, cli.json, cli.debug)
    );

    Ok(())
}

async fn run_connection_command(dir: &PathBuf, action: &ConnectionAction) -> Result<()> {
    match action {
        ConnectionAction::Add(args) => {
            let profile = connection_store::prompt_profile(args)?;
            let mut connections = connection_store::load(dir)?;
            if let Some(existing) = connections.iter_mut().find(|c| c.name == profile.name) {
                *existing = profile;
            } else {
                connections.push(profile);
            }
            connection_store::save(dir, &connections)?;
            println!(
                "Conexão salva em {}",
                connection_store::store_path(dir).display()
            );
        }
        ConnectionAction::List => {
            let connections = connection_store::load(dir)?;
            if connections.is_empty() {
                println!("Nenhuma conexão cadastrada.");
            }
            for c in connections {
                println!(
                    "{}  adapter={}  env={}  system={}  {}:{}  database={}",
                    c.name,
                    c.adapter,
                    c.environment,
                    c.system,
                    c.host,
                    c.port,
                    c.database.unwrap_or_default()
                );
            }
        }
        ConnectionAction::Remove { name } => {
            let mut connections = connection_store::load(dir)?;
            let before = connections.len();
            connections.retain(|c| c.name != *name);
            if before == connections.len() {
                bail!("Conexão não encontrada: {name}");
            }
            connection_store::save(dir, &connections)?;
            println!("Conexão removida: {name}");
        }
        ConnectionAction::Test { name } => {
            let connections = connection_store::load(dir)?;
            let profile = connections
                .iter()
                .find(|c| c.name == *name)
                .ok_or_else(|| anyhow::anyhow!("Conexão não encontrada: {name}"))?;
            connection::connect_profile(profile).await?;
            println!("Conexão bem-sucedida: {name}");
        }
    }
    Ok(())
}
