use anyhow::{anyhow, Result};
use comfy_table::{presets::UTF8_FULL, Table};
use dialoguer::{MultiSelect, Select};
use futures::future::join_all;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::process::{Command, Stdio};
use tiberius::Row;

use crate::connection::DbClient;
use crate::db_script::{PromptDef, QueryDef, Script};

pub type Record = HashMap<String, Json>;

pub struct QueryResult {
    pub name: String,
    pub recordset: Vec<Record>,
}

fn cell_to_json(row: &Row, idx: usize) -> Json {
    macro_rules! try_type {
        ($t:ty, $conv:expr) => {
            if let Ok(v) = row.try_get::<$t, usize>(idx) {
                return match v {
                    Some(val) => $conv(val),
                    None => Json::Null,
                };
            }
        };
    }

    try_type!(&str, |v: &str| Json::String(v.to_string()));
    try_type!(i32, |v: i32| Json::from(v));
    try_type!(i64, |v: i64| Json::from(v));
    try_type!(i16, |v: i16| Json::from(v));
    try_type!(u8, |v: u8| Json::from(v));
    try_type!(f64, |v: f64| serde_json::Number::from_f64(v)
        .map(Json::Number)
        .unwrap_or(Json::Null));
    try_type!(f32, |v: f32| serde_json::Number::from_f64(v as f64)
        .map(Json::Number)
        .unwrap_or(Json::Null));
    try_type!(bool, |v: bool| Json::Bool(v));
    try_type!(uuid::Uuid, |v: uuid::Uuid| Json::String(v.to_string()));
    try_type!(rust_decimal::Decimal, |v: rust_decimal::Decimal| {
        Json::String(v.to_string())
    });
    try_type!(chrono::NaiveDateTime, |v: chrono::NaiveDateTime| Json::String(
        v.to_string()
    ));
    try_type!(chrono::NaiveDate, |v: chrono::NaiveDate| Json::String(
        v.to_string()
    ));

    Json::Null
}

fn row_to_record(row: &Row) -> Record {
    let mut map = Record::new();
    for (idx, col) in row.columns().iter().enumerate() {
        map.insert(col.name().to_string(), cell_to_json(row, idx));
    }
    map
}

async fn execute_query(client: &mut DbClient, sql: &str) -> Result<Vec<Record>> {
    let stream = client.simple_query(sql).await?;
    let rows = stream.into_first_result().await?;
    Ok(rows.iter().map(row_to_record).collect())
}

async fn apply_prompt(prompt: &PromptDef, recordset: &[Record], params: &mut HashMap<String, Json>) -> Result<()> {
    if recordset.is_empty() {
        println!("    (sem registros — prompt \"{}\" ignorado)", prompt.message);
        return Ok(());
    }

    let labels: Vec<String> = recordset
        .iter()
        .map(|row| {
            row.get(&prompt.label_field)
                .or_else(|| row.get(&prompt.value_field))
                .map(json_display)
                .unwrap_or_else(|| "?".to_string())
        })
        .collect();

    if prompt.multiple {
        let selected = MultiSelect::new()
            .with_prompt(prompt.message.clone())
            .items(&labels)
            .interact()?;
        let values: Vec<Json> = selected
            .into_iter()
            .map(|i| recordset[i].get(&prompt.value_field).cloned().unwrap_or(Json::Null))
            .collect();
        params.insert(prompt.param_name.clone(), Json::Array(values));
    } else {
        let selected = Select::new()
            .with_prompt(prompt.message.clone())
            .items(&labels)
            .default(0)
            .interact()?;
        let value = recordset[selected]
            .get(&prompt.value_field)
            .cloned()
            .unwrap_or(Json::Null);
        params.insert(prompt.param_name.clone(), value);
    }

    Ok(())
}

fn json_display(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Null => String::new(),
        other => other.to_string(),
    }
}

pub async fn run_sequential(
    pools: &mut HashMap<String, DbClient>,
    default_database: &str,
    script: &Script,
    queries: &[QueryDef],
    params: &mut HashMap<String, Json>,
    debug: bool,
    json: bool,
) -> Result<Vec<QueryResult>> {
    let mut results = Vec::new();
    let mut prev: Vec<Record> = Vec::new();

    for query in queries {
        let label = query.name.clone();
        println!("\n→ {label}");

        let sql = script.render_sql(query, params, &prev)?;
        let db_key = query.database.as_deref().unwrap_or(default_database);
        let client = pools
            .get_mut(db_key)
            .ok_or_else(|| anyhow!("Pool não encontrada para database \"{db_key}\""))?;
        let recordset = execute_query(client, &sql).await?;

        if debug || query.debug {
            println!("\nSQL:\n{sql}");
            print_results(
                &[QueryResult {
                    name: label.clone(),
                    recordset: recordset.clone(),
                }],
                json,
            );
        }

        if let Some(prompt) = &query.prompt {
            apply_prompt(prompt, &recordset, params).await?;
        }

        prev = recordset.clone();
        results.push(QueryResult {
            name: label,
            recordset,
        });
    }

    Ok(results)
}

