use rundb::db_script;
use std::collections::HashMap;
use std::path::Path;

fn main() {
    let dir = Path::new("db-scripts");
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "js").unwrap_or(false))
        .collect();
    entries.sort();

    for path in entries {
        print!("{}: ", path.display());
        match db_script::load(&path) {
            Ok(script) => {
                let params: HashMap<String, serde_json::Value> = script
                    .params
                    .iter()
                    .map(|p| (p.name.clone(), serde_json::Value::String("test".into())))
                    .collect();

                let mut ok = true;
                for q in &script.queries {
                    match script.render_sql(q, &params, &[]) {
                        Ok(sql) => {
                            if sql.trim().is_empty() {
                                ok = false;
                                println!("\n  query \"{}\" -> SQL vazio", q.name);
                            }
                        }
                        Err(e) => {
                            ok = false;
                            println!("\n  query \"{}\" ERRO: {e}", q.name);
                        }
                    }
                }
                if script.description.is_some() {
                    print!("[com descrição] ");
                }
                if ok {
                    println!(
                        "OK (database={}, {} queries)",
                        script.database,
                        script.queries.len()
                    );
                }
            }
            Err(e) => println!("FALHOU: {e}"),
        }
    }
}
