use std::io::{IsTerminal, Write};

use anyhow::Result;
use mem_api::AppConfig;
use reqwest::Client;

use crate::commands::runtime::{QueryArgs, RememberArgs, ResumeArgs, TourArgs};

/// Guided first-run tour: seed the showcase corpus, then actually run the
/// three commands that cover day-to-day use — remember, query, resume — with
/// their real output. The honest message: you only need three commands.
pub(super) async fn handle(
    args: TourArgs,
    client: Client,
    config: AppConfig,
    cli_writer_id: Option<String>,
) -> Result<()> {
    let project = args.project.clone();

    println!("Welcome to Memory Layer. Three commands cover day-to-day use:");
    println!("  remember  - capture completed work into durable memory");
    println!("  query     - ask a project-specific question against that memory");
    println!("  resume    - get a briefing to pick up where you left off");
    println!();
    println!("This tour runs all three for real against the '{project}' project.");

    print!("\nSeeding the showcase corpus... ");
    std::io::stdout().flush().ok();
    let count =
        super::demo::seed_demo_corpus(&project, &client, &config, cli_writer_id.as_deref()).await?;
    println!("{count} memories loaded.");

    pause(&format!(
        "Step 1/3 — remember. We record that you took this tour:\n  memory remember --project {project} --title \"Completed the Memory Layer tour\" ..."
    ));
    crate::commands::remember::handle(
        RememberArgs {
            project: Some(project.clone()),
            title: Some("Completed the Memory Layer tour".to_string()),
            memory_type: Some("project".to_string()),
            prompt: Some("Run the guided Memory Layer tour.".to_string()),
            summary: Some(
                "Walked through remember, query, and resume against the demo corpus.".to_string(),
            ),
            notes: vec![
                "The three core commands are remember, query, and resume; everything else is optional depth.".to_string(),
            ],
            files_changed: Vec::new(),
            tests_passed: Vec::new(),
            tests_failed: Vec::new(),
            command_output_file: None,
            auto_files: false,
            dry_run: false,
        },
        client.clone(),
        config.clone(),
        cli_writer_id.clone(),
    )
    .await?;

    pause(&format!(
        "Step 2/3 — query. Ask a question only project memory can answer:\n  memory query --project {project} --question \"How does reinforcement work?\""
    ));
    crate::commands::query::handle(
        QueryArgs {
            project: project.clone(),
            question: "How does reinforcement work?".to_string(),
            types: Vec::new(),
            tags: Vec::new(),
            limit: 8,
            min_confidence: None,
            include_stale: false,
            history: false,
            json: false,
        },
        client.clone(),
        config.clone(),
    )
    .await?;

    pause(&format!(
        "Step 3/3 — resume. Get a re-entry briefing for the project:\n  memory resume --project {project}"
    ));
    crate::commands::resume::handle(
        ResumeArgs {
            project: Some(project.clone()),
            json: false,
            include_llm_summary: true,
        },
        client,
        config,
    )
    .await?;

    println!();
    println!("That is the whole daily loop. Where to go next:");
    println!("  memory tui   # browse every memory, its provenance, and the graph");
    println!("  https://www.memory-layer.dev/docs/quickstart");
    Ok(())
}

/// Print the step banner; wait for Enter when interactive so each step's
/// output can be read, and continue without blocking when piped/scripted.
fn pause(banner: &str) {
    println!("\n{banner}");
    if std::io::stdin().is_terminal() {
        print!("Press Enter to run it... ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok();
    } else {
        println!("(non-interactive: continuing)");
    }
}
