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

// --- New Auth/Admin DTOs ---

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchTenantReq {
    pub tenant_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordReq {
    pub current_password: String,
    pub new_password: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgotReq {
    pub email: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetReq {
    pub token: String,
    pub new_password: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserReq {
    pub name: String,
    pub email: String,
    pub role: recon_domain::UserRole,
    pub password: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchUserReq {
    pub role: Option<recon_domain::UserRole>,
    pub disabled: Option<bool>,
}

// --- Ingestion DTOs ---

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSourceReq {
    pub kind: recon_domain::SourceKind,
    pub name: String,
    pub currency: String,
    #[serde(default)]
    pub format_dialect: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunReq {
    pub name: String,
    pub source_a_id: String,
    pub source_b_id: String,
    pub from: String,
    pub to: String,
}

/// PATCH /sources/:id request body.
///
/// `format_dialect` uses a double-`Option` so we can distinguish three states:
///   - field absent in JSON           → don't change
///   - field present with `null`      → clear the dialect
///   - field present with a value     → set it
#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSourceReq {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub format_dialect: Option<Option<String>>,
}

fn deserialize_double_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(de).map(Some)
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
