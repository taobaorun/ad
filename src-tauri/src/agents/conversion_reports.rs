use std::path::Path;

use super::{
    opaque_contract_id, resolve_project_agent_workspace, AgentContext, AgentError,
    ArtifactDisposition, ConversionArtifact, ConversionItemFinalState, ConversionItemReport,
    ConversionReport, OperationReceipt, OperationStatus, WorkspaceKey, WorkspaceOperationIssue,
    WorkspaceOperationOutcome,
};

pub(crate) fn conversion_report(
    context: &AgentContext,
    artifacts: &[ConversionArtifact],
    has_mutations: bool,
) -> Result<ConversionReport, AgentError> {
    let workspace_key = conversion_workspace_key(context)?;
    let items = artifacts.iter().map(conversion_item_report).collect();
    let residuals = artifacts
        .iter()
        .filter_map(artifact_residual)
        .collect::<Vec<_>>();
    let outcome = if has_mutations {
        if residuals.is_empty() {
            WorkspaceOperationOutcome::Changed
        } else {
            WorkspaceOperationOutcome::PartialFailure
        }
    } else if artifacts
        .iter()
        .any(|artifact| artifact.disposition == ArtifactDisposition::Conflict)
    {
        WorkspaceOperationOutcome::Conflict
    } else if !residuals.is_empty() {
        WorkspaceOperationOutcome::Unsupported
    } else {
        WorkspaceOperationOutcome::NoChange
    };
    Ok(ConversionReport {
        workspace_key,
        outcome,
        items,
        residuals,
        receipt: None,
    })
}

pub(crate) fn has_required_conversion_residuals(artifacts: &[ConversionArtifact]) -> bool {
    artifacts.iter().any(|artifact| {
        matches!(
            artifact.disposition,
            ArtifactDisposition::RequiresInput | ArtifactDisposition::Conflict
        )
    })
}

pub(crate) fn conversion_plan_is_applicable(
    artifacts: &[ConversionArtifact],
    has_mutations: bool,
    safe_subset: bool,
) -> bool {
    has_mutations && (safe_subset || !has_required_conversion_residuals(artifacts))
}

pub(crate) fn complete_conversion_report(
    mut report: ConversionReport,
    receipt: OperationReceipt,
) -> ConversionReport {
    if receipt.status != OperationStatus::Complete {
        report.outcome = WorkspaceOperationOutcome::PartialFailure;
        report.residuals.push(execution_residual(receipt.status));
        for item in &mut report.items {
            if matches!(
                item.state,
                ConversionItemFinalState::Exact | ConversionItemFinalState::Mapped
            ) {
                item.state = ConversionItemFinalState::Failed;
            }
        }
    }
    report.receipt = Some(receipt);
    report
}

fn conversion_workspace_key(context: &AgentContext) -> Result<WorkspaceKey, AgentError> {
    if let Some(project_path) = context.project_path.as_deref() {
        return resolve_project_agent_workspace(&context.installation_id, Path::new(project_path))
            .map(|workspace| workspace.key);
    }
    Ok(WorkspaceKey::from(opaque_contract_id(
        "conversion-workspace",
        &[context.installation_id.as_str(), "user"],
    )))
}

fn conversion_item_report(artifact: &ConversionArtifact) -> ConversionItemReport {
    ConversionItemReport {
        item_id: artifact.id.clone(),
        state: match artifact.disposition {
            ArtifactDisposition::Exact => ConversionItemFinalState::Exact,
            ArtifactDisposition::Mapped | ArtifactDisposition::Partial => {
                ConversionItemFinalState::Mapped
            }
            ArtifactDisposition::Unchanged => ConversionItemFinalState::Unchanged,
            ArtifactDisposition::RequiresInput => ConversionItemFinalState::RequiresInput,
            ArtifactDisposition::Unsupported => ConversionItemFinalState::Unsupported,
            ArtifactDisposition::Conflict => ConversionItemFinalState::Conflict,
        },
        residuals: artifact_residual(artifact).into_iter().collect(),
    }
}

