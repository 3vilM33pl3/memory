use anyhow::{Context, Result};
use uuid::Uuid;

use crate::commands::{
    api::ApiClient,
    runtime::{ProposalsArgs, ProposalsCommand},
};

pub(super) async fn handle(args: ProposalsArgs, api: &ApiClient) -> Result<()> {
    match args.command {
        ProposalsCommand::List(args) => {
            let mut response = api.replacement_proposals(&args.project).await?;
            if let Some(limit) = args.limit {
                response.proposals.truncate(limit);
            }
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                print_replacement_proposals(&response);
            }
        }
        ProposalsCommand::Show(args) => {
            let response = api.replacement_proposals(&args.project).await?;
            let proposal = find_replacement_proposal(response.proposals, args.id)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&proposal)?);
            } else {
                print_replacement_proposal_detail(&proposal);
            }
        }
        ProposalsCommand::Approve(args) => {
            let response = api
                .approve_replacement_proposal(&args.project, args.id)
                .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                println!(
                    "Approved replacement proposal {}: {} -> {}",
                    response.proposal_id, response.target_summary, response.candidate_summary
                );
            }
        }
        ProposalsCommand::Reject(args) => {
            let response = api
                .reject_replacement_proposal(&args.project, args.id)
                .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                println!(
                    "Rejected replacement proposal {} for {}.",
                    response.proposal_id, response.target_summary
                );
            }
        }
    }
    Ok(())
}

fn find_replacement_proposal(
    proposals: Vec<mem_api::ReplacementProposalRecord>,
    id: Uuid,
) -> Result<mem_api::ReplacementProposalRecord> {
    proposals
        .into_iter()
        .find(|proposal| proposal.id == id)
        .with_context(|| format!("pending replacement proposal {id} was not found"))
}

fn print_replacement_proposals(response: &mem_api::ReplacementProposalListResponse) {
    if response.proposals.is_empty() {
        println!(
            "No pending replacement proposals for `{}`.",
            response.project
        );
        return;
    }
    println!(
        "Pending replacement proposals for `{}`: {}",
        response.project,
        response.proposals.len()
    );
    for (index, proposal) in response.proposals.iter().enumerate() {
        println!(
            "\n{}. {} [{} score {}]",
            index + 1,
            proposal.candidate_summary,
            proposal.candidate_memory_type,
            proposal.score
        );
        println!("   id: {}", proposal.id);
        println!("   target: {}", proposal.target_summary);
        if !proposal.reasons.is_empty() {
            println!("   why: {}", proposal.reasons.join(", "));
        }
    }
}

fn print_replacement_proposal_detail(proposal: &mem_api::ReplacementProposalRecord) {
    println!("Proposal: {}", proposal.id);
    println!("Project: {}", proposal.project);
    println!("Target memory: {}", proposal.target_memory_id);
    println!("Target summary: {}", proposal.target_summary);
    println!("Candidate summary: {}", proposal.candidate_summary);
    println!(
        "Type / Score / Policy: {} / {} / {}",
        proposal.candidate_memory_type, proposal.score, proposal.policy
    );
    if !proposal.reasons.is_empty() {
        println!("Why proposed: {}", proposal.reasons.join(", "));
    }
    println!("Created: {}", proposal.created_at.to_rfc3339());
    println!("\nCandidate text:\n{}", proposal.candidate_canonical_text);
}
