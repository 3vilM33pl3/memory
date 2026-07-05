//! Verdict types returned by a [`super::VerdictProvider`] and the strict
//! validation applied to them before anything is persisted. Evidence
//! references are checked against the deterministic stage-1 context: a
//! provider citing something it was never shown fails the run instead of
//! polluting the evidence record.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::ValidationContext;

/// How valid the memory content still is, per the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Valid,
    PartiallyValid,
    Outdated,
    Ambiguous,
    Unsupported,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Valid => "valid",
            Verdict::PartiallyValid => "partially_valid",
            Verdict::Outdated => "outdated",
            Verdict::Ambiguous => "ambiguous",
            Verdict::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStance {
    Supports,
    Contradicts,
    Neutral,
}

impl EvidenceStance {
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceStance::Supports => "supports",
            EvidenceStance::Contradicts => "contradicts",
            EvidenceStance::Neutral => "neutral",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    File,
    CodeSymbol,
    Doc,
    Commit,
    Test,
    Issue,
    Memory,
    SearchHit,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceKind::File => "file",
            EvidenceKind::CodeSymbol => "code_symbol",
            EvidenceKind::Doc => "doc",
            EvidenceKind::Commit => "commit",
            EvidenceKind::Test => "test",
            EvidenceKind::Issue => "issue",
            EvidenceKind::Memory => "memory",
            EvidenceKind::SearchHit => "search_hit",
        }
    }
}

/// Raw provider output, before validation.
#[derive(Debug, Clone, Deserialize)]
pub struct RawVerdict {
    pub verdict: Verdict,
    pub confidence: f32,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<RawEvidence>,
    #[serde(default)]
    pub proposed_summary: Option<String>,
    #[serde(default)]
    pub proposed_text: Option<String>,
    /// False when the memory is technically correct but its wording could
    /// be clearer or easier to retrieve.
    #[serde(default = "default_clarity_ok")]
    pub clarity_ok: bool,
}

fn default_clarity_ok() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawEvidence {
    pub kind: EvidenceKind,
    #[serde(alias = "ref")]
    pub evidence_ref: String,
    pub stance: EvidenceStance,
    #[serde(default)]
    pub excerpt: Option<String>,
}

/// A verdict that passed bounds and evidence-reference validation.
#[derive(Debug, Clone)]
pub struct ValidatedVerdict {
    pub verdict: Verdict,
    pub confidence: f32,
    pub reasons: Vec<String>,
    pub evidence: Vec<RawEvidence>,
    pub proposed_summary: Option<String>,
    pub proposed_text: Option<String>,
    pub clarity_ok: bool,
}

const MAX_REASONS: usize = 16;
const MAX_EVIDENCE: usize = 32;

/// Validates a raw verdict against the gathered context. Rejects
/// out-of-range confidence, oversized lists, empty proposed rewrites, and
/// any evidence reference that was not part of the stage-1 context.
pub fn validate_verdict(raw: RawVerdict, context: &ValidationContext) -> Result<ValidatedVerdict> {
    if !(0.0..=1.0).contains(&raw.confidence) {
        bail!("verdict confidence {} outside 0..=1", raw.confidence);
    }
    if raw.reasons.len() > MAX_REASONS {
        bail!(
            "verdict lists {} reasons (max {MAX_REASONS})",
            raw.reasons.len()
        );
    }
    if raw.evidence.len() > MAX_EVIDENCE {
        bail!(
            "verdict lists {} evidence items (max {MAX_EVIDENCE})",
            raw.evidence.len()
        );
    }
    let mut evidence = raw.evidence;
    for item in &mut evidence {
        let evidence_ref = item.evidence_ref.trim();
        if evidence_ref.is_empty() {
            bail!("verdict evidence reference is empty");
        }
        if context.allows_reference(evidence_ref) {
            item.evidence_ref = evidence_ref.to_string();
            continue;
        }
        // Providers sometimes copy a citable line's trailing annotation
        // ("<sha> (2026-06-15 subject)"); accept and normalize to the bare
        // token when that token alone is in the allowlist.
        let token = evidence_ref.split_whitespace().next().unwrap_or_default();
        if !token.is_empty() && context.allows_reference(token) {
            item.evidence_ref = token.to_string();
            continue;
        }
        bail!("verdict cites evidence not present in gathered context: {evidence_ref}");
    }
    if let Some(summary) = &raw.proposed_summary
        && summary.trim().is_empty()
    {
        bail!("verdict proposed an empty summary");
    }
    if let Some(text) = &raw.proposed_text
        && text.trim().is_empty()
    {
        bail!("verdict proposed an empty canonical text");
    }
    Ok(ValidatedVerdict {
        verdict: raw.verdict,
        confidence: raw.confidence,
        reasons: raw.reasons,
        evidence,
        proposed_summary: raw.proposed_summary,
        proposed_text: raw.proposed_text,
        clarity_ok: raw.clarity_ok,
    })
}

