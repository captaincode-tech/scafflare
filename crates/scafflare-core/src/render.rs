use std::collections::BTreeMap;
use std::path::PathBuf;

use minijinja::{Environment, UndefinedBehavior};
use serde_json::{Map, Value};

use crate::error::{Result, ScafflareError};
use crate::recipe::{evaluate_condition, FileStrategy};
use crate::resolver::Resolution;

#[derive(Debug, Clone)]
pub struct RenderedFile {
    pub recipe: String,
    pub destination: PathBuf,
    pub strategy: FileStrategy,
    pub contents: Vec<u8>,
}

pub fn build_context(
    resolution: &Resolution,
    supplied: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut variables = supplied.clone();
    for recipe in &resolution.recipes {
        for (key, value) in &recipe.document.variables {
            variables
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    for recipe in &resolution.recipes {
        for (key, value) in &recipe.document.metadata.activation_variables {
            variables.insert(key.clone(), value.clone());
        }
    }
    variables
}

pub fn render_resolution(
    resolution: &Resolution,
    variables: &BTreeMap<String, Value>,
) -> Result<Vec<RenderedFile>> {
    let context = Value::Object(Map::from_iter(variables.clone()));
    let mut files = Vec::new();

    for recipe in &resolution.recipes {
        let recipe_name = &recipe.document.metadata.name;
        for spec in &recipe.document.files {
            if !evaluate_condition(spec.when.as_deref(), variables)? {
                continue;
            }
            let template = recipe.templates.get(&spec.source).ok_or_else(|| {
                ScafflareError::InvalidRecipe {
                    recipe: recipe_name.clone(),
                    reason: format!("template `{}` is referenced but missing", spec.source),
                }
            })?;
            let template =
                std::str::from_utf8(template).map_err(|error| ScafflareError::Render {
                    recipe: recipe_name.clone(),
                    file: spec.source.clone(),
                    reason: format!("template is not UTF-8: {error}"),
                })?;
            let contents = render_template(recipe_name, &spec.source, template, &context)?;
            files.push(RenderedFile {
                recipe: recipe_name.clone(),
                destination: PathBuf::from(&spec.destination),
                strategy: spec.strategy.clone(),
                contents: contents.into_bytes(),
            });
        }
    }
    files.sort_by(|left, right| {
        left.destination
            .cmp(&right.destination)
            .then(left.recipe.cmp(&right.recipe))
    });
    Ok(files)
}

fn render_template(recipe: &str, source: &str, template: &str, context: &Value) -> Result<String> {
    let mut environment = Environment::new();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    environment
        .render_str(template, context)
        .map_err(|error| ScafflareError::Render {
            recipe: recipe.to_owned(),
            file: source.to_owned(),
            reason: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_renderer_reports_missing_variable() {
        let result = render_template("test", "file", "{{ missing }}", &Value::Null);
        assert!(matches!(result, Err(ScafflareError::Render { .. })));
    }
}
