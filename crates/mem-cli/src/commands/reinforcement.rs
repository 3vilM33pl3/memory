use anyhow::Result;

use crate::commands::{
    api::ApiClient,
    runtime::{ReviewArgs, ReviewCommand, ScoresArgs, ValidateMemoryArgs},
};

pub(super) async fn handle_scores(args: ScoresArgs, api: &ApiClient) -> Result<()> {
    let response = api
        .memory_scores(&args.project, args.needs_review, args.limit)
        .await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }
    if response.scores.is_empty() {
        println!("No reinforcement scores recorded for {}.", response.project);
        return Ok(());
    }
    println!(
        "{:<38} {:>10} {:>7} {:>6} {:>6} {:>10}  SUMMARY",
        "MEMORY", "ACTIVATION", "ACCESS", "CITED", "VOLAT", "VALIDATED"
    );
    for score in &response.scores {
        let validated = score
            .validated_at
            .map(|at| at.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "-".to_string());
        let mut summary: String = score.summary.chars().take(60).collect();
        if score.needs_review {
            summary = format!("[NEEDS REVIEW] {summary}");
        }
        println!(
            "{:<38} {:>10.2} {:>7} {:>6} {:>6.2} {:>10}  {}",
            score.memory_id,
            score.activation,
            score.access_count,
            score.citation_count,
            score.volatility,
            validated,
            summary
        );
    }
    Ok(())
}

pub(super) async fn handle_validate(args: ValidateMemoryArgs, api: &ApiClient) -> Result<()> {
    let dry_run = if args.dry_run {
        Some(true)
    } else if args.execute {
        Some(false)
    } else {
        None
    };
    let run = api.validate_memory(args.id, dry_run).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&run)?);
        return Ok(());
    }
    print_run(&run);
    Ok(())
}

pub(super) async fn handle_review(args: ReviewArgs, api: &ApiClient) -> Result<()> {
    match args.command {
        ReviewCommand::List(args) => {
            let response = api
                .validation_runs(&args.project, !args.all, args.limit)
                .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
                return Ok(());
            }
            if response.runs.is_empty() {
                println!(
                    "No {}validation runs for {}.",
                    if args.all { "" } else { "pending " },
                    response.project
                );
                return Ok(());
            }
            for run in &response.runs {
                print_run(run);
                println!();
            }
        }
        ReviewCommand::Apply(args) => {
            let response = api.review_validation_run(args.id, "apply").await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                match response.new_memory_id {
                    Some(new_id) => println!(
                        "Applied correction from run {} as new memory version {new_id}.",
                        response.run_id
                    ),
                    None => println!("Applied review for run {}.", response.run_id),
                }
            }
        }
        ReviewCommand::Reject(args) => {
            let response = api.review_validation_run(args.id, "reject").await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                println!("Rejected correction from run {}.", response.run_id);
            }
        }
    }
    Ok(())
}

fn print_run(run: &mem_api::ValidationRunInfo) {
    println!(
        "run {} [{}] memory {} — {}",
        run.id,
        run.status,
        run.memory_id,
        run.summary.chars().take(70).collect::<String>()
    );
    println!(
        "  trigger={} verdict={} confidence={} action={}{}{}",
        run.trigger,
        run.verdict.as_deref().unwrap_or("-"),
        run.confidence
            .map(|value| format!("{value:.2}"))
            .unwrap_or_else(|| "-".to_string()),
        run.action.as_deref().unwrap_or("-"),
        if run.dry_run { " (dry run)" } else { "" },
        run.review_status
            .as_deref()
            .map(|status| format!(" review={status}"))
            .unwrap_or_default(),
    );
    if !run.reasons.is_empty() {
        println!("  reasons: {}", run.reasons.join("; "));
    }
    if let Some(summary) = &run.proposed_summary {
        println!("  proposed summary: {summary}");
    }
    if let Some(text) = &run.proposed_text {
        println!(
            "  proposed text: {}",
            text.chars().take(200).collect::<String>()
        );
    }
    if let Some(error) = &run.error {
        println!("  error: {error}");
    }
}
