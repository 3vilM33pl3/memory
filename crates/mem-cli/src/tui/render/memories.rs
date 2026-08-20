// SPDX-License-Identifier: AGPL-3.0-or-later

// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{fs, path::Path};

use anyhow::{Context, Result};
use mem_config::repo_agent_settings_path;
use mem_record::{MemoryStatus, MemoryType, ProjectMemoryListItem, ReplacementPolicy};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row},
};
use similar::{ChangeTag, TextDiff};

use crate::commands::memory_ops::SourceKindString;

#[allow(unused_imports)]
use super::*;
use crate::tui::{
    app::*,
    markdown::render_markdown_lines,
    theme::{Theme, themed_focus_block},
};

pub(in crate::tui) fn build_history_lines(
    history: &mem_record::MemoryHistoryResponse,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        label_span("Canonical: "),
        Span::styled(
            history.canonical_id.to_string(),
            Style::default().fg(Theme::TEXT),
        ),
        Span::raw("   "),
        label_span("Versions: "),
        Span::styled(
            history.versions.len().to_string(),
            Style::default().fg(Theme::ACCENT_STRONG),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "Press Shift+H again to return to the single-version detail.",
        Style::default().fg(Theme::MUTED),
    )));
    lines.push(Line::from(""));
    for version in &history.versions {
        let header_style = if version.is_tombstone {
            Style::default()
                .fg(Theme::DANGER)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD)
        };
        let tombstone_suffix = if version.is_tombstone {
            "  [tombstone]"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled(format!("v{}", version.version_no), header_style),
            Span::raw("  "),
            memory_type_span(&version.memory_type),
            Span::raw("  "),
            status_span(match version.status {
                MemoryStatus::Active => "active",
                MemoryStatus::Archived => "archived",
            }),
            Span::styled(
                tombstone_suffix.to_string(),
                Style::default().fg(Theme::DANGER),
            ),
        ]));
        lines.push(Line::from(vec![
            label_span("id: "),
            Span::styled(version.id.to_string(), Style::default().fg(Theme::MUTED)),
            Span::raw("   "),
            label_span("updated: "),
            Span::styled(
                format_timestamp_medium(version.updated_at),
                Style::default().fg(Theme::MUTED),
            ),
        ]));
        if version.is_tombstone {
            lines.push(Line::from(Span::styled(
                "  (empty — memory was deleted at this point)",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            lines.push(Line::from(vec![
                label_span("summary: "),
                Span::styled(version.summary.clone(), Style::default().fg(Theme::TEXT)),
            ]));
            let preview: String = version.canonical_text.chars().take(320).collect();
            let ellipsis = if version.canonical_text.chars().count() > 320 {
                "..."
            } else {
                ""
            };
            lines.push(Line::from(Span::styled(
                format!("{preview}{ellipsis}"),
                Style::default().fg(Theme::TEXT),
            )));
        }
        lines.push(Line::from(""));
    }
    lines
}

pub(in crate::tui) fn build_memory_detail_lines(app: &App) -> Vec<Line<'static>> {
    if let Some(history) = &app.memories.selected_history {
        return build_history_lines(history);
    }
    if let Some(detail) = &app.memories.selected_detail {
        let mut lines = vec![
            Line::from(vec![
                label_span("Summary: "),
                Span::styled(detail.summary.clone(), Style::default().fg(Theme::TEXT)),
            ]),
            Line::from(vec![
                label_span("Type: "),
                memory_type_span(&detail.memory_type),
                Span::raw("   "),
                label_span("Status: "),
                status_span(match detail.status {
                    MemoryStatus::Active => "active",
                    MemoryStatus::Archived => "archived",
                }),
            ]),
            Line::from(vec![
                label_span("Confidence: "),
                Span::styled(
                    format!("{:.2}", detail.confidence),
                    confidence_style(detail.confidence),
                ),
                Span::raw("   "),
                label_span("Importance: "),
                Span::styled(
                    detail.importance.to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(vec![
                label_span("Updated: "),
                Span::styled(
                    format_timestamp_medium(detail.updated_at),
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(""),
        ];
        lines.extend(build_memory_validation_lines(app, detail));
        lines.extend([Line::from(vec![section_span("Embeddings")])]);
        if detail.embedding_spaces.is_empty() {
            lines.push(Line::from(Span::styled(
                "No embeddings for this memory yet. Run Re-embed for this project to populate the active embedding space.",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            for space in &detail.embedding_spaces {
                let chunks_label = if space.chunk_count == 1 {
                    "1 chunk".to_string()
                } else {
                    format!("{} chunks", space.chunk_count)
                };
                let mut spans = vec![
                    Span::styled(space.provider.clone(), Style::default().fg(Theme::ACCENT)),
                    Span::raw(" · "),
                    Span::styled(space.model.clone(), Style::default().fg(Theme::TEXT)),
                    Span::raw(" · "),
                    Span::styled(chunks_label, Style::default().fg(Theme::TEXT)),
                ];
                if let Some(updated) = space.last_updated {
                    spans.push(Span::raw(" · "));
                    spans.push(Span::styled(
                        format!("updated {}", format_timestamp_medium(updated)),
                        Style::default().fg(Theme::MUTED),
                    ));
                }
                lines.push(Line::from(spans));
                if !embedding_base_url_is_default(&space.provider, &space.base_url) {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", space.base_url),
                        Style::default().fg(Theme::MUTED),
                    )));
                }
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Canonical Text")]));
        lines.extend(render_markdown_lines(&detail.canonical_text));
        lines.push(Line::from(""));
        lines.extend([
            Line::from(vec![
                label_span("Tags: "),
                Span::styled(
                    if detail.tags.is_empty() {
                        "none".to_string()
                    } else {
                        detail.tags.join(", ")
                    },
                    Style::default().fg(Theme::TEXT),
                ),
            ]),
            Line::from(""),
            Line::from(vec![section_span("Sources")]),
        ]);

        if detail.sources.is_empty() {
            lines.push(Line::from(Span::styled(
                "No provenance sources recorded.",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            for source in &detail.sources {
                let mut parts = vec![source.source_kind.source_kind_string().to_string()];
                if let Some(path) = &source.file_path {
                    parts.push(path.clone());
                }
                if let Some(excerpt) = &source.excerpt {
                    parts.push(excerpt.clone());
                }
                if let Some(provenance) = &source.provenance {
                    parts.push(format!("provenance: {}", provenance.status.as_str()));
                    if let Some(reason) = &provenance.reason {
                        parts.push(reason.clone());
                    }
                }
                lines.push(Line::from(Span::styled(
                    parts.join(" | "),
                    Style::default().fg(Theme::TEXT),
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![section_span("Related Memories")]));
        if detail.related_memories.is_empty() {
            lines.push(Line::from(Span::styled(
                "No related memories recorded.",
                Style::default().fg(Theme::MUTED),
            )));
        } else {
            for related in &detail.related_memories {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", related.relation_type),
                        Style::default().fg(Theme::ACCENT),
                    ),
                    memory_type_span(&related.memory_type),
                    Span::raw(" "),
                    Span::styled(
                        format!("({:.2}) ", related.confidence),
                        confidence_style(related.confidence),
                    ),
                    Span::styled(related.summary.clone(), Style::default().fg(Theme::TEXT)),
                ]));
            }
        }
        lines
    } else if app.memories.filtered_memories.is_empty() {
        if app.memories.all_memories.is_empty() {
            // Fresh project: point at the fastest first-run wins instead of a
            // dead end.
            vec![
                Line::from(Span::styled(
                    format!("Project {} has no memories yet.", app.project),
                    Style::default().fg(Theme::MUTED),
                )),
                Line::from(Span::raw("")),
                Line::from(Span::styled(
                    "Get something to explore:",
                    Style::default().fg(Theme::TEXT),
                )),
                Line::from(Span::styled(
                    "  memory demo   # load a showcase project",
                    Style::default().fg(Theme::MUTED),
                )),
                Line::from(Span::styled(
                    "  memory tour   # guided remember -> query -> resume walkthrough",
                    Style::default().fg(Theme::MUTED),
                )),
                Line::from(Span::styled(
                    "  memory remember --project <slug> ...   # capture real work",
                    Style::default().fg(Theme::MUTED),
                )),
                Line::from(Span::raw("")),
                Line::from(Span::styled(
                    "Quickstart: https://www.memory-layer.dev/docs/quickstart",
                    Style::default().fg(Theme::MUTED),
                )),
            ]
        } else {
            vec![Line::from(Span::styled(
                format!(
                    "No memories match the current filters for project {}.",
                    app.project
                ),
                Style::default().fg(Theme::MUTED),
            ))]
        }
    } else {
        vec![Line::from(Span::styled(
            "Select a memory to load its details.",
            Style::default().fg(Theme::MUTED),
        ))]
    }
}

pub(in crate::tui) fn build_memory_validation_lines(
    app: &App,
    detail: &mem_record::MemoryEntryResponse,
) -> Vec<Line<'static>> {
    let validation = &app.memories.validation;
    if validation.memory_id != Some(detail.id)
        && !validation.loading
        && validation.run.is_none()
        && validation.error.is_none()
    {
        return Vec::new();
    }

    let mut lines = vec![Line::from(vec![section_span("Validation Proof")])];
    if validation.loading {
        lines.push(Line::from(Span::styled(
            "Searching recorded sources first, then falling back to a bounded repo scan if proof is weak.",
            Style::default().fg(Theme::MUTED),
        )));
    }
    if validation.applying {
        lines.push(Line::from(Span::styled(
            "Applying preview...",
            Style::default().fg(Theme::WARNING),
        )));
    }
    if let Some(error) = &validation.error {
        lines.push(Line::from(vec![
            label_span("Error: "),
            Span::styled(error.clone(), Style::default().fg(Theme::DANGER)),
        ]));
    }
    if let Some(run) = &validation.run {
        let confidence = run
            .confidence
            .map(|value| format!("{value:.2}"))
            .unwrap_or_else(|| "-".to_string());
        lines.push(Line::from(vec![
            label_span("Verdict: "),
            Span::styled(
                run.verdict.clone().unwrap_or_else(|| "-".to_string()),
                Style::default().fg(Theme::ACCENT_STRONG),
            ),
            Span::raw("   "),
            label_span("Confidence: "),
            Span::styled(confidence, Style::default().fg(Theme::TEXT)),
        ]));
        lines.push(Line::from(vec![
            label_span("Action: "),
            Span::styled(
                run.action.clone().unwrap_or_else(|| "-".to_string()),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Scope: "),
            Span::styled(
                run.proof_scope
                    .map(|scope| scope.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                Style::default().fg(Theme::TEXT),
            ),
            Span::raw("   "),
            label_span("Fallback: "),
            Span::styled(
                if run.proof_fallback_used { "yes" } else { "no" },
                Style::default().fg(if run.proof_fallback_used {
                    Theme::WARNING
                } else {
                    Theme::MUTED
                }),
            ),
        ]));
        if !run.reasons.is_empty() {
            lines.push(Line::from(vec![
                label_span("Reasons: "),
                Span::styled(run.reasons.join("; "), Style::default().fg(Theme::MUTED)),
            ]));
        }
        if !run.evidence.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Evidence")]));
            for evidence in &run.evidence {
                let color = match evidence.stance.as_str() {
                    "supports" => Theme::SUCCESS,
                    "contradicts" => Theme::DANGER,
                    _ => Theme::MUTED,
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", evidence.stance),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{} {}", evidence.kind, evidence.evidence_ref),
                        Style::default().fg(Theme::TEXT),
                    ),
                ]));
                if let Some(excerpt) = &evidence.excerpt {
                    for line in excerpt.lines().take(4) {
                        lines.push(Line::from(Span::styled(
                            format!("  {line}"),
                            Style::default().fg(Theme::MUTED),
                        )));
                    }
                }
            }
        }
        if run.proposed_summary.is_some() || run.proposed_text.is_some() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![section_span("Suggested Replacement")]));
            lines.push(Line::from(Span::styled(
                "Press y to apply this preview, n to dismiss it.",
                Style::default().fg(Theme::MUTED),
            )));
            if let Some(summary) = &run.proposed_summary
                && summary != &detail.summary
            {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![label_span("Summary diff")]));
                lines.extend(diff_lines(&detail.summary, summary));
            }
            if let Some(text) = &run.proposed_text
                && text != &detail.canonical_text
            {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![label_span("Canonical text diff")]));
                lines.extend(diff_lines(&detail.canonical_text, text));
            }
        }
    }
    lines.push(Line::from(""));
    lines
}

