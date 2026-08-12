use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use serde::Serialize;
use serde_json::Value;
use stackforge_core::plan::{ChangeKind, FilePlan};
use stackforge_core::recipe::RecipeDocument;
use stackforge_core::registry::{BundledRegistry, RecipeRegistry};
use stackforge_core::state::load_state;
use stackforge_core::{
    commit, preview_add, preview_init, preview_remove, run_safe_validation_commands,
    validation_commands, GenerationPreview, GenerationRequest,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Parser)]
#[command(name = "stackforge", version = VERSION, about = "Composable, language-agnostic backend scaffolding")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Emit machine-readable JSON to stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Suppress successful human-readable output.
    #[arg(long, global = true, short = 'q')]
    quiet: bool,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create a project from composable recipes.
    Init(InitArgs),
    /// Add one or more recipes to an existing StackForge project.
    Add(ModifyArgs),
    /// Remove a recipe and its unmodified, exclusively owned files.
    Remove(RemoveArgs),
    /// List bundled recipes.
    List,
    /// Diagnose StackForge and the project state.
    Doctor(PathArgs),
    /// Validate the current project's lockfile and installed recipes.
    Validate(PathArgs),
    /// Work with standalone recipe files.
    Recipe {
        #[command(subcommand)]
        command: RecipeCommands,
    },
}

#[derive(Debug, Subcommand)]
enum RecipeCommands {
    /// Validate a recipe.yaml file and its local template references.
    Validate { path: PathBuf },
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Project directory name.
    project_name: String,
    /// Parent directory for the new project.
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    /// Skip wizard; provide selection flags instead.
    #[arg(long)]
    non_interactive: bool,
    /// HTTP framework: express, hono, none.
    #[arg(long, default_value = "express")]
    framework: String,
    /// Architecture: minimal, layered, clean.
    #[arg(long, default_value = "minimal")]
    architecture: String,
    /// Database: sqlite, none.
    #[arg(long, default_value = "none")]
    database: String,
    /// Enable Zod validation.
    #[arg(long)]
    zod: bool,
    /// Enable Pino logging.
    #[arg(long)]
    pino: bool,
    /// Enable Vitest.
    #[arg(long)]
    vitest: bool,
    /// Enable Biome.
    #[arg(long)]
    biome: bool,
    /// Enable Husky and lint-staged.
    #[arg(long)]
    hooks: bool,
    /// Enable GitHub Actions.
    #[arg(long)]
    github_actions: bool,
    /// Automatically approve the displayed file preview.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Run declared argv-form validation commands after commit.
    #[arg(long)]
    run_commands: bool,
}

#[derive(Debug, Args)]
struct ModifyArgs {
    /// Recipe names to add.
    #[arg(required = true, num_args = 1..)]
    recipes: Vec<String>,
    /// Existing project directory.
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    /// Automatically approve the displayed file preview.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Run declared argv-form validation commands after commit.
    #[arg(long)]
    run_commands: bool,
}

#[derive(Debug, Args)]
struct RemoveArgs {
    /// Installed recipe name.
    recipe: String,
    /// Existing project directory.
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    /// Automatically approve the displayed file preview.
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Debug, Args)]
struct PathArgs {
    /// Project directory.
    #[arg(long, default_value = ".")]
    directory: PathBuf,
}

#[derive(Debug, Serialize)]
struct JsonEnvelope<T: Serialize> {
    ok: bool,
    data: T,
}

#[derive(Debug, Serialize)]
struct JsonError {
    ok: bool,
    error: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string(&JsonError {
                        ok: false,
                        error: error.to_string()
                    })
                    .expect("JSON serialization cannot fail")
                );
            } else {
                eprintln!("{} {error}", style("error:").red().bold());
            }
            ExitCode::from(1)
        }
    }
}

fn execute(cli: &Cli) -> anyhow::Result<()> {
    match &cli.command {
        Commands::Init(args) => init(cli, args),
        Commands::Add(args) => add(cli, args),
        Commands::Remove(args) => remove(cli, args),
        Commands::List => list(cli),
        Commands::Doctor(args) => doctor(cli, args),
        Commands::Validate(args) => validate_project(cli, args),
        Commands::Recipe {
            command: RecipeCommands::Validate { path },
        } => validate_recipe(cli, path),
    }
}

