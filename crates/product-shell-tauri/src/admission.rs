//! One native admission boundary for typed component commands.
use crate::operation_log::{begin_operation, OperationGuard};
use product_contract::{Operation, OperationState, Problem, ProblemCode, Provenance, RouteRequest};
use product_ipc::{ComponentCall, ComponentRequest, ExecutionClass, IncomingRequest};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Manager, WebviewWindow};

pub const MAX_ACTIVE: usize = 64;
pub const MAX_ACTIVE_CONTROLS: usize = 8;

#[derive(Default)]
pub struct ActiveRequests(Arc<Mutex<HashMap<String, ExecutionClass>>>);

pub struct Reservation {
    active: Arc<Mutex<HashMap<String, ExecutionClass>>>,
    id: String,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.id);
        }
    }
}
impl ActiveRequests {
    pub fn reserve(&self, id: &str, class: ExecutionClass) -> Result<Reservation, ProblemCode> {
        let mut ids = self.0.lock().map_err(|_| ProblemCode::Unavailable)?;
        if ids.contains_key(id) {
            return Err(ProblemCode::Replayed);
        }
        let limit = match class {
            ExecutionClass::Normal => MAX_ACTIVE,
            ExecutionClass::Control => MAX_ACTIVE_CONTROLS,
        };
        if ids.values().filter(|held| **held == class).count() >= limit {
            return Err(ProblemCode::Overloaded);
        }
        ids.insert(id.to_owned(), class);
        Ok(Reservation {
            active: self.0.clone(),
            id: id.to_owned(),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub operation: Operation,
    pub value: Value,
}

pub struct Admission {
    provenance: Provenance,
    _reservation: Option<Reservation>,
    guard: OperationGuard,
}

/// Decode while the rejection guard is alive. The component identity comes
/// from the native command, never from a renderer-selected owner string.
pub fn admit_request<C: ComponentCall>(
    window: &WebviewWindow,
    incoming: IncomingRequest,
) -> Result<(Admission, ComponentRequest<C>), Problem> {
    let guard = begin_operation(window, C::COMPONENT, &incoming.method);
    let request = incoming
        .decode::<C>()
        .map_err(|_| rejected::<C>(ProblemCode::InvalidRequest))?;
    let admission = admit_with_guard(window, &request.header, &request.call, guard)?;
    Ok((admission, request))
}

pub fn admit<C: ComponentCall>(
    window: &WebviewWindow,
    header: &RouteRequest,
    call: &C,
) -> Result<Admission, Problem> {
    admit_with_guard(
        window,
        header,
        call,
        begin_operation(window, C::COMPONENT, call.method()),
    )
}

fn rejected<C: ComponentCall>(code: ProblemCode) -> Problem {
    let product = C::COMPONENT.split('.').next().unwrap_or("unknown");
    Problem {
        code,
        provenance: Provenance {
            product: product.into(),
            component: format!("{product}.dispatch"),
            request_id: "rejected".into(),
            revision: 1,
        },
    }
}

fn admit_with_guard<C: ComponentCall>(
    window: &WebviewWindow,
    header: &RouteRequest,
    call: &C,
    guard: OperationGuard,
) -> Result<Admission, Problem> {
    let provenance = if C::INSTALLATION_REVIEW {
        crate::authorize_installation_review(window, header)?
    } else if C::IMPORT_PHASE {
        crate::authorize_owner_migration(window, header, C::COMPONENT)?
    } else {
        crate::authorize(window, header, C::COMPONENT)?
    };
    let problem = |code| Problem {
        code,
        provenance: provenance.clone(),
    };
    if !call.routes().contains(&header.route.as_str()) {
        return Err(problem(ProblemCode::Unauthorized));
    }
    let reservation = if C::SHARED_REQUEST_LIMIT {
        let active = window
            .try_state::<ActiveRequests>()
            .ok_or_else(|| problem(ProblemCode::Unavailable))?;
        Some(
            active
                .reserve(&header.request_id, call.class())
                .map_err(problem)?,
        )
    } else {
        None
    };
    Ok(Admission {
        provenance,
        _reservation: reservation,
        guard,
    })
}

pub(crate) fn outcome(
    result: &Result<Value, String>,
    classify: fn(&str) -> &'static str,
) -> (OperationState, Value) {
    match result {
        Ok(value) => (OperationState::Succeeded {}, value.clone()),
        Err(error) => {
            let issue = classify(error);
            let state = if issue == "cancelled" || issue.ends_with("_cancelled") {
                OperationState::Cancelled {}
            } else {
                OperationState::Failed {
                    code: ProblemCode::Unavailable,
                }
            };
            (state, json!({ "issue": issue }))
        }
    }
}

impl Admission {
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    pub fn problem(&self, code: ProblemCode) -> Problem {
        Problem {
            code,
            provenance: self.provenance.clone(),
        }
    }
    pub fn finish(
        self,
        result: Result<Value, String>,
        classify: fn(&str) -> &'static str,
    ) -> Reply {
        let (state, value) = outcome(&result, classify);
        // P0-07's log boundary accepts only projected native codes. A raw
        // String may be a path, server message or secret even if token-shaped.
        let issue = result.as_ref().err().map(|error| classify(error));
        self.guard.finish(&state, issue);
        Reply {
            operation: Operation {
                provenance: self.provenance,
                outcome: state,
            },
            value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replayed_and_overflowing_requests_are_refused() {
        use product_ipc::ExecutionClass::{Control, Normal};
        let active = ActiveRequests::default();
        let first = active.reserve("request-1", Normal).unwrap();
        assert_eq!(
            active.reserve("request-1", Control).err(),
            Some(ProblemCode::Replayed)
        );
        drop(first);
        assert!(active.reserve("request-1", Normal).is_ok());
        let held: Vec<_> = (0..MAX_ACTIVE)
            .map(|index| active.reserve(&format!("r-{index}"), Normal).unwrap())
            .collect();
        assert_eq!(
            active.reserve("one-more", Normal).err(),
            Some(ProblemCode::Overloaded)
        );
        let controls: Vec<_> = (0..MAX_ACTIVE_CONTROLS)
            .map(|index| active.reserve(&format!("c-{index}"), Control).unwrap())
            .collect();
        assert_eq!(
            active.reserve("c-extra", Control).err(),
            Some(ProblemCode::Overloaded)
        );
        drop((held, controls));
        assert!(active.reserve("after-retirement", Normal).is_ok());
    }
    #[test]
    fn outcomes_map_to_operation_states_without_exposing_unknown_errors() {
        assert!(matches!(
            outcome(&Ok(json!(1)), |_| "x"),
            (OperationState::Succeeded {}, _)
        ));
        let (state, value) = outcome(&Err("digest_cancelled".into()), |code| {
            if code == "digest_cancelled" {
                "cancelled"
            } else {
                "unavailable"
            }
        });
        assert!(matches!(state, OperationState::Cancelled {}));
        assert_eq!(value, json!({"issue":"cancelled"}));
        let (state, value) = outcome(&Err("private_token_from_remote".into()), |_| "unavailable");
        assert!(matches!(
            state,
            OperationState::Failed {
                code: ProblemCode::Unavailable
            }
        ));
        assert_eq!(value, json!({"issue":"unavailable"}));
    }
}
