use crate::db_script::ParamDef;
use anyhow::{bail, Result};
use dialoguer::{Input, Select};
use serde_json::Value as Json;
use std::collections::HashMap;

/// Parseia os --param chave=valor em um mapa chave -> Json.
/// Tenta interpretar o valor como JSON; se falhar, usa como string crua.
pub fn parse_param_flags(raw: &[String]) -> Result<HashMap<String, Json>> {
    let mut out = HashMap::new();

    for entry in raw {
        let idx = entry
            .find('=')
            .ok_or_else(|| anyhow::anyhow!("Param inválido: \"{entry}\". Use o formato chave=valor."))?;
        let key = entry[..idx].trim().to_string();
        let raw_value = &entry[idx + 1..];

        if key.is_empty() {
            bail!("Param inválido: \"{entry}\". Chave vazia.");
        }

        let value = serde_json::from_str::<Json>(raw_value)
            .unwrap_or_else(|_| Json::String(raw_value.to_string()));
        out.insert(key, value);
    }

    Ok(out)
}

/// Resolve os params declarados pelo script: usa o valor fornecido via --param
/// ou solicita interativamente (select se houver `choices`, senão input livre).
pub fn resolve_params(
    declared: &[ParamDef],
    provided: &HashMap<String, Json>,
) -> Result<HashMap<String, Json>> {
    let mut resolved = HashMap::new();

    for decl in declared {
        if let Some(v) = provided.get(&decl.name) {
            resolved.insert(decl.name.clone(), v.clone());
            continue;
        }

        let answer = if let Some(choices) = &decl.choices {
            let default_idx = decl
                .default
                .as_ref()
                .and_then(|d| choices.iter().position(|c| c == d))
                .unwrap_or(0);

            let selection = Select::new()
                .with_prompt(decl.message.clone())
                .items(choices)
                .default(default_idx)
                .interact()?;
            choices[selection].clone()
        } else {
            let mut input = Input::<String>::new().with_prompt(decl.message.clone());
            if let Some(d) = &decl.default {
                input = input.default(d.clone());
            }
            input.interact_text()?
        };

        resolved.insert(decl.name.clone(), Json::String(answer));
    }

    Ok(resolved)
}
