use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

fn token(cfg: &recon_api::state::AuthConfig, tenant: &str) -> String {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    recon_auth::token::encode_access(
        &cfg.jwt_secret,
        "user-ada",
        tenant,
        recon_domain::UserRole::Admin,
        cfg.access_ttl_secs,
        now,
    )
    .unwrap()
}

async fn json(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (st, v)
}

fn multipart_body(boundary: &str, parts: &[(&str, Option<&str>, &str)]) -> Vec<u8> {
    // parts: (name, filename, value)
    let mut body = Vec::new();
    for (name, filename, value) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(fname) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{fname}\"\r\n\r\n").as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

#[sqlx::test(migrations = "../../migrations")]
async fn full_ingest_pipeline(pool: sqlx::PgPool) {
    // Seed a tenant + the admin user so the token's tenant exists.
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();

    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));

    // 1. Create two sources.
    let mk_source = |name: &str, kind: &str| {
        Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
            .header("content-type", "application/json")
            .body(Body::from(format!("{{\"kind\":\"{kind}\",\"name\":\"{name}\",\"currency\":\"GBP\"}}"))).unwrap()
    };
    let (st, bank) = json(&app, mk_source("Bank", "bank")).await;
    assert_eq!(st, StatusCode::OK, "create bank source");
    let bank_id = bank["id"].as_str().unwrap().to_string();
    let (_st, ledger) = json(&app, mk_source("Ledger", "ledger")).await;
    let ledger_id = ledger["id"].as_str().unwrap().to_string();

    // 2. Ingest a CSV into the bank source.
    let boundary = "BOUNDARY";
    let mapping = r#"{"hasHeader":true,"delimiter":44,"externalRef":{"header":"ref"},"valueDate":{"header":"date"},"dateFormat":"%Y-%m-%d","amount":{"signed":{"column":{"header":"amount"},"debitWhenNegative":true}},"description":{"header":"desc"}}"#;
    let csv = "ref,date,amount,desc\nA1,2026-05-10,-10.00,Coffee\nA2,2026-05-11,-99.99,Lunch\n";
    let body = multipart_body(boundary, &[
        ("file", Some("bank.csv"), csv),
        ("format", None, "csv"),
        ("mapping", None, mapping),
    ]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{bank_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "ingest csv: {v}");
    assert_eq!(v["ingested"], 2);

    // 3. Re-uploading the same CSV is a 409 duplicate.
    let body = multipart_body(boundary, &[
        ("file", Some("bank.csv"), csv),
        ("format", None, "csv"),
        ("mapping", None, mapping),
    ]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{bank_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::CONFLICT, "duplicate ingest");
    assert_eq!(v["error"]["code"], "duplicate");
    assert!(v["error"]["refs"].as_array().unwrap().contains(&Value::from("A1")));

    // 4. A bad CSV row -> 422 with the parse report.
    let bad = "ref,date,amount,desc\nB1,not-a-date,-1.00,Bad\n";
    let body = multipart_body(boundary, &[
        ("file", Some("bad.csv"), bad),
        ("format", None, "csv"),
        ("mapping", None, mapping),
    ]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{ledger_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "bad row");
    assert_eq!(v["error"]["code"], "parse");
    assert_eq!(v["error"]["rows"][0]["field"], "valueDate");

    // 5. Ingest a CAMT.053 into the ledger source.
    let camt = r#"<Document><Stmt><Ntry><Amt Ccy="GBP">10.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><ValDt><Dt>2026-05-10</Dt></ValDt><NtryRef>A1</NtryRef><AddtlNtryInf>Coffee</AddtlNtryInf></Ntry></Stmt></Document>"#;
    let body = multipart_body(boundary, &[
        ("file", Some("ledger.xml"), camt),
        ("format", None, "camt053"),
    ]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{ledger_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "ingest camt: {v}");
    assert_eq!(v["ingested"], 1);

    // 6. Create a run over the window and read it back.
    let req = Request::builder().method("POST").uri("/api/runs").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(format!("{{\"name\":\"R\",\"sourceAId\":\"{bank_id}\",\"sourceBId\":\"{ledger_id}\",\"from\":\"2026-05-01\",\"to\":\"2026-05-31\"}}"))).unwrap();
    let (st, run) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "create run: {run}");
    let run_id = run["id"].as_str().unwrap();

    let req = Request::builder().method("GET").uri(format!("/api/runs/{run_id}")).header("authorization", &auth).body(Body::empty()).unwrap();
    let (st, detail) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(detail["run"]["id"], run_id);

    // 7. Invalid date range -> 400.
    let req = Request::builder().method("POST").uri("/api/runs").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(format!("{{\"name\":\"R\",\"sourceAId\":\"{bank_id}\",\"sourceBId\":\"{ledger_id}\",\"from\":\"2026-05-31\",\"to\":\"2026-05-01\"}}"))).unwrap();
    let (st, _) = json(&app, req).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_missing_format_is_bad_request(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s1','tenant-acme','bank','Bank','GBP')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();
    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));
    let boundary = "B";
    let body = multipart_body(boundary, &[("file", Some("x.csv"), "ref,date\n")]); // no `format` field
    let req = Request::builder().method("POST").uri("/api/sources/s1/ingest")
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _) = json(&app, req).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn cross_tenant_ingest_is_not_found(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme'),('tenant-globex','Globex','globex')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s-acme','tenant-acme','bank','Bank','GBP')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-globex','admin')").execute(&pool).await.unwrap();

    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-globex"));
    let boundary = "B";
    let body = multipart_body(boundary, &[("file", Some("x.xml"), "<Document></Document>"), ("format", None, "camt053")]);
    let req = Request::builder().method("POST").uri("/api/sources/s-acme/ingest")
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _) = json(&app, req).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