fn init(cli: &Cli, args: &InitArgs) -> anyhow::Result<()> {
    if cli.json && !args.non_interactive {
        anyhow::bail!("--json requires --non-interactive for init");
    }
    let selection = if args.non_interactive {
        Selection::from_args(args)?
    } else {
        interactive_selection(args)?
    };
    let root = args.directory.join(&args.project_name);
    let request = GenerationRequest {
        project_name: args.project_name.clone(),
        recipes: selection.recipes(),
        variables: selection.variables(&args.project_name),
        capabilities: capabilities(),
    };
    let registry = BundledRegistry::new();
    let preview = preview_init(&root, &registry, request)?;
    execute_preview(cli, &root, preview, args.yes, args.run_commands)
}

fn add(cli: &Cli, args: &ModifyArgs) -> anyhow::Result<()> {
    let registry = BundledRegistry::new();
    let preview = preview_add(
        &args.directory,
        &registry,
        args.recipes.clone(),
        capabilities(),
    )?;
    execute_preview(cli, &args.directory, preview, args.yes, args.run_commands)
}

fn remove(cli: &Cli, args: &RemoveArgs) -> anyhow::Result<()> {
    let registry = BundledRegistry::new();
    let (plan, state) = preview_remove(&args.directory, &registry, &args.recipe, capabilities())?;
    emit_plan(cli, &plan)?;
    if !confirm(cli, &plan, args.yes)? {
        return Ok(());
    }
    commit(&args.directory, &plan)?;
    if !cli.quiet && !cli.json {
        println!(
            "{} removed recipe `{}` from {}",
            style("done").green().bold(),
            args.recipe,
            args.directory.display()
        );
    }
    if cli.json {
        emit_json(&serde_json::json!({"plan": plan, "state": state, "removed": args.recipe}))?;
    }
    Ok(())
}

