use anyhow::{anyhow, bail, Context as _, Result};
use rquickjs::{Context, Function, Object, Persistent, Runtime, Value};
use serde_json::Value as Json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ParamDef {
    pub name: String,
    pub message: String,
    pub choices: Option<Vec<String>>,
    pub default: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromptDef {
    pub message: String,
    pub value_field: String,
    pub label_field: String,
    pub param_name: String,
    pub multiple: bool,
}

pub struct QueryDef {
    pub name: String,
    pub database: Option<String>,
    pub debug: bool,
    pub prompt: Option<PromptDef>,
    sql: Persistent<Function<'static>>,
}

pub enum Mode {
    Sequential,
    Parallel,
}

pub struct Script {
    pub database: String,
    pub mode: Mode,
    pub params: Vec<ParamDef>,
    pub queries: Vec<QueryDef>,
    // Kept alive for the whole life of the Script: the Persistent<Function>s
    // stored above are only valid as long as this runtime/context exist.
    _runtime: Runtime,
    context: Context,
}

fn get_opt_string(obj: &Object, key: &str) -> Result<Option<String>> {
    if !obj.contains_key(key)? {
        return Ok(None);
    }
    let v: Value = obj.get(key)?;
    if v.is_undefined() || v.is_null() {
        return Ok(None);
    }
    Ok(Some(
        v.as_string()
            .ok_or_else(|| anyhow!("campo \"{key}\" deveria ser string"))?
            .to_string()?,
    ))
}

fn get_opt_bool(obj: &Object, key: &str) -> Result<bool> {
    if !obj.contains_key(key)? {
        return Ok(false);
    }
    let v: Value = obj.get(key)?;
    Ok(v.as_bool().unwrap_or(false))
}

fn get_opt_string_array(obj: &Object, key: &str) -> Result<Option<Vec<String>>> {
    if !obj.contains_key(key)? {
        return Ok(None);
    }
    let v: Value = obj.get(key)?;
    if v.is_undefined() || v.is_null() {
        return Ok(None);
    }
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow!("campo \"{key}\" deveria ser array"))?;
    let mut out = Vec::new();
    for item in arr.iter::<String>() {
        out.push(item?);
    }
    Ok(Some(out))
}

/// Resolve o caminho absoluto do script: absoluto, bare filename (procura em
/// scripts_dir) ou relativo ao cwd.
pub fn resolve_script_path(file: &str, scripts_dir: &Path) -> Result<PathBuf> {
    let p = Path::new(file);

    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else if !file.contains('/') && !file.contains(std::path::MAIN_SEPARATOR) {
        scripts_dir.join(file)
    } else {
        std::env::current_dir()?.join(p)
    };

    if !absolute.exists() {
        bail!("Arquivo de script não encontrado: {}", absolute.display());
    }

    Ok(absolute)
}

pub fn load(path: &Path) -> Result<Script> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Falha ao ler script: {}", path.display()))?;

    let runtime = Runtime::new().context("Falha ao iniciar runtime JS")?;
    let context = Context::full(&runtime).context("Falha ao iniciar contexto JS")?;

    context.with(|ctx| -> Result<()> {
        let console = Object::new(ctx.clone())?;
        let log = Function::new(ctx.clone(), |args: rquickjs::function::Rest<Value>| {
            let parts: Vec<String> = args
                .0
                .iter()
                .map(|v| {
                    v.as_string()
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_else(|| js_to_json(v).map(|j| j.to_string()).unwrap_or_default())
                })
                .collect();
            println!("{}", parts.join(" "));
        })?;
        console.set("log", log)?;
        ctx.globals().set("console", console)?;
        Ok(())
    })?;

    // Wrap the script's `module.exports = {...}` CommonJS style in an IIFE
    // that returns the exports object, so we can grab it directly.
    let wrapped = format!(
        "(function() {{ const module = {{ exports: {{}} }}; const exports = module.exports; {source}\nreturn module.exports; }})()"
    );

    let (database, mode, params, queries) = context.with(|ctx| -> Result<_> {
        let exports: Value = ctx
            .eval(wrapped.into_bytes())
            .map_err(|e| anyhow!("Erro ao avaliar script JS: {e}\n{}", describe_js_error(&ctx)))?;

        let obj = exports
            .as_object()
            .ok_or_else(|| anyhow!("O script \"{}\" deve exportar um objeto.", path.display()))?;

        let database = get_opt_string(obj, "database")?
            .ok_or_else(|| anyhow!("O script \"{}\" deve declarar \"database\".", path.display()))?;

        let mode = match get_opt_string(obj, "mode")?.as_deref() {
            Some("parallel") => Mode::Parallel,
            _ => Mode::Sequential,
        };

        let mut params = Vec::new();
        if obj.contains_key("params")? {
            let params_val: Value = obj.get("params")?;
            if let Some(arr) = params_val.as_array() {
                for item in arr.iter::<Object>() {
                    let item = item?;
                    let name = get_opt_string(&item, "name")?
                        .ok_or_else(|| anyhow!("Cada param do script deve ter um campo \"name\"."))?;
                    let message =
                        get_opt_string(&item, "message")?.unwrap_or_else(|| format!("{name}:"));
                    let choices = get_opt_string_array(&item, "choices")?;
                    let default = get_opt_string(&item, "default")?;
                    params.push(ParamDef {
                        name,
                        message,
                        choices,
                        default,
                    });
                }
            }
        }

        let queries_val: Value = obj
            .get("queries")
            .map_err(|e| anyhow!("{e}"))?;
        let queries_arr = queries_val.as_array().ok_or_else(|| {
            anyhow!(
                "O script \"{}\" deve declarar um array \"queries\" não-vazio.",
                path.display()
            )
        })?;

        if queries_arr.is_empty() {
            bail!(
                "O script \"{}\" deve declarar um array \"queries\" não-vazio.",
                path.display()
            );
        }

        let mut queries = Vec::new();
        for (idx, item) in queries_arr.iter::<Object>().enumerate() {
            let item = item?;
            let name =
                get_opt_string(&item, "name")?.unwrap_or_else(|| format!("Query {}", idx + 1));
            let database = get_opt_string(&item, "database")?;
            let debug = get_opt_bool(&item, "debug")?;

            let prompt = if item.contains_key("prompt")? {
                let p: Object = item.get("prompt")?;
                Some(PromptDef {
                    message: get_opt_string(&p, "message")?.unwrap_or_default(),
                    value_field: get_opt_string(&p, "valueField")?.unwrap_or_default(),
                    label_field: get_opt_string(&p, "labelField")?.unwrap_or_default(),
                    param_name: get_opt_string(&p, "paramName")?.unwrap_or_default(),
                    multiple: get_opt_bool(&p, "multiple")?,
                })
            } else {
                None
            };

            let sql_val: Value = item.get("sql")?;
            let sql_fn = sql_val
                .as_function()
                .ok_or_else(|| anyhow!("Query \"{name}\" deve declarar \"sql\" como função."))?
                .clone();
            let sql = Persistent::save(&ctx, sql_fn);

            queries.push(QueryDef {
                name,
                database,
                debug,
                prompt,
                sql,
            });
        }

        Ok((database, mode, params, queries))
    })?;

    Ok(Script {
        database,
        mode,
        params,
        queries,
        _runtime: runtime,
        context,
    })
}