/// Parses provider content (optionally fenced in ```json blocks) into a
/// [`RawVerdict`].
pub fn parse_verdict_content(content: &str) -> Result<RawVerdict> {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            trimmed
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(json).map_err(|error| anyhow::anyhow!("parse verdict JSON: {error}"))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::minimal_context;
    use super::*;

    fn raw(verdict: Verdict, confidence: f32) -> RawVerdict {
        RawVerdict {
            verdict,
            confidence,
            reasons: vec!["reason".to_string()],
            evidence: Vec::new(),
            proposed_summary: None,
            proposed_text: None,
            clarity_ok: true,
        }
    }

    #[test]
    fn accepts_verdict_with_known_reference() {
        let context = minimal_context(&["src/lib.rs"]);
        let mut verdict = raw(Verdict::Valid, 0.9);
        verdict.evidence.push(RawEvidence {
            kind: EvidenceKind::File,
            evidence_ref: "src/lib.rs".to_string(),
            stance: EvidenceStance::Supports,
            excerpt: None,
        });
        assert!(validate_verdict(verdict, &context).is_ok());
    }

    #[test]
    fn normalizes_annotated_reference_to_bare_token() {
        let context = minimal_context(&["abc1234"]);
        let mut verdict = raw(Verdict::Valid, 0.9);
        verdict.evidence.push(RawEvidence {
            kind: EvidenceKind::Commit,
            evidence_ref: "abc1234 (2026-06-15 subject line)".to_string(),
            stance: EvidenceStance::Supports,
            excerpt: None,
        });
        let validated = validate_verdict(verdict, &context).expect("token match accepted");
        assert_eq!(validated.evidence[0].evidence_ref, "abc1234");
    }

    #[test]
    fn rejects_hallucinated_reference() {
        let context = minimal_context(&["src/lib.rs"]);
        let mut verdict = raw(Verdict::Valid, 0.9);
        verdict.evidence.push(RawEvidence {
            kind: EvidenceKind::File,
            evidence_ref: "src/invented.rs".to_string(),
            stance: EvidenceStance::Supports,
            excerpt: None,
        });
        let error = validate_verdict(verdict, &context).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("not present in gathered context")
        );
    }

    #[test]
    fn rejects_out_of_range_confidence_and_empty_rewrites() {
        let context = minimal_context(&[]);
        assert!(validate_verdict(raw(Verdict::Valid, 1.5), &context).is_err());
        let mut verdict = raw(Verdict::Valid, 0.9);
        verdict.proposed_summary = Some("   ".to_string());
        assert!(validate_verdict(verdict, &context).is_err());
    }

    #[test]
    fn parses_fenced_and_bare_json() {
        let body = r#"{"verdict":"outdated","confidence":0.7}"#;
        assert_eq!(
            parse_verdict_content(body).unwrap().verdict,
            Verdict::Outdated
        );
        let fenced = format!("```json\n{body}\n```");
        assert_eq!(
            parse_verdict_content(&fenced).unwrap().verdict,
            Verdict::Outdated
        );
        assert!(parse_verdict_content("not json").is_err());
    }
}