fn list(cli: &Cli) -> anyhow::Result<()> {
    let registry = BundledRegistry::new();
    let mut recipes = Vec::new();
    for name in registry.list()? {
        let recipe = registry.get(&name)?;
        recipes.push(serde_json::json!({
            "name": recipe.document.metadata.name,
            "version": recipe.document.metadata.version,
            "description": recipe.document.metadata.description,
        }));
    }
    if cli.json {
        emit_json(&recipes)?;
    } else {
        println!("{} bundled recipes", style(recipes.len()).cyan().bold());
        for recipe in recipes {
            println!(
                "  {}  {}",
                style(recipe["name"].as_str().unwrap_or_default()).green(),
                recipe["description"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn doctor(cli: &Cli, args: &PathArgs) -> anyhow::Result<()> {
    let node = command_available("node");
    let npm = command_available("npm");
    let state = load_state(&args.directory).ok();
    let report = serde_json::json!({
        "stackforge_version": VERSION,
        "project": args.directory,
        "node": node,
        "npm": npm,
        "managed_project": state.is_some(),
        "installed_recipes": state.as_ref().map(|value| value.recipes.clone()).unwrap_or_default(),
    });
    if cli.json {
        emit_json(&report)?;
    } else {
        println!("{} StackForge {}", style("doctor").cyan().bold(), VERSION);
        println!("  node: {}", status(node));
        println!("  npm:  {}", status(npm));
        println!(
            "  state: {}",
            if state.is_some() {
                style("healthy").green()
            } else {
                style("not initialized").yellow()
            }
        );
    }
    Ok(())
}

fn validate_project(cli: &Cli, args: &PathArgs) -> anyhow::Result<()> {
    let state = load_state(&args.directory)?;
    let registry = BundledRegistry::new();
    let mut checked = Vec::new();
    for installed in &state.recipes {
        let recipe = registry.get(&installed.name)?;
        if recipe.document.metadata.version != installed.version {
            anyhow::bail!(
                "lockfile version for `{}` is {}, but registry supplies {}",
                installed.name,
                installed.version,
                recipe.document.metadata.version
            );
        }
        checked.push(installed.name.clone());
    }
    let response = serde_json::json!({"valid": true, "recipes": checked});
    if cli.json {
        emit_json(&response)?;
    } else if !cli.quiet {
        println!(
            "{} lockfile and {} recipes are valid",
            style("valid").green().bold(),
            checked.len()
        );
    }
    Ok(())
}

fn validate_recipe(cli: &Cli, path: &Path) -> anyhow::Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let fallback_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or("recipe");
    let recipe = RecipeDocument::from_yaml(fallback_name, &contents)?;
    let root = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("recipe path has no parent"))?;
    for file in &recipe.files {
        if !root.join(&file.source).is_file() {
            anyhow::bail!(
                "recipe `{}` references missing template `{}`",
                recipe.metadata.name,
                file.source
            );
        }
    }
    let response = serde_json::json!({"valid": true, "name": recipe.metadata.name, "version": recipe.metadata.version});
    if cli.json {
        emit_json(&response)?;
    } else if !cli.quiet {
        println!(
            "{} recipe `{}` is valid",
            style("valid").green().bold(),
            recipe.metadata.name
        );
    }
    Ok(())
}

fn execute_preview(
    cli: &Cli,
    root: &Path,
    preview: GenerationPreview,
    yes: bool,
    run_commands: bool,
) -> anyhow::Result<()> {
    emit_plan(cli, &preview.plan)?;
    if !confirm(cli, &preview.plan, yes)? {
        return Ok(());
    }
    commit(root, &preview.plan)?;
    let commands = validation_commands(&preview.resolution);
    if run_commands {
        if !cli.json {
            println!(
                "{} running declared validation commands",
                style("run").cyan().bold()
            );
        }
        run_safe_validation_commands(&preview.resolution, root)?;
    } else if !commands.is_empty() && !cli.quiet && !cli.json {
        println!("{} validation commands were not run. Re-run with --run-commands to execute the declared argv commands.", style("note:").yellow().bold());
        for (recipe, command) in &commands {
            println!("  {recipe}: {command}");
        }
    }
    if cli.json {
        emit_json(&serde_json::json!({
            "root": root,
            "recipes": preview.resolution.names(),
            "plan": preview.plan,
            "commands_run": run_commands,
        }))?;
    } else if !cli.quiet {
        println!(
            "{} generated {}",
            style("done").green().bold(),
            root.display()
        );
    }
    Ok(())
}

fn emit_plan(cli: &Cli, plan: &FilePlan) -> anyhow::Result<()> {
    if cli.json || cli.quiet {
        return Ok(());
    }
    println!("{}", style("preview").cyan().bold());
    for change in &plan.changes {
        let label = match change.kind {
            ChangeKind::Create => style("create").green(),
            ChangeKind::Update => style("update").yellow(),
            ChangeKind::Delete => style("delete").red(),
            ChangeKind::Unchanged => style("same").dim(),
            ChangeKind::Skip => style("skip").dim(),
        };
        println!(
            "  {:<8} {}  ({})",
            label,
            change.path.display(),
            change.recipe
        );
    }
    Ok(())
}

fn confirm(cli: &Cli, plan: &FilePlan, approved: bool) -> anyhow::Result<bool> {
    if !plan.has_changes() {
        return Ok(true);
    }
    if approved {
        return Ok(true);
    }
    if cli.json {
        anyhow::bail!("changes require --yes in --json mode");
    }
    Ok(Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Apply this preview?")
        .default(false)
        .interact()?)
}

fn emit_json<T: Serialize>(data: &T) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string(&JsonEnvelope { ok: true, data })?
    );
    Ok(())
}

fn status(available: bool) -> console::StyledObject<&'static str> {
    if available {
        style("available").green()
    } else {
        style("missing").red()
    }
}

fn capabilities() -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    if command_available("node") {
        values.insert("node-runtime".to_owned());
    }
    if command_available("npm") {
        values.insert("npm".to_owned());
    }
    values
}

fn command_available(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--version")
        .output()
        .is_ok()
}

#[derive(Debug, Clone)]
struct Selection {
    framework: String,
    architecture: String,
    database: String,
    zod: bool,
    pino: bool,
    vitest: bool,
    biome: bool,
    hooks: bool,
    github_actions: bool,
}

impl Selection {
    fn from_args(args: &InitArgs) -> anyhow::Result<Self> {
        let selection = Self {
            framework: args.framework.clone(),
            architecture: args.architecture.clone(),
            database: args.database.clone(),
            zod: args.zod,
            pino: args.pino,
            vitest: args.vitest,
            biome: args.biome,
            hooks: args.hooks,
            github_actions: args.github_actions,
        };
        selection.validate()?;
        Ok(selection)
    }

