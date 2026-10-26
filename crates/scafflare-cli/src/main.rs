use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use scafflare_core::plan::{ChangeKind, FilePlan};
use scafflare_core::recipe::{evaluate_condition, Prompt, PromptKind, RecipeDocument};
use scafflare_core::registry::{CompositeRegistry, RecipeRegistry};
use scafflare_core::state::load_state;
use scafflare_core::{
    commit, preview_add_with, preview_init, preview_remove, preview_upgrade, project_status,
    run_safe_validation_commands, validation_commands, Conflict, ConflictMode, FileState,
    GenerationPreview, GenerationRequest, RecipeState, UpgradeOptions,
};
use serde_json::Value;

#[derive(Debug, Parser)]
#[command(
    name = "scafflare",
    version,
    about = "Composable, language-agnostic backend scaffolding"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(long, global = true, value_name = "DIR")]
    recipe_root: Vec<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true, short = 'q')]
    quiet: bool,
}
#[derive(Debug, Subcommand)]
enum Commands {
    Init(InitArgs),
    Add(ModifyArgs),
    Remove(RemoveArgs),
    /// Bring installed recipes to the bundled versions with a three-way merge.
    Upgrade(UpgradeArgs),
    /// Show outdated recipes, modified files, and unresolved conflicts.
    Status(StatusArgs),
    List,
    Doctor(PathArgs),
    Validate(PathArgs),
    Recipe {
        #[command(subcommand)]
        command: RecipeCommands,
    },
}
#[derive(Debug, Subcommand)]
enum RecipeCommands {
    Validate { path: PathBuf },
}
#[derive(Debug, Args)]
struct InitArgs {
    project_name: String,
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    #[arg(long)]
    non_interactive: bool,
    #[arg(long, value_name = "RECIPE")]
    recipe: Vec<String>,
    #[arg(long = "set", value_name = "KEY=VALUE")]
    set: Vec<String>,
    #[arg(long, short = 'y')]
    yes: bool,
    #[arg(long)]
    run_commands: bool,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ConflictsArg {
    /// Write nothing and report every conflict (default).
    Abort,
    /// Write git-style markers, or `<file>.scafflare-new` for JSON and unmanaged files.
    Markers,
}

impl From<ConflictsArg> for ConflictMode {
    fn from(value: ConflictsArg) -> Self {
        match value {
            ConflictsArg::Abort => ConflictMode::Abort,
            ConflictsArg::Markers => ConflictMode::Markers,
        }
    }
}

#[derive(Debug, Args)]
struct UpgradeArgs {
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    /// Show what would change without writing anything.
    #[arg(long)]
    dry_run: bool,
    /// Print a unified diff of every file that would be written.
    #[arg(long)]
    diff: bool,
    #[arg(long, short = 'y')]
    yes: bool,
    #[arg(long, value_enum, default_value_t = ConflictsArg::Abort)]
    conflicts: ConflictsArg,
    #[arg(long = "set", value_name = "KEY=VALUE")]
    set: Vec<String>,
    #[arg(long)]
    run_commands: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    /// Exit with status 2 when the project needs attention.
    #[arg(long)]
    exit_code: bool,
}

#[derive(Debug, Args)]
struct ModifyArgs {
    #[arg(required=true, num_args=1..)]
    recipes: Vec<String>,
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    #[arg(long, short = 'y')]
    yes: bool,
    #[arg(long)]
    run_commands: bool,
    #[arg(long, value_enum, default_value_t = ConflictsArg::Abort)]
    conflicts: ConflictsArg,
}
#[derive(Debug, Args)]
struct RemoveArgs {
    recipe: String,
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    #[arg(long, short = 'y')]
    yes: bool,
}
#[derive(Debug, Args)]
struct PathArgs {
    #[arg(long, default_value = ".")]
    directory: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(&cli) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if cli.json {
                println!(
                    "{{\"ok\":false,\"error\":{}}}",
                    serde_json::to_string(&error.to_string()).unwrap()
                );
            } else {
                eprintln!("{} {error}", style("error:").red().bold());
            }
            ExitCode::from(1)
        }
    }
}
/// Exit status: 0 success, 1 error, 2 conflicts (or `status --exit-code` findings).
fn execute(cli: &Cli) -> anyhow::Result<u8> {
    match &cli.command {
        Commands::Init(a) => init(cli, a),
        Commands::Add(a) => add(cli, a),
        Commands::Upgrade(a) => upgrade(cli, a),
        Commands::Status(a) => status(cli, a),
        Commands::Remove(a) => remove(cli, a).map(|()| 0),
        Commands::List => list(cli).map(|()| 0),
        Commands::Doctor(a) => doctor(cli, a).map(|()| 0),
        Commands::Validate(a) => validate(cli, a).map(|()| 0),
        Commands::Recipe {
            command: RecipeCommands::Validate { path },
        } => validate_recipe(path).map(|()| 0),
    }
}
fn registry(cli: &Cli) -> CompositeRegistry {
    CompositeRegistry::new(cli.recipe_root.clone())
}
fn init(cli: &Cli, args: &InitArgs) -> anyhow::Result<u8> {
    if cli.json && !args.non_interactive {
        anyhow::bail!("--json requires --non-interactive");
    }
    let registry = registry(cli);
    let (recipes, variables) = selection(&registry, args)?;
    let request = GenerationRequest {
        project_name: args.project_name.clone(),
        recipes,
        variables,
        capabilities: capabilities(&registry)?,
    };
    let root = args.directory.join(&args.project_name);
    let preview = preview_init(&root, &registry, request)?;
    apply_preview(cli, &root, preview, args.yes, args.run_commands)
}
fn add(cli: &Cli, args: &ModifyArgs) -> anyhow::Result<u8> {
    let registry = registry(cli);
    let preview = preview_add_with(
        &args.directory,
        &registry,
        args.recipes.clone(),
        capabilities(&registry)?,
        args.conflicts.into(),
    )?;
    apply_preview(cli, &args.directory, preview, args.yes, args.run_commands)
}
fn remove(cli: &Cli, args: &RemoveArgs) -> anyhow::Result<()> {
    let registry = registry(cli);
    let (plan, state) = preview_remove(
        &args.directory,
        &registry,
        &args.recipe,
        capabilities(&registry)?,
    )?;
    show_plan(cli, &plan);
    if approve(cli, &plan, args.yes)? {
        commit(&args.directory, &plan)?;
        if cli.json {
            println!(
                "{}",
                serde_json::json!({"ok":true,"plan":plan,"state":state})
            );
        } else if !cli.quiet {
            println!("{} removed `{}`", style("done").green().bold(), args.recipe);
        }
    }
    Ok(())
}
fn list(cli: &Cli) -> anyhow::Result<()> {
    let r = registry(cli);
    let entries:Vec<_>=r.list()?.into_iter().map(|name| { let d=r.get(&name).unwrap().document; serde_json::json!({"name":d.metadata.name,"description":d.metadata.description,"entrypoint":d.metadata.wizard_entrypoint})}).collect();
    if cli.json {
        println!("{}", serde_json::to_string(&entries)?);
    } else {
        for value in entries {
            println!(
                "{}  {}",
                style(value["name"].as_str().unwrap()).green(),
                value["description"].as_str().unwrap()
            );
        }
    }
    Ok(())
}
fn doctor(cli: &Cli, args: &PathArgs) -> anyhow::Result<()> {
    let r = registry(cli);
    let probes = probes(&r)?;
    let available = capabilities(&r)?;
    let state = load_state(&args.directory).ok();
    if cli.json {
        println!(
            "{}",
            serde_json::json!({"capabilities":available,"probes":probes,"managed_project":state.is_some()})
        );
    } else {
        for (capability, program) in probes {
            println!(
                "{capability} ({program}): {}",
                if available.contains(&capability) {
                    "available"
                } else {
                    "missing"
                }
            );
        }
    }
    Ok(())
}
fn validate(cli: &Cli, args: &PathArgs) -> anyhow::Result<()> {
    let r = registry(cli);
    let state = load_state(&args.directory)?;
    for installed in &state.recipes {
        let available = r.get(&installed.name)?.document.metadata.version;
        if available != installed.version {
            anyhow::bail!(
                "recipe version mismatch for {} (installed {}, available {}); run `scafflare upgrade`",
                installed.name,
                installed.version,
                available
            );
        }
    }
    if !cli.quiet {
        println!("{} lockfile is valid", style("valid").green());
    }
    Ok(())
}
fn validate_recipe(path: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(path)?;
    let name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .unwrap_or("recipe");
    let doc = RecipeDocument::from_yaml(name, &raw)?;
    let root = path.parent().unwrap();
    for file in &doc.files {
        if !root.join(&file.source).is_file() {
            anyhow::bail!("missing template {}", file.source);
        }
    }
    println!("valid recipe `{}`", doc.metadata.name);
    Ok(())
}

