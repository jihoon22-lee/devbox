//! Pure retry planning for Start Workspace.
//!
//! The planner has no process or filesystem access.  It turns the stable run
//! step/provenance records into a deterministic resume point so command code
//! cannot accidentally restart an idempotent step or a process/resource that
//! was already running before Workbench was invoked.

use super::preflight::{ResourceProvenance, ResourceState};

pub const WAIT_PORT_STEP: &str = "wait-port";
pub const OPEN_WSL_STEP: &str = "open-wsl-desktop";
pub const OPEN_CODE_PAD_STEP: &str = "open-code-pad";
pub const RETRY_STEP_ORDER: &[&str] = &[WAIT_PORT_STEP, OPEN_WSL_STEP, OPEN_CODE_PAD_STEP];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryStep<'a> {
    pub name: &'a str,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPlan {
    /// The first failed step from which this attempt resumes.
    pub resume_from: String,
    /// Steps that must not run again.  This includes successful steps and
    /// child steps already represented by existing or Workbench-owned provenance.
    pub skipped_steps: Vec<String>,
    /// Failed steps in deterministic execution order, including the resume
    /// step and any later steps that still need work.
    pub pending_steps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryPlanError {
    NoFailedStep,
    UnknownStep,
}

impl RetryPlanError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::NoFailedStep => "다시 시도할 실패 단계가 없습니다",
            Self::UnknownStep => "Workspace 실행 단계가 올바르지 않아 다시 시도할 수 없습니다",
        }
    }
}

/// Build a plan from the persisted run state.  A run can only be retried from
/// one of the known bounded steps; unknown values fail closed rather than
/// becoming a free-form command selector.
pub fn plan_retry(
    steps: &[RetryStep<'_>],
    resources: &[ResourceProvenance],
) -> Result<RetryPlan, RetryPlanError> {
    // Successful preflight metadata may be present in a run record but is not
    // an executable retry step.  An unknown *failed* step is still rejected;
    // this prevents a partially upgraded record from selecting an arbitrary
    // command target.
    if steps
        .iter()
        .any(|step| !step.ok && !step.name.is_empty() && !RETRY_STEP_ORDER.contains(&step.name))
    {
        return Err(RetryPlanError::UnknownStep);
    }

    // A malformed or migrated run may contain duplicate entries. Reject a
    // conflicting known step rather than letting vector order decide which
    // result becomes the retry authority.
    if RETRY_STEP_ORDER
        .iter()
        .any(|name| steps.iter().filter(|step| step.name == *name).count() > 1)
    {
        return Err(RetryPlanError::UnknownStep);
    }

    // Use the canonical execution order, not the order in which a renderer or
    // an older backend happened to serialize steps. This guarantees that a
    // malformed ordering cannot skip an earlier failed dependency boundary.
    let (resume_index, first_failed_name) = RETRY_STEP_ORDER
        .iter()
        .enumerate()
        .find_map(|(index, name)| {
            steps
                .iter()
                .any(|step| step.name == *name && !step.ok)
                .then_some((index, *name))
        })
        .ok_or_else(|| {
            if steps.iter().any(|step| !step.ok) {
                RetryPlanError::UnknownStep
            } else {
                RetryPlanError::NoFailedStep
            }
        })?;

    let mut skipped_steps = Vec::new();
    let mut pending_steps = Vec::new();
    for name in RETRY_STEP_ORDER.iter().skip(resume_index) {
        let step = steps.iter().find(|step| step.name == *name);
        if step.is_some_and(|step| step.ok) || existing_process_for_step(name, resources) {
            skipped_steps.push((*name).to_string());
        } else {
            pending_steps.push((*name).to_string());
        }
    }

    Ok(RetryPlan {
        resume_from: first_failed_name.to_string(),
        skipped_steps,
        pending_steps,
    })
}

pub fn can_retry(steps: &[RetryStep<'_>], resources: &[ResourceProvenance]) -> bool {
    plan_retry(steps, resources).is_ok_and(|plan| !plan.pending_steps.is_empty())
}

pub fn failed_step<'a>(steps: &[RetryStep<'a>]) -> Option<&'a str> {
    RETRY_STEP_ORDER.iter().find_map(|name| {
        steps
            .iter()
            .find(|step| step.name == *name && !step.ok)
            .map(|step| step.name)
    })
}