    fn validate(&self) -> anyhow::Result<()> {
        for (label, value, allowed) in [
            (
                "framework",
                self.framework.as_str(),
                &["express", "hono", "none"][..],
            ),
            (
                "architecture",
                self.architecture.as_str(),
                &["minimal", "layered", "clean"][..],
            ),
            ("database", self.database.as_str(), &["sqlite", "none"][..]),
        ] {
            if !allowed.contains(&value) {
                anyhow::bail!(
                    "invalid {label} `{value}`; expected one of {}",
                    allowed.join(", ")
                );
            }
        }
        Ok(())
    }

    fn recipes(&self) -> Vec<String> {
        let mut recipes = vec![
            "node".to_owned(),
            "typescript".to_owned(),
            format!("architecture-{}", self.architecture),
        ];
        if self.framework != "none" {
            recipes.push(self.framework.clone());
        }
        if self.database == "sqlite" {
            recipes.extend(["sqlite-libsql".to_owned(), "drizzle".to_owned()]);
        }
        if self.zod || (self.architecture == "clean" && self.database == "sqlite") {
            recipes.push("zod".to_owned());
        }
        if self.pino {
            recipes.push("pino".to_owned());
        }
        if self.vitest {
            recipes.push("vitest".to_owned());
        }
        if self.biome {
            recipes.push("biome".to_owned());
        }
        if self.hooks {
            recipes.push("husky-lint-staged".to_owned());
        }
        if self.github_actions {
            recipes.push("github-actions".to_owned());
        }
        recipes
    }

    fn variables(&self, project_name: &str) -> BTreeMap<String, Value> {
        let slug = project_name.replace('_', "-");
        let pascal = slug
            .split('-')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<String>();
        BTreeMap::from([
            (
                "project_name".to_owned(),
                Value::String(project_name.to_owned()),
            ),
            ("project_slug".to_owned(), Value::String(slug)),
            ("project_pascal".to_owned(), Value::String(pascal)),
            (
                "framework".to_owned(),
                Value::String(self.framework.clone()),
            ),
            (
                "architecture".to_owned(),
                Value::String(self.architecture.clone()),
            ),
            ("database".to_owned(), Value::String(self.database.clone())),
            (
                "zod_enabled".to_owned(),
                Value::Bool(
                    self.zod || (self.architecture == "clean" && self.database == "sqlite"),
                ),
            ),
            ("pino_enabled".to_owned(), Value::Bool(self.pino)),
            ("vitest_enabled".to_owned(), Value::Bool(self.vitest)),
            ("biome_enabled".to_owned(), Value::Bool(self.biome)),
        ])
    }
}

fn interactive_selection(args: &InitArgs) -> anyhow::Result<Selection> {
    let theme = ColorfulTheme::default();
    let framework = ["express", "hono", "none"][Select::with_theme(&theme)
        .with_prompt("HTTP framework")
        .items(&["Express", "Hono", "None"])
        .default(0)
        .interact()?]
    .to_owned();
    let architecture = ["minimal", "layered", "clean"][Select::with_theme(&theme)
        .with_prompt("Architecture")
        .items(&["Minimal", "Layered", "Clean"])
        .default(0)
        .interact()?]
    .to_owned();
    let database = ["none", "sqlite"][Select::with_theme(&theme)
        .with_prompt("Database")
        .items(&["None", "SQLite + LibSQL + Drizzle"])
        .default(0)
        .interact()?]
    .to_owned();
    let zod = Confirm::with_theme(&theme)
        .with_prompt("Enable Zod validation?")
        .default(args.zod)
        .interact()?;
    let pino = Confirm::with_theme(&theme)
        .with_prompt("Enable Pino logging?")
        .default(args.pino)
        .interact()?;
    let vitest = Confirm::with_theme(&theme)
        .with_prompt("Enable Vitest?")
        .default(args.vitest)
        .interact()?;
    let biome = Confirm::with_theme(&theme)
        .with_prompt("Enable Biome?")
        .default(args.biome)
        .interact()?;
    let hooks = Confirm::with_theme(&theme)
        .with_prompt("Enable Husky + lint-staged?")
        .default(args.hooks)
        .interact()?;
    let github_actions = Confirm::with_theme(&theme)
        .with_prompt("Enable GitHub Actions?")
        .default(args.github_actions)
        .interact()?;
    Ok(Selection {
        framework,
        architecture,
        database,
        zod,
        pino,
        vitest,
        biome,
        hooks,
        github_actions,
    })
}
