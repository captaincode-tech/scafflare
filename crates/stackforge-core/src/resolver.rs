use std::collections::{BTreeSet, HashMap, HashSet};

use crate::error::{Result, StackForgeError};
use crate::recipe::LoadedRecipe;
use crate::registry::RecipeRegistry;

#[derive(Debug, Clone)]
pub struct Resolution {
    pub recipes: Vec<LoadedRecipe>,
    pub requested: Vec<String>,
}

impl Resolution {
    pub fn names(&self) -> Vec<String> {
        self.recipes
            .iter()
            .map(|recipe| recipe.document.metadata.name.clone())
            .collect()
    }
}

pub struct Resolver<'a, R: RecipeRegistry> {
    registry: &'a R,
    capabilities: BTreeSet<String>,
}

impl<'a, R: RecipeRegistry> Resolver<'a, R> {
    pub fn new(registry: &'a R, capabilities: impl IntoIterator<Item = String>) -> Self {
        Self {
            registry,
            capabilities: capabilities.into_iter().collect(),
        }
    }

    pub fn resolve(&self, requested: &[String]) -> Result<Resolution> {
        if requested.is_empty() {
            return Err(StackForgeError::InvalidInput(
                "at least one recipe must be selected".to_owned(),
            ));
        }

        let mut loaded = HashMap::<String, LoadedRecipe>::new();
        let mut permanent = HashSet::<String>::new();
        let mut temporary = Vec::<String>::new();
        let mut order = Vec::<String>::new();
        for recipe in requested {
            self.visit(
                recipe,
                &mut loaded,
                &mut permanent,
                &mut temporary,
                &mut order,
            )?;
        }

        let selected: BTreeSet<String> = order.iter().cloned().collect();
        for name in &order {
            let recipe = loaded
                .get(name)
                .expect("resolver must load every selected recipe");
            for conflict in &recipe.document.conflicts {
                if selected.contains(conflict) {
                    return Err(StackForgeError::Conflict {
                        left: name.clone(),
                        right: conflict.clone(),
                    });
                }
            }
            if !recipe.document.compatible_with.is_empty()
                && !recipe
                    .document
                    .compatible_with
                    .iter()
                    .any(|candidate| selected.contains(candidate))
            {
                return Err(StackForgeError::Incompatible {
                    recipe: name.clone(),
                    selected: recipe.document.compatible_with.join(", "),
                });
            }
            for capability in &recipe.document.required_capabilities {
                if !self.capabilities.contains(capability) {
                    return Err(StackForgeError::MissingCapability {
                        recipe: name.clone(),
                        capability: capability.clone(),
                    });
                }
            }
        }

        let recipes = order
            .iter()
            .map(|name| {
                loaded
                    .remove(name)
                    .expect("resolved recipe must be available")
            })
            .collect();
        Ok(Resolution {
            recipes,
            requested: requested.to_vec(),
        })
    }

    fn visit(
        &self,
        name: &str,
        loaded: &mut HashMap<String, LoadedRecipe>,
        permanent: &mut HashSet<String>,
        temporary: &mut Vec<String>,
        order: &mut Vec<String>,
    ) -> Result<()> {
        if permanent.contains(name) {
            return Ok(());
        }
        if let Some(position) = temporary.iter().position(|entry| entry == name) {
            let mut cycle = temporary[position..].to_vec();
            cycle.push(name.to_owned());
            return Err(StackForgeError::DependencyCycle(cycle.join(" -> ")));
        }

        temporary.push(name.to_owned());
        let recipe = self.registry.get(name)?;
        for dependency in &recipe.document.depends_on {
            self.visit(dependency, loaded, permanent, temporary, order)?;
        }
        temporary.pop();
        permanent.insert(name.to_owned());
        loaded.insert(name.to_owned(), recipe);
        order.push(name.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use semver::Version;

    use super::*;
    use crate::recipe::{RecipeDocument, RecipeMetadata};
    use crate::registry::{RecipeRegistry, RegistrySource};

    struct TestRegistry(BTreeMap<String, LoadedRecipe>);

    impl RecipeRegistry for TestRegistry {
        fn list(&self) -> Result<Vec<String>> {
            Ok(self.0.keys().cloned().collect())
        }
        fn get(&self, name: &str) -> Result<LoadedRecipe> {
            self.0
                .get(name)
                .cloned()
                .ok_or_else(|| StackForgeError::RecipeNotFound(name.to_owned()))
        }
        fn source(&self) -> RegistrySource {
            RegistrySource::Filesystem
        }
    }

    fn recipe(name: &str, dependencies: &[&str], conflicts: &[&str]) -> LoadedRecipe {
        LoadedRecipe {
            document: RecipeDocument {
                schema_version: 1,
                metadata: RecipeMetadata {
                    name: name.to_owned(),
                    version: Version::new(0, 1, 0),
                    description: "test".to_owned(),
                },
                prompts: vec![],
                variables: BTreeMap::new(),
                depends_on: dependencies
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect(),
                compatible_with: vec![],
                conflicts: conflicts.iter().map(|value| (*value).to_owned()).collect(),
                required_capabilities: vec![],
                files: vec![],
                validation_commands: vec![],
                post_generation_instructions: vec![],
            },
            templates: BTreeMap::new(),
        }
    }

    #[test]
    fn resolves_dependencies_before_dependents() {
        let registry = TestRegistry(BTreeMap::from([
            ("node".to_owned(), recipe("node", &[], &[])),
            ("api".to_owned(), recipe("api", &["node"], &[])),
        ]));
        let resolution = Resolver::new(&registry, Vec::<String>::new())
            .resolve(&["api".to_owned()])
            .unwrap();
        assert_eq!(resolution.names(), vec!["node", "api"]);
    }

    #[test]
    fn detects_conflicts() {
        let registry = TestRegistry(BTreeMap::from([
            ("a".to_owned(), recipe("a", &[], &["b"])),
            ("b".to_owned(), recipe("b", &[], &[])),
        ]));
        let error = Resolver::new(&registry, Vec::<String>::new())
            .resolve(&["a".to_owned(), "b".to_owned()])
            .unwrap_err();
        assert!(matches!(error, StackForgeError::Conflict { .. }));
    }
}