fn existing_process_for_step(step: &str, resources: &[ResourceProvenance]) -> bool {
    let app_id = match step {
        OPEN_WSL_STEP => "wsl-desktop",
        OPEN_CODE_PAD_STEP => "code-pad",
        _ => return false,
    };
    resources.iter().any(|resource| {
        resource.kind == "process"
            && resource.id == app_id
            && matches!(
                resource.state,
                ResourceState::Existing | ResourceState::WorkbenchStarted
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(id: &str, state: ResourceState) -> ResourceProvenance {
        ResourceProvenance {
            kind: "process".into(),
            id: id.into(),
            state,
        }
    }

    #[test]
    fn retry_resumes_at_first_failure_and_skips_owned_success() {
        let steps = [
            RetryStep {
                name: WAIT_PORT_STEP,
                ok: true,
            },
            RetryStep {
                name: OPEN_WSL_STEP,
                ok: true,
            },
            RetryStep {
                name: OPEN_CODE_PAD_STEP,
                ok: false,
            },
        ];
        let plan = plan_retry(&steps, &[]).unwrap();
        assert_eq!(plan.resume_from, OPEN_CODE_PAD_STEP);
        assert!(plan.skipped_steps.is_empty());
        assert_eq!(plan.pending_steps, vec![OPEN_CODE_PAD_STEP.to_string()]);
        assert!(can_retry(&steps, &[]));
    }

    #[test]
    fn retry_does_not_restart_a_workbench_owned_resource() {
        let steps = [
            RetryStep {
                name: WAIT_PORT_STEP,
                ok: true,
            },
            RetryStep {
                name: OPEN_WSL_STEP,
                ok: false,
            },
            RetryStep {
                name: OPEN_CODE_PAD_STEP,
                ok: false,
            },
        ];
        let plan = plan_retry(
            &steps,
            &[resource("wsl-desktop", ResourceState::WorkbenchStarted)],
        )
        .unwrap();
        assert_eq!(plan.resume_from, OPEN_WSL_STEP);
        assert_eq!(plan.skipped_steps, vec![OPEN_WSL_STEP.to_string()]);
        assert_eq!(plan.pending_steps, vec![OPEN_CODE_PAD_STEP.to_string()]);
    }

    #[test]
    fn existing_external_resource_is_not_restarted() {
        let steps = [RetryStep {
            name: OPEN_WSL_STEP,
            ok: false,
        }];
        let external = ResourceProvenance {
            kind: "process".into(),
            id: "wsl-desktop".into(),
            state: ResourceState::Existing,
        };
        let plan = plan_retry(&steps, std::slice::from_ref(&external)).unwrap();
        assert_eq!(plan.skipped_steps, vec![OPEN_WSL_STEP.to_string()]);
        assert_eq!(plan.pending_steps, vec![OPEN_CODE_PAD_STEP.to_string()]);
        assert!(can_retry(&steps, std::slice::from_ref(&external)));
    }

    #[test]
    fn no_failure_and_unknown_step_fail_closed() {
        let success = [RetryStep {
            name: OPEN_CODE_PAD_STEP,
            ok: true,
        }];
        assert_eq!(plan_retry(&success, &[]), Err(RetryPlanError::NoFailedStep));

        let unknown = [RetryStep {
            name: "future-step",
            ok: false,
        }];
        assert_eq!(plan_retry(&unknown, &[]), Err(RetryPlanError::UnknownStep));
        assert_eq!(failed_step(&unknown), None);
    }

    #[test]
    fn retry_uses_canonical_order_and_rejects_duplicate_steps() {
        let out_of_order = [
            RetryStep {
                name: OPEN_CODE_PAD_STEP,
                ok: false,
            },
            RetryStep {
                name: WAIT_PORT_STEP,
                ok: false,
            },
        ];
        let plan = plan_retry(&out_of_order, &[]).unwrap();
        assert_eq!(plan.resume_from, WAIT_PORT_STEP);
        assert_eq!(
            plan.pending_steps,
            RETRY_STEP_ORDER
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        );

        let duplicate = [
            RetryStep {
                name: OPEN_CODE_PAD_STEP,
                ok: false,
            },
            RetryStep {
                name: OPEN_CODE_PAD_STEP,
                ok: true,
            },
        ];
        assert_eq!(
            plan_retry(&duplicate, &[]),
            Err(RetryPlanError::UnknownStep)
        );
    }
}