pub async fn run_parallel(
    pools: &mut HashMap<String, DbClient>,
    default_database: &str,
    script: &Script,
    queries: &[QueryDef],
    params: &HashMap<String, Json>,
    json: bool,
) -> Result<Vec<QueryResult>> {
    println!("\n→ Executando {} queries em paralelo...", queries.len());

    // Monta o SQL de cada query de forma síncrona (JS) antes de disparar as
    // execuções assíncronas concorrentes.
    let mut prepared = Vec::new();
    for query in queries {
        let sql = script.render_sql(query, params, &[])?;
        let db_key = query.database.as_deref().unwrap_or(default_database).to_string();
        prepared.push((query.name.clone(), db_key, sql, query.debug));
    }

    // tiberius Client não é Clone; para paralelizar de verdade seria preciso
    // um client por task. Como já abrimos uma pool por database único,
    // reaproveitamos: se duas queries usam o mesmo database elas rodam em
    // sequência entre si, mas databases diferentes rodam concorrentemente.
    let mut by_db: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, (_, db_key, _, _)) in prepared.iter().enumerate() {
        by_db.entry(db_key.clone()).or_default().push(idx);
    }

    let mut clients: HashMap<String, DbClient> = HashMap::new();
    for db_key in by_db.keys() {
        if let Some(c) = pools.remove(db_key) {
            clients.insert(db_key.clone(), c);
        }
    }

    let futures = clients.into_iter().map(|(db_key, mut client)| {
        let indices = by_db.get(&db_key).cloned().unwrap_or_default();
        let prepared = &prepared;
        async move {
            let mut out = Vec::new();
            for idx in indices {
                let (name, _, sql, debug) = &prepared[idx];
                let recordset = execute_query(&mut client, sql).await?;
                out.push((idx, name.clone(), sql.clone(), *debug, recordset));
            }
            Ok::<_, anyhow::Error>((db_key, client, out))
        }
    });

    let outcomes = join_all(futures).await;

    let mut ordered: Vec<Option<QueryResult>> = (0..prepared.len()).map(|_| None).collect();
    for outcome in outcomes {
        let (db_key, client, out) = outcome?;
        pools.insert(db_key, client);
        for (idx, name, sql, debug, recordset) in out {
            if debug {
                println!("\n→ {name}");
                println!("\nSQL:\n{sql}");
                print_results(
                    &[QueryResult {
                        name: name.clone(),
                        recordset: recordset.clone(),
                    }],
                    json,
                );
            }
            ordered[idx] = Some(QueryResult { name, recordset });
        }
    }

    Ok(ordered.into_iter().map(|r| r.unwrap()).collect())
}

fn render_table(recordset: &[Record]) -> String {
    if recordset.is_empty() {
        return String::new();
    }
    let mut columns: Vec<String> = Vec::new();
    for row in recordset {
        for k in row.keys() {
            if !columns.contains(k) {
                columns.push(k.clone());
            }
        }
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(columns.clone());

    for row in recordset {
        let cells: Vec<String> = columns
            .iter()
            .map(|c| row.get(c).map(json_display).unwrap_or_default())
            .collect();
        table.add_row(cells);
    }

    table.to_string()
}

fn print_with_pager(text: &str) {
    let lines = text.lines().count();
    let is_tty = std::io::stdout().is_terminal();
    let terminal_rows = terminal_size::terminal_size()
        .map(|(_, h)| h.0 as usize)
        .unwrap_or(24);

    if lines <= terminal_rows || !is_tty {
        println!("{text}");
        return;
    }

    let result = Command::new("less")
        .arg("-S")
        .arg("-R")
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn();

    match result {
        Ok(mut child) => {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
        Err(_) => println!("{text}"),
    }
}

pub fn print_results(results: &[QueryResult], as_json: bool) {
    if as_json {
        let payload: Vec<_> = results
            .iter()
            .map(|r| serde_json::json!({ "name": r.name, "recordset": r.recordset }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return;
    }

    for r in results {
        let header = format!("\n=== {} ===", r.name);
        if r.recordset.is_empty() {
            println!("{header}");
            println!("(sem registros)");
        } else {
            let output = format!("{header}\n{}", render_table(&r.recordset));
            print_with_pager(&output);
        }
    }
}
