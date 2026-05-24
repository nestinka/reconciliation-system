use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignBody {
    pub user_id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunQ {
    pub status: Option<String>,
    pub source_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BreakQ {
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub ageing_bucket: Option<String>,
    pub assignee_id: Option<String>,
}

// --- Auth DTOs ---

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResp {
    pub access_token: String,
    pub user: recon_domain::User,
    pub active_tenant: recon_domain::Tenant,
    pub memberships: Vec<recon_domain::Membership>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessTokenResp {
    pub access_token: String,
}