fn diff_lines(old: &str, new: &str) -> Vec<Line<'static>> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();
    for change in diff.iter_all_changes() {
        let (prefix, color) = match change.tag() {
            ChangeTag::Delete => ("- ", Theme::DANGER),
            ChangeTag::Insert => ("+ ", Theme::SUCCESS),
            ChangeTag::Equal => ("  ", Theme::MUTED),
        };
        let value = change.value().trim_end_matches('\n');
        lines.push(Line::from(Span::styled(
            format!("{prefix}{value}"),
            Style::default().fg(color),
        )));
    }
    lines
}

pub(in crate::tui) fn review_detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(proposal) = app
        .review
        .replacement_proposals
        .get(app.review.replacement_selected_index)
    else {
        return vec![Line::from(Span::styled(
            "Select a proposal on the left to inspect it here.",
            Style::default().fg(Theme::MUTED),
        ))];
    };

    let mut lines = vec![
        Line::from(vec![
            label_span("Target: "),
            Span::styled(
                proposal.target_summary.clone(),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
        Line::from(vec![
            label_span("Candidate: "),
            Span::styled(
                proposal.candidate_summary.clone(),
                Style::default().fg(Theme::ACCENT),
            ),
        ]),
        Line::from(vec![
            label_span("Type / Score / Policy: "),
            Span::styled(
                format!(
                    "{} / {} / {}",
                    proposal.candidate_memory_type, proposal.score, proposal.policy
                ),
                Style::default().fg(Theme::TEXT),
            ),
        ]),
    ];
    if !proposal.reasons.is_empty() {
        lines.push(Line::from(vec![
            label_span("Why: "),
            Span::styled(
                proposal.reasons.join(", "),
                Style::default().fg(Theme::MUTED),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        proposal.candidate_canonical_text.clone(),
        Style::default().fg(Theme::MUTED),
    )));
    lines
}

pub(in crate::tui) fn write_replacement_policy(
    repo_root: &Path,
    policy: ReplacementPolicy,
) -> Result<()> {
    let path = repo_agent_settings_path(repo_root);
    let mut value = if path.exists() {
        fs::read_to_string(&path)?
            .parse::<toml::Value>()
            .context("parse .agents/memory-layer.toml")?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let table = value
        .as_table_mut()
        .context(".agents/memory-layer.toml must be a top-level table")?;
    let curation = table
        .entry("curation".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let curation_table = curation
        .as_table_mut()
        .context("[curation] must be a table")?;
    curation_table.insert(
        "replacement_policy".to_string(),
        toml::Value::String(policy.to_string()),
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml::to_string_pretty(&value)?)?;
    Ok(())
}

pub(in crate::tui) fn memory_row(item: &ProjectMemoryListItem) -> Row<'static> {
    let row_style = match item.status {
        MemoryStatus::Active => Style::default().fg(Theme::TEXT).bg(Theme::PANEL),
        MemoryStatus::Archived => Style::default().fg(Theme::MUTED).bg(Theme::PANEL),
    };
    // Build the summary cell with an optional "v2"/"v3"/... badge so the
    // user can tell at a glance that the row is a replacement rather than
    // an original capture. v1 never shows a badge to keep the list clean.
    let mut summary_spans = Vec::with_capacity(2);
    summary_spans.push(Span::styled(
        item.summary.clone(),
        Style::default().fg(Theme::TEXT),
    ));
    if item.version_no > 1 {
        summary_spans.push(Span::raw("  "));
        summary_spans.push(Span::styled(
            format!("v{}", item.version_no),
            Style::default()
                .fg(Theme::ACCENT_STRONG)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Row::new(vec![
        Cell::from(Line::from(summary_spans)),
        Cell::from(memory_type_span(&item.memory_type)),
        Cell::from(status_span(match item.status {
            MemoryStatus::Active => "active",
            MemoryStatus::Archived => "archived",
        })),
        Cell::from(Span::styled(
            format!("{:.2}", item.confidence),
            confidence_style(item.confidence),
        )),
        Cell::from(Span::styled(
            format_timestamp_medium(item.updated_at),
            Style::default().fg(Theme::MUTED),
        )),
    ])
    .style(row_style)
}

pub(in crate::tui) fn memory_detail_max_scroll(app: &App, frame_area: Rect) -> u16 {
    let root = split_root_area(frame_area);
    let detail_area = split_memories_area(root[2])[1];
    let block = themed_focus_block(
        "Detail",
        app.memories.memories_focus == MemoriesFocus::Detail,
    );
    let inner = block.inner(detail_area);
    if inner.width == 0 || inner.height == 0 {
        return 0;
    }
    wrapped_line_count(&build_memory_detail_lines(app), inner.width)
        .saturating_sub(inner.height as usize) as u16
}

pub(in crate::tui) fn memory_type_span(memory_type: &MemoryType) -> Span<'static> {
    let label = memory_type.to_string();
    memory_type_span_from_label(&label)
}

pub(in crate::tui) fn memory_type_span_from_label(label: &str) -> Span<'static> {
    let color = match label {
        "architecture" => Color::Rgb(120, 190, 255),
        "convention" => Color::Rgb(149, 220, 180),
        "decision" => Color::Rgb(255, 205, 120),
        "incident" => Color::Rgb(255, 140, 140),
        "debugging" => Color::Rgb(255, 170, 110),
        "environment" => Color::Rgb(190, 170, 255),
        "domain_fact" => Color::Rgb(130, 225, 220),
        "documentation" => Color::Rgb(170, 210, 255),
        "plan" => Color::Rgb(255, 120, 200),
        "implementation" => Color::Rgb(120, 230, 140),
        "refactor" => Color::Rgb(130, 220, 160),
        "all" => Theme::TEXT,
        _ => Theme::TEXT,
    };
    Span::styled(
        label.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub(in crate::tui) fn confidence_style(confidence: f32) -> Style {
    let color = if confidence >= 0.8 {
        Theme::SUCCESS
    } else if confidence >= 0.5 {
        Theme::WARNING
    } else {
        Theme::DANGER
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}