fn artifact_residual(artifact: &ConversionArtifact) -> Option<WorkspaceOperationIssue> {
    let suffix = match artifact.disposition {
        ArtifactDisposition::Partial => "partial",
        ArtifactDisposition::RequiresInput => "requires_input",
        ArtifactDisposition::Unsupported => "unsupported",
        ArtifactDisposition::Conflict => "conflict",
        ArtifactDisposition::Exact
        | ArtifactDisposition::Mapped
        | ArtifactDisposition::Unchanged => return None,
    };
    Some(WorkspaceOperationIssue {
        code: format!("conversion_{suffix}"),
        message_key: format!("agentConversion.report.{suffix}"),
        resource_key: None,
    })
}

fn execution_residual(status: OperationStatus) -> WorkspaceOperationIssue {
    match status {
        OperationStatus::Complete => {
            unreachable!("complete receipts do not produce execution residuals")
        }
        OperationStatus::Compensated => WorkspaceOperationIssue {
            code: "conversion_compensated".into(),
            message_key: "agentConversion.report.compensated".into(),
            resource_key: None,
        },
        OperationStatus::PartialFailure => WorkspaceOperationIssue {
            code: "conversion_execution_partial_failure".into(),
            message_key: "agentConversion.report.execution_partial_failure".into(),
            resource_key: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{
        ConversionEndpoint, ConversionRiskLevel, InstallationId, ResourceKind, ResourceLocation,
        ResourceOrigin, ResourceRef, ResourceScope,
    };

    fn artifact(disposition: ArtifactDisposition) -> ConversionArtifact {
        ConversionArtifact {
            id: format!("settings:{disposition:?}"),
            kind: ResourceKind::Settings,
            source: ConversionEndpoint {
                resource: ResourceRef {
                    installation_id: InstallationId::from("claude-code:default"),
                    project_path: None,
                    kind: ResourceKind::Settings,
                    scope: ResourceScope::User,
                    logical_id: "user-settings".into(),
                },
                location: ResourceLocation {
                    path: "/Users/test/.claude/settings.json".into(),
                    origin: ResourceOrigin::User,
                },
            },
            target: None,
            disposition,
            resolution: None,
            risk: ConversionRiskLevel::Safe,
            item_count: None,
            detail_code: None,
            message: "test conversion artifact".into(),
        }
    }

    #[test]
    fn required_residuals_block_full_apply_but_allow_an_explicit_safe_subset() {
        let artifacts = vec![
            artifact(ArtifactDisposition::Mapped),
            artifact(ArtifactDisposition::RequiresInput),
        ];

        assert!(!conversion_plan_is_applicable(&artifacts, true, false));
        assert!(conversion_plan_is_applicable(&artifacts, true, true));
        assert!(!conversion_plan_is_applicable(&artifacts, false, true));
    }

    #[test]
    fn report_keeps_item_level_residuals_separate_from_execution_receipts() {
        let context = AgentContext {
            installation_id: InstallationId::from("codex:default"),
            project_path: None,
        };
        let report = conversion_report(
            &context,
            &[
                artifact(ArtifactDisposition::Mapped),
                artifact(ArtifactDisposition::Unsupported),
            ],
            true,
        )
        .unwrap();

        assert_eq!(report.outcome, WorkspaceOperationOutcome::PartialFailure);
        assert_eq!(report.residuals.len(), 1);
        assert_eq!(report.items[0].state, ConversionItemFinalState::Mapped);
        assert_eq!(report.items[1].state, ConversionItemFinalState::Unsupported);
        assert!(report.receipt.is_none());
        assert_eq!(
            report.workspace_key,
            WorkspaceKey::from(opaque_contract_id(
                "conversion-workspace",
                &["codex:default", "user"],
            ))
        );
    }
}