fn selection<R: RecipeRegistry>(
    r: &R,
    args: &InitArgs,
) -> anyhow::Result<(Vec<String>, BTreeMap<String, Value>)> {
    let mut recipes = if args.recipe.is_empty() {
        let entries: Vec<_> = r
            .list()?
            .into_iter()
            .filter(|name| {
                r.get(name)
                    .map(|x| x.document.metadata.wizard_entrypoint)
                    .unwrap_or(false)
            })
            .collect();
        if entries.len() != 1 {
            anyhow::bail!(
                "pass --recipe because registry has {} entrypoints",
                entries.len()
            );
        }
        entries
    } else {
        args.recipe.clone()
    };
    let mut vars = project_vars(&args.project_name);
    let supplied = parse_sets(&args.set)?;
    let mut prompts = Vec::new();
    for name in &recipes {
        let doc = r.get(name)?.document;
        for (key, value) in doc.variables {
            vars.entry(key).or_insert(value);
        }
        prompts.extend(doc.prompts);
    }
    prompts.sort_by_key(|p| p.order);
    for prompt in prompts {
        if !evaluate_condition(prompt.when.as_deref(), &vars)? {
            continue;
        }
        let value = if let Some(v) = supplied.get(&prompt.key) {
            v.clone()
        } else if args.non_interactive {
            prompt
                .default
                .clone()
                .ok_or_else(|| anyhow::anyhow!("prompt {} needs --set", prompt.key))?
        } else {
            ask(&prompt)?
        };
        apply_prompt(&prompt, value, &mut recipes, &mut vars)?;
    }
    for (k, v) in supplied {
        vars.insert(k, v);
    }
    recipes.sort();
    recipes.dedup();
    Ok((recipes, vars))
}
fn ask(prompt: &Prompt) -> anyhow::Result<Value> {
    let t = ColorfulTheme::default();
    match prompt.kind {
        PromptKind::Select => {
            let labels: Vec<_> = prompt.options.iter().map(|x| x.label.as_str()).collect();
            let default = prompt
                .default
                .as_ref()
                .and_then(|v| prompt.options.iter().position(|x| x.value == *v))
                .unwrap_or(0);
            let selected = Select::with_theme(&t)
                .with_prompt(&prompt.message)
                .items(&labels)
                .default(default)
                .interact()?;
            Ok(prompt.options[selected].value.clone())
        }
        PromptKind::Confirm => Ok(Value::Bool(
            Confirm::with_theme(&t)
                .with_prompt(&prompt.message)
                .default(
                    prompt
                        .default
                        .as_ref()
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )
                .interact()?,
        )),
        PromptKind::Text => {
            let mut field = Input::<String>::with_theme(&t).with_prompt(&prompt.message);
            if let Some(default) = prompt.default.as_ref().and_then(Value::as_str) {
                field = field.default(default.to_owned());
            }
            let value = field.interact_text()?;
            if prompt.required && value.trim().is_empty() {
                anyhow::bail!("prompt {} is required", prompt.key);
            }
            Ok(Value::String(value))
        }
    }
}
fn apply_prompt(
    prompt: &Prompt,
    value: Value,
    recipes: &mut Vec<String>,
    vars: &mut BTreeMap<String, Value>,
) -> anyhow::Result<()> {
    match prompt.kind {
        PromptKind::Select => {
            let opt = prompt
                .options
                .iter()
                .find(|x| x.value == value)
                .ok_or_else(|| anyhow::anyhow!("invalid value for {}", prompt.key))?;
            recipes.extend(opt.recipes.iter().cloned());
            for (k, v) in &opt.variables {
                vars.insert(k.clone(), v.clone());
            }
        }
        PromptKind::Confirm => {
            if value
                .as_bool()
                .ok_or_else(|| anyhow::anyhow!("{} must be bool", prompt.key))?
            {
                recipes.extend(prompt.recipes.iter().cloned());
            }
        }
        PromptKind::Text => {}
    }
    vars.insert(prompt.key.clone(), value);
    Ok(())
}
fn parse_sets(items: &[String]) -> anyhow::Result<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    for item in items {
        let (key, val) = item
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--set expects KEY=VALUE"))?;
        out.insert(
            key.to_owned(),
            serde_json::from_str(val).unwrap_or_else(|_| Value::String(val.to_owned())),
        );
    }
    Ok(out)
}
fn project_vars(name: &str) -> BTreeMap<String, Value> {
    let slug = name.replace('_', "-");
    let pascal = slug
        .split('-')
        .filter(|x| !x.is_empty())
        .map(|x| {
            let mut c = x.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect();
    BTreeMap::from([
        ("project_name".into(), Value::String(name.into())),
        ("project_slug".into(), Value::String(slug)),
        ("project_pascal".into(), Value::String(pascal)),
    ])
}
fn probes<R: RecipeRegistry>(r: &R) -> anyhow::Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for name in r.list()? {
        for probe in r.get(&name)?.document.metadata.capability_probes {
            match result.get(&probe.capability) {
                Some(old) if old != &probe.program => {
                    anyhow::bail!("conflicting probe {}", probe.capability)
                }
                _ => {
                    result.insert(probe.capability, probe.program);
                }
            }
        }
    }
    Ok(result)
}
fn capabilities<R: RecipeRegistry>(r: &R) -> anyhow::Result<BTreeSet<String>> {
    Ok(probes(r)?
        .into_iter()
        .filter_map(|(capability, program)| {
            std::process::Command::new(program)
                .arg("--version")
                .output()
                .is_ok()
                .then_some(capability)
        })
        .collect())
}
fn apply_preview(
    cli: &Cli,
    root: &Path,
    preview: GenerationPreview,
    yes: bool,
    run: bool,
) -> anyhow::Result<u8> {
    report_conflicts(cli, &preview.conflicts);
    let aborted = preview.has_conflicts() && !preview.plan.has_changes();
    if aborted {
        if cli.json {
            println!(
                "{}",
                serde_json::json!({"ok":false,"conflicts":preview.conflicts,"outcomes":preview.outcomes})
            );
        }
        return Ok(2);
    }
    show_plan(cli, &preview.plan);
    if !approve(cli, &preview.plan, yes)? {
        return Ok(0);
    }
    commit(root, &preview.plan)?;
    if run {
        run_safe_validation_commands(&preview.resolution, root)?;
    } else if !cli.quiet && !cli.json {
        for (r, c) in validation_commands(&preview.resolution) {
            println!("note: {r}: {c}");
        }
    }
    if cli.json {
        println!(
            "{}",
            serde_json::json!({"ok":true,"recipes":preview.resolution.names(),"plan":preview.plan,"outcomes":preview.outcomes,"conflicts":preview.conflicts})
        );
    } else if !cli.quiet {
        println!(
            "{} generated {}",
            style("done").green().bold(),
            root.display()
        );
    }
    Ok(if preview.has_conflicts() { 2 } else { 0 })
}

fn report_conflicts(cli: &Cli, conflicts: &[Conflict]) {
    if conflicts.is_empty() || cli.json {
        return;
    }
    eprintln!(
        "{} {} conflict(s):",
        style("conflict").red().bold(),
        conflicts.len()
    );
    for conflict in conflicts {
        let detail = if conflict.json_paths.is_empty() {
            String::new()
        } else {
            format!(" at {}", conflict.json_paths.join(", "))
        };
        eprintln!(
            "  {} ({:?}{detail}) from recipe {}",
            conflict.path.display(),
            conflict.kind,
            conflict.recipe
        );
    }
}

fn upgrade(cli: &Cli, args: &UpgradeArgs) -> anyhow::Result<u8> {
    let registry = registry(cli);
    let options = UpgradeOptions {
        conflict_mode: args.conflicts.into(),
        variables: parse_sets(&args.set)?,
    };
    let up = preview_upgrade(
        &args.directory,
        &registry,
        capabilities(&registry)?,
        options,
    )?;
    let preview = &up.preview;
    report_conflicts(cli, &preview.conflicts);
    let aborted = preview.has_conflicts() && !preview.plan.has_changes();
    let envelope = |ok: bool, dry_run: bool| {
        serde_json::json!({
            "ok": ok,
            "dry_run": dry_run,
            "from": up.from,
            "to": up.to,
            "plan": preview.plan,
            "outcomes": preview.outcomes,
            "conflicts": preview.conflicts,
        })
    };
    if aborted {
        if cli.json {
            println!("{}", envelope(false, args.dry_run));
        }
        return Ok(2);
    }
    if args.diff && !cli.json {
        for change in &preview.plan.changes {
            if let Some(diff) = scafflare_core::plan::unified_diff(&args.directory, change) {
                if !scafflare_core::blobs::is_blob_path(&change.path) {
                    print!("{diff}");
                }
            }
        }
    }
    let code = if preview.has_conflicts() { 2 } else { 0 };
    if args.dry_run {
        if cli.json {
            println!("{}", envelope(!preview.has_conflicts(), true));
        } else if !cli.quiet {
            show_plan(cli, &preview.plan);
            println!("{} dry run: nothing was written", style("note").cyan());
        }
        return Ok(code);
    }
    if !preview.plan.has_changes() {
        if cli.json {
            println!("{}", envelope(true, false));
        } else if !cli.quiet {
            println!(
                "{} project is already up to date",
                style("done").green().bold()
            );
        }
        return Ok(code);
    }
    show_plan(cli, &preview.plan);
    if !approve(cli, &preview.plan, args.yes)? {
        return Ok(0);
    }
    commit(&args.directory, &preview.plan)?;
    if args.run_commands {
        run_safe_validation_commands(&preview.resolution, &args.directory)?;
    }
    if cli.json {
        println!("{}", envelope(!preview.has_conflicts(), false));
    } else if !cli.quiet {
        for (from, to) in up.from.iter().filter_map(|from| {
            up.to
                .iter()
                .find(|to| to.name == from.name && to.version != from.version)
                .map(|to| (from, to))
        }) {
            println!("upgraded {} {} -> {}", from.name, from.version, to.version);
        }
        println!(
            "{} upgraded {}",
            style("done").green().bold(),
            args.directory.display()
        );
    }
    Ok(code)
}

fn status(cli: &Cli, args: &StatusArgs) -> anyhow::Result<u8> {
    let registry = registry(cli);
    let status = project_status(&args.directory, &registry)?;
    if cli.json {
        println!("{}", serde_json::to_string(&status)?);
    } else if !cli.quiet {
        println!("project: {}", status.project_name);
        for recipe in &status.recipes {
            let label = match recipe.state {
                RecipeState::Current => "current".to_owned(),
                RecipeState::Outdated => format!(
                    "outdated (available {})",
                    recipe
                        .available
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default()
                ),
                RecipeState::NewerThanRegistry => "newer than this scafflare".to_owned(),
                RecipeState::MissingFromRegistry => "missing from registry".to_owned(),
            };
            println!("  {} {}: {label}", recipe.name, recipe.installed);
        }
        for file in &status.files {
            match file.state {
                FileState::Clean => {}
                FileState::Modified => println!("  modified   {}", file.path),
                FileState::Missing => println!("  missing    {}", file.path),
                FileState::Conflicted => println!("  conflicted {}", file.path),
            }
        }
        if status.legacy_files > 0 {
            println!(
                "note: {} file(s) have no merge base (lockfile v1); run `scafflare upgrade` to record them",
                status.legacy_files
            );
        }
        if !status.needs_attention() {
            println!(
                "{} project is clean and up to date",
                style("ok").green().bold()
            );
        }
    }
    Ok(if args.exit_code && status.needs_attention() {
        2
    } else {
        0
    })
}

fn show_plan(cli: &Cli, plan: &FilePlan) {
    if cli.quiet || cli.json {
        return;
    }
    let snapshots = plan
        .changes
        .iter()
        .filter(|change| scafflare_core::blobs::is_blob_path(&change.path))
        .count();
    for change in plan
        .changes
        .iter()
        .filter(|change| !scafflare_core::blobs::is_blob_path(&change.path))
    {
        let tag = match change.kind {
            ChangeKind::Create => "create",
            ChangeKind::Update => "update",
            ChangeKind::Delete => "delete",
            ChangeKind::Unchanged => "same",
            ChangeKind::Skip => "skip",
        };
        println!("{tag:8} {} ({})", change.path.display(), change.recipe);
    }
    if snapshots > 0 {
        println!(
            "{:8} {snapshots} merge base snapshot(s) in .scafflare/base",
            "state"
        );
    }
}
fn approve(cli: &Cli, plan: &FilePlan, yes: bool) -> anyhow::Result<bool> {
    if !plan.has_changes() || yes {
        return Ok(true);
    }
    if cli.json {
        anyhow::bail!("--yes required with --json");
    }
    Ok(Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Apply this preview?")
        .default(false)
        .interact()?)
}
