use anyhow::{Context, Result};
use mem_api::{
    AppConfig, AuthMeResponse, AuthMembershipGrantRequest, AuthMembershipResponse,
    AuthPrincipalResponse, AuthServiceTokenCreateRequest, AuthServiceTokenResponse,
};
use reqwest::Client;

use crate::commands::{
    api::get_json,
    output::service_url,
    runtime::{AuthArgs, AuthCommand, AuthMembershipCommand, AuthTokenCommand},
};

pub(super) async fn handle(args: AuthArgs, client: Client, config: AppConfig) -> Result<()> {
    match args.command {
        AuthCommand::Whoami(args) => {
            let me: AuthMeResponse = get_json(
                client
                    .get(service_url(&config, "/v1/auth/me"))
                    .send()
                    .await?,
            )
            .await?;
            if args.json {
                print_json(&me)?;
            } else {
                println!("{} ({:?})", me.principal.display_name, me.principal.kind);
                println!("Principal: {}", me.principal.id);
                println!("Mode: {:?}", me.mode);
                println!(
                    "Global role: {}",
                    me.principal
                        .global_role
                        .map(|role| role.as_str())
                        .unwrap_or("none")
                );
                print_access(&me.principal);
            }
        }
        AuthCommand::Token(args) => match args.command {
            AuthTokenCommand::Create(args) => {
                let ttl = u64::try_from(args.ttl.as_secs())
                    .context("token ttl does not fit in seconds")?;
                let token: AuthServiceTokenResponse = get_json(
                    client
                        .post(service_url(&config, "/v1/auth/tokens"))
                        .json(&AuthServiceTokenCreateRequest {
                            name: args.name,
                            project: args.project,
                            role: args.role.into(),
                            expires_in_seconds: Some(ttl),
                        })
                        .send()
                        .await?,
                )
                .await?;
                if args.json {
                    print_json(&token)?;
                } else {
                    println!("Created service token {} ({})", token.name, token.id);
                    println!(
                        "Token: {}",
                        token
                            .token
                            .as_deref()
                            .context("service omitted token secret")?
                    );
                    println!(
                        "This secret is shown once. Store it in OpenBao or another secret manager."
                    );
                }
            }
            AuthTokenCommand::List(args) => {
                let tokens: Vec<AuthServiceTokenResponse> = get_json(
                    client
                        .get(service_url(&config, "/v1/auth/tokens"))
                        .send()
                        .await?,
                )
                .await?;
                if args.json {
                    print_json(&tokens)?;
                } else if tokens.is_empty() {
                    println!("No service tokens.");
                } else {
                    for token in tokens {
                        let status = if token.revoked_at.is_some() {
                            "revoked"
                        } else if token
                            .expires_at
                            .is_some_and(|time| time <= chrono::Utc::now())
                        {
                            "expired"
                        } else {
                            "active"
                        };
                        println!(
                            "{}  {}  {}  {}",
                            token.id, token.token_prefix, status, token.name
                        );
                        for access in token.projects {
                            println!("  {}: {}", access.project, access.role.as_str());
                        }
                    }
                }
            }
            AuthTokenCommand::Revoke(args) => {
                let token: AuthServiceTokenResponse = get_json(
                    client
                        .post(service_url(
                            &config,
                            &format!("/v1/auth/tokens/{}/revoke", args.selector),
                        ))
                        .send()
                        .await?,
                )
                .await?;
                if args.json {
                    print_json(&token)?;
                } else {
                    println!("Revoked {} ({})", token.name, token.id);
                }
            }
        },
        AuthCommand::Membership(args) => match args.command {
            AuthMembershipCommand::Grant(args) => {
                let membership: AuthMembershipResponse = get_json(
                    client
                        .post(service_url(&config, "/v1/auth/memberships"))
                        .json(&AuthMembershipGrantRequest {
                            principal_id: args.principal,
                            project: args.project,
                            role: args.role.into(),
                        })
                        .send()
                        .await?,
                )
                .await?;
                if args.json {
                    print_json(&membership)?;
                } else {
                    println!(
                        "Granted {} on {} to {} ({})",
                        membership.role.as_str(),
                        membership.project,
                        membership.principal_id,
                        membership.id
                    );
                }
            }
            AuthMembershipCommand::List(args) => {
                let memberships: Vec<AuthMembershipResponse> = get_json(
                    client
                        .get(service_url(&config, "/v1/auth/memberships"))
                        .send()
                        .await?,
                )
                .await?;
                if args.json {
                    print_json(&memberships)?;
                } else if memberships.is_empty() {
                    println!("No project memberships.");
                } else {
                    for membership in memberships {
                        println!(
                            "{}  {}  {}  {}",
                            membership.id,
                            membership.project,
                            membership.role.as_str(),
                            membership.principal_id
                        );
                    }
                }
            }
            AuthMembershipCommand::Revoke(args) => {
                let membership: AuthMembershipResponse = get_json(
                    client
                        .delete(service_url(
                            &config,
                            &format!("/v1/auth/memberships/{}", args.id),
                        ))
                        .send()
                        .await?,
                )
                .await?;
                if args.json {
                    print_json(&membership)?;
                } else {
                    println!(
                        "Revoked {} access to {} for {}",
                        membership.role.as_str(),
                        membership.project,
                        membership.principal_id
                    );
                }
            }
        },
    }
    Ok(())
}

fn print_access(principal: &AuthPrincipalResponse) {
    if principal.projects.is_empty() {
        println!("Projects: inherited from global role");
        return;
    }
    println!("Projects:");
    for access in &principal.projects {
        println!(
            "  {}: {} ({})",
            access.project,
            access.role.as_str(),
            access.source
        );
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