fn describe_js_error(ctx: &rquickjs::Ctx) -> String {
    if let Some(ex) = ctx.catch().as_exception() {
        ex.to_string()
    } else {
        String::new()
    }
}

/// Converte um serde_json::Value em um valor JS dentro do contexto informado.
fn json_to_js<'js>(ctx: &rquickjs::Ctx<'js>, value: &Json) -> Result<Value<'js>> {
    Ok(match value {
        Json::Null => Value::new_null(ctx.clone()),
        Json::Bool(b) => Value::new_bool(ctx.clone(), *b),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::new_int(ctx.clone(), i as i32)
            } else {
                Value::new_float(ctx.clone(), n.as_f64().unwrap_or_default())
            }
        }
        Json::String(s) => rquickjs::String::from_str(ctx.clone(), s)?.into_value(),
        Json::Array(items) => {
            let arr = rquickjs::Array::new(ctx.clone())?;
            for (i, item) in items.iter().enumerate() {
                arr.set(i, json_to_js(ctx, item)?)?;
            }
            arr.into_value()
        }
        Json::Object(map) => {
            let obj = Object::new(ctx.clone())?;
            for (k, v) in map {
                obj.set(k.as_str(), json_to_js(ctx, v)?)?;
            }
            obj.into_value()
        }
    })
}

/// Converte um valor JS (esperado: primitivo, array de objetos, ou objeto)
/// em serde_json::Value.
fn js_to_json(value: &Value) -> Result<Json> {
    if value.is_undefined() || value.is_null() {
        return Ok(Json::Null);
    }
    if let Some(b) = value.as_bool() {
        return Ok(Json::Bool(b));
    }
    if let Some(i) = value.as_int() {
        return Ok(Json::from(i));
    }
    if let Some(f) = value.as_float() {
        return Ok(serde_json::Number::from_f64(f)
            .map(Json::Number)
            .unwrap_or(Json::Null));
    }
    if let Some(s) = value.as_string() {
        return Ok(Json::String(s.to_string()?));
    }
    if let Some(arr) = value.as_array() {
        let mut out = Vec::new();
        for item in arr.iter::<Value>() {
            out.push(js_to_json(&item?)?);
        }
        return Ok(Json::Array(out));
    }
    if let Some(obj) = value.as_object() {
        let mut map = serde_json::Map::new();
        for key in obj.keys::<String>() {
            let key = key?;
            let v: Value = obj.get(&key)?;
            map.insert(key, js_to_json(&v)?);
        }
        return Ok(Json::Object(map));
    }
    Ok(Json::Null)
}

impl Script {
    /// Executa `query.sql(params, prev)` e retorna o SQL resultante como String.
    pub fn render_sql(
        &self,
        query: &QueryDef,
        params: &HashMap<String, Json>,
        prev: &[HashMap<String, Json>],
    ) -> Result<String> {
        self.context.with(|ctx| -> Result<String> {
            let func = query.sql.clone().restore(&ctx)?;

            let params_json = Json::Object(
                params
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            );
            let params_js = json_to_js(&ctx, &params_json)?;

            let prev_json = Json::Array(
                prev.iter()
                    .map(|row| Json::Object(row.iter().map(|(k, v)| (k.clone(), v.clone())).collect()))
                    .collect(),
            );
            let prev_js = json_to_js(&ctx, &prev_json)?;

            let result: Value = func
                .call((params_js, prev_js))
                .map_err(|e| anyhow!("Erro ao executar sql() da query \"{}\": {e}\n{}", query.name, describe_js_error(&ctx)))?;

            let s = result
                .as_string()
                .ok_or_else(|| {
                    anyhow!(
                        "Query \"{}\" produziu SQL inválido (não-string ou vazio).",
                        query.name
                    )
                })?
                .to_string()?;

            if s.trim().is_empty() {
                bail!(
                    "Query \"{}\" produziu SQL inválido (não-string ou vazio).",
                    query.name
                );
            }

            Ok(s)
        })
    }
}
