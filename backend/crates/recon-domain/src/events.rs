use serde::{Deserialize, Serialize};
use crate::types::BreakStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution { WriteOff, ManualMatch }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum CaseEventBody {
    Comment { text: String },
    Assignment {
        #[serde(rename = "assigneeId")]
        assignee_id: String,
    },
    ManualMatchProposed {
        #[serde(rename = "txnIds")]
        txn_ids: Vec<String>,
    },
    WriteOffProposed { reason: String },
    ApprovalRequested { resolution: Resolution },
    Approved {},
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseEvent {
    pub id: String,
    pub actor_id: String,
    pub at: String,
    #[serde(flatten)]
    pub body: CaseEventBody,
}

/// POST body: the client supplies actor + kind + payload; the server assigns id/at.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCaseEvent {
    pub actor_id: String,
    #[serde(flatten)]
    pub body: CaseEventBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Case {
    pub id: String,
    pub break_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    pub status: BreakStatus,
    pub events: Vec<CaseEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_requested_wire_shape() {
        let e = CaseEvent {
            id: "evt-1".into(),
            actor_id: "user-mia".into(),
            at: "2026-05-16T09:35:00Z".into(),
            body: CaseEventBody::ApprovalRequested { resolution: Resolution::WriteOff },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["id"], "evt-1");
        assert_eq!(v["actorId"], "user-mia");
        assert_eq!(v["kind"], "approval_requested");
        assert_eq!(v["payload"]["resolution"], "write_off");
    }

    #[test]
    fn assignment_payload_is_camel_case() {
        let e = CaseEvent {
            id: "e".into(), actor_id: "user-ada".into(), at: "t".into(),
            body: CaseEventBody::Assignment { assignee_id: "user-mia".into() },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "assignment");
        assert_eq!(v["payload"]["assigneeId"], "user-mia");
    }

    #[test]
    fn round_trips_through_json() {
        let json = serde_json::json!({
            "id": "x", "actorId": "user-mia", "at": "t",
            "kind": "comment", "payload": { "text": "hi" }
        });
        let e: CaseEvent = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&e).unwrap(), json);
    }

    #[test]
    fn new_case_event_omits_id_and_at() {
        let json = serde_json::json!({
            "actorId": "user-mia", "kind": "approved", "payload": {}
        });
        let n: NewCaseEvent = serde_json::from_value(json).unwrap();
        assert_eq!(n.actor_id, "user-mia");
        assert!(matches!(n.body, CaseEventBody::Approved {}));
    }
}
