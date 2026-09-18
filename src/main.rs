use anyhow::{bail, Result};
use clap::Parser;
use dialoguer::Select;
use rundb::cli::Cli;
use rundb::{connection, db_script, help, params, runner};
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
    let mut parts = vec!["rundb".to_string(), format!("-f {file}"), format!("-e {env}")];

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

    let file = match &cli.file {
        Some(f) => f.clone(),
        None => {
            if !dir.exists() {
                bail!("Pasta de scripts não encontrada: {}", dir.display());
            }
            let mut scripts: Vec<String> = std::fs::read_dir(&dir)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|f| f.ends_with(".js"))
                .collect();
            scripts.sort();

            if scripts.is_empty() {
                bail!("Nenhum script encontrado em {}.", dir.display());
            }

            let selection = Select::new()
                .with_prompt("Qual script deseja executar?")
                .items(&scripts)
                .default(0)
                .interact()?;
            scripts[selection].clone()
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

    let provided_params = params::parse_param_flags(&cli.param)?;
    let mut resolved_params = params::resolve_params(&script.params, &provided_params)?;

    if !resolved_params.is_empty() {
        println!(
            "Params:    {}",
            serde_json::to_string(&resolved_params).unwrap_or_default()
        );
    }

    let mut pools = HashMap::new();
    for db in &databases {
        let client = connection::connect(&cli.env, db).await?;
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
            runner::print_results(
                std::slice::from_ref(last),
                cli.json,
            );
        }
    }

    println!("\nConcluído.\n");
    println!(
        "Para repetir:\n  {}\n",
        build_repeat_command(&file, &cli.env, &resolved_params, cli.json, cli.debug)
    );

    Ok(())
}
