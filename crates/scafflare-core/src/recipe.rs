use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, ScafflareError};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDocument {
    pub schema_version: u32,
    pub metadata: RecipeMetadata,
    #[serde(default)]
    pub prompts: Vec<Prompt>,
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub compatible_with: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub files: Vec<FileSpec>,
    #[serde(default)]
    pub validation_commands: Vec<CommandSpec>,
    #[serde(default)]
    pub post_generation_instructions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeMetadata {
    pub name: String,
    pub version: Version,
    pub description: String,
    #[serde(default)]
    pub wizard_entrypoint: bool,
    #[serde(default)]
    pub capability_probes: Vec<CapabilityProbe>,
    #[serde(default)]
    pub activation_variables: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProbe {
    pub capability: String,
    pub program: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    pub key: String,
    pub message: String,
    #[serde(default = "default_prompt_kind")]
    pub kind: PromptKind,
    #[serde(default)]
    pub options: Vec<PromptOption>,
    #[serde(default)]
    pub recipes: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default)]
    pub order: u16,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    Select,
    Confirm,
    Text,
}

fn default_prompt_kind() -> PromptKind {
    PromptKind::Text
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptOption {
    pub label: String,
    pub value: Value,
    #[serde(default)]
    pub recipes: Vec<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStrategy {
    Create,
    Replace,
    MergeJson,
    Skip,
    Fail,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileSpec {
    pub source: String,
    pub destination: String,
    pub strategy: FileStrategy,
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CommandSpec {
    ShellLike(String),
    Args(Vec<String>),
}

impl CommandSpec {
    pub fn display(&self) -> String {
        match self {
            Self::ShellLike(value) => value.clone(),
            Self::Args(values) => values.join(" "),
        }
    }

    pub fn program_and_args(&self) -> Option<(&str, Vec<&str>)> {
        match self {
            Self::ShellLike(_) => None,
            Self::Args(values) if !values.is_empty() => {
                Some((&values[0], values[1..].iter().map(String::as_str).collect()))
            }
            Self::Args(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedRecipe {
    pub document: RecipeDocument,
    pub templates: BTreeMap<String, Vec<u8>>,
}

impl RecipeDocument {
    pub fn from_yaml(recipe_id: &str, input: &str) -> Result<Self> {
        let document: Self =
            serde_yaml::from_str(input).map_err(|error| ScafflareError::InvalidRecipe {
                recipe: recipe_id.to_owned(),
                reason: error.to_string(),
            })?;
        document.validate()?;
        Ok(document)
    }

    pub fn validate(&self) -> Result<()> {
        let name = &self.metadata.name;
        if self.schema_version != 1 {
            return Err(ScafflareError::InvalidRecipe {
                recipe: name.clone(),
                reason: format!("unsupported schema_version `{}`", self.schema_version),
            });
        }
        if !is_kebab_case(name) {
            return Err(ScafflareError::InvalidRecipe {
                recipe: name.clone(),
                reason: "metadata.name must use kebab-case".to_owned(),
            });
        }
        if self.metadata.description.trim().is_empty() {
            return Err(ScafflareError::InvalidRecipe {
                recipe: name.clone(),
                reason: "metadata.description must not be empty".to_owned(),
            });
        }

        let mut prompt_keys = HashSet::new();
        for prompt in &self.prompts {
            if !is_identifier(&prompt.key) || !prompt_keys.insert(&prompt.key) {
                return Err(ScafflareError::InvalidRecipe {
                    recipe: name.clone(),
                    reason: format!(
                        "prompt key `{}` must be unique and identifier-like",
                        prompt.key
                    ),
                });
            }
        }

        for key in self.variables.keys() {
            if !is_identifier(key) {
                return Err(ScafflareError::InvalidRecipe {
                    recipe: name.clone(),
                    reason: format!("variable key `{key}` must be identifier-like"),
                });
            }
        }

        for file in &self.files {
            validate_relative_path(name, &file.source)?;
            validate_relative_path(name, &file.destination)?;
            if !file.source.starts_with("templates/") {
                return Err(ScafflareError::InvalidRecipe {
                    recipe: name.clone(),
                    reason: format!("file source `{}` must live below templates/", file.source),
                });
            }
        }
        Ok(())
    }
}

pub fn validate_relative_path(recipe: &str, value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || value.contains('\\')
        || value.starts_with('/')
        || value.starts_with('~')
        || (value.len() >= 2 && value.as_bytes()[1] == b':')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ScafflareError::UnsafePath {
            recipe: recipe.to_owned(),
            path: value.to_owned(),
        });
    }
    Ok(())
}

pub fn evaluate_condition(
    condition: Option<&str>,
    variables: &BTreeMap<String, Value>,
) -> Result<bool> {
    let Some(condition) = condition.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(true);
    };
    if condition.contains("&&") {
        return condition
            .split("&&")
            .map(|part| evaluate_condition(Some(part.trim()), variables))
            .try_fold(true, |all, next| next.map(|value| all && value));
    }
    let (key, operator, expected) = if let Some((key, value)) = condition.split_once("==") {
        (key.trim(), "==", value.trim())
    } else if let Some((key, value)) = condition.split_once("!=") {
        (key.trim(), "!=", value.trim())
    } else {
        let value = variables.get(condition).ok_or_else(|| {
            ScafflareError::InvalidInput(format!(
                "condition references unknown variable `{condition}`"
            ))
        })?;
        return value.as_bool().ok_or_else(|| {
            ScafflareError::InvalidInput(format!(
                "condition `{condition}` must reference a boolean variable"
            ))
        });
    };

    let actual = variables.get(key).ok_or_else(|| {
        ScafflareError::InvalidInput(format!("condition references unknown variable `{key}`"))
    })?;
    let expected = expected.trim_matches('"').trim_matches('\'');
    let actual_text = match actual {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => {
            return Err(ScafflareError::InvalidInput(format!(
                "condition `{condition}` uses unsupported value type"
            )))
        }
    };
    Ok(if operator == "==" {
        actual_text == expected
    } else {
        actual_text != expected
    })
}

fn is_kebab_case(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_and_windows_prefixes() {
        for path in [
            "../escape",
            "C:\\escape",
            "/absolute",
            "nested/../../escape",
        ] {
            assert!(validate_relative_path("test", path).is_err(), "{path}");
        }
        assert!(validate_relative_path("test", "src/main.ts").is_ok());
    }

    #[test]
    fn evaluates_simple_conditions() {
        let variables = BTreeMap::from([
            ("database".to_owned(), Value::Bool(true)),
            ("framework".to_owned(), Value::String("express".to_owned())),
        ]);
        assert!(evaluate_condition(Some("database"), &variables).unwrap());
        assert!(evaluate_condition(Some("framework == express"), &variables).unwrap());
        assert!(!evaluate_condition(Some("framework != express"), &variables).unwrap());
    }
}
