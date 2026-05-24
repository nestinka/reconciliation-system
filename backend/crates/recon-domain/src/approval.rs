use crate::events::{Case, CaseEventBody};
use crate::types::{BreakStatus, User, UserRole};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ApprovalError {
    #[error("Case is not pending approval.")]
    NotPending,
    #[error("User does not have approver or admin role.")]
    Role,
    #[error("No approval request found in case history.")]
    NoRequest,
    #[error("Maker cannot approve their own proposal (four-eyes principle).")]
    Maker,
}

/// Four-eyes gate, ported from web/lib/case/approval.ts. Fails closed.
pub fn can_approve(c: &Case, user: &User) -> Result<(), ApprovalError> {
    if c.status != BreakStatus::PendingApproval {
        return Err(ApprovalError::NotPending);
    }
    if !matches!(user.role, UserRole::Approver | UserRole::Admin) {
        return Err(ApprovalError::Role);
    }
    let last_request = c
        .events
        .iter()
        .rev()
        .find(|e| matches!(e.body, CaseEventBody::ApprovalRequested { .. }));
    let Some(req) = last_request else {
        return Err(ApprovalError::NoRequest);
    };
    if req.actor_id == user.id {
        return Err(ApprovalError::Maker);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn case_with(status: BreakStatus, events: Vec<CaseEvent>) -> Case {
        Case {
            id: "c".into(),
            break_id: "b".into(),
            assignee_id: None,
            status,
            events,
        }
    }
    fn req(actor: &str) -> CaseEvent {
        CaseEvent {
            id: "r".into(),
            actor_id: actor.into(),
            at: "t".into(),
            body: CaseEventBody::ApprovalRequested {
                resolution: Resolution::WriteOff,
            },
        }
    }
    fn user(id: &str, role: UserRole) -> User {
        User {
            id: id.into(),
            name: id.into(),
            email: format!("{}@example.com", id),
            disabled: false,
            role,
        }
    }

    #[test]
    fn maker_cannot_approve_own_proposal() {
        let c = case_with(BreakStatus::PendingApproval, vec![req("user-mia")]);
        let r = can_approve(&c, &user("user-mia", UserRole::Approver));
        assert!(matches!(r, Err(ApprovalError::Maker)));
    }
    #[test]
    fn operator_cannot_approve() {
        let c = case_with(BreakStatus::PendingApproval, vec![req("user-mia")]);
        let r = can_approve(&c, &user("user-sam", UserRole::Operator));
        assert!(matches!(r, Err(ApprovalError::Role)));
    }
    #[test]
    fn different_approver_can_approve() {
        let c = case_with(BreakStatus::PendingApproval, vec![req("user-mia")]);
        assert!(can_approve(&c, &user("user-theo", UserRole::Approver)).is_ok());
    }
    #[test]
    fn not_pending_is_rejected() {
        let c = case_with(BreakStatus::Open, vec![req("user-mia")]);
        assert!(matches!(
            can_approve(&c, &user("user-theo", UserRole::Approver)),
            Err(ApprovalError::NotPending)
        ));
    }
    #[test]
    fn missing_request_fails_closed() {
        let c = case_with(BreakStatus::PendingApproval, vec![]);
        assert!(matches!(
            can_approve(&c, &user("user-theo", UserRole::Approver)),
            Err(ApprovalError::NoRequest)
        ));
    }
}
