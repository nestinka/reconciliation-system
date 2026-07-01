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

// POST /auth/login as the given seeded user; returns just the accessToken.
async fn login_as(app: &axum::Router, email: &str, password: &str) -> String {
    let body = serde_json::json!({ "email": email, "password": password });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "login_as: expected 200");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    v["accessToken"]
        .as_str()
        .expect("accessToken in /auth/login response")
        .to_string()
}

const MT940_FIXTURE: &[u8] = b":20:REF20250601
:25:GB29NWBK60161331926819
:28C:00123/00001
:60F:C250601GBP1000,00
:61:250601D100,00NTRFBANKREF-1//BNKREF-A
:86:Counterparty payment
:62F:C250601GBP900,00
";

#[sqlx::test(migrations = "../../migrations")]
async fn mt940_happy_path_ingest(pool: sqlx::PgPool) {
    recon_store::Store::from_pool(pool.clone())
        .seed()
        .await
        .unwrap();
    let (app, _cfg) = recon_api::test_app(pool.clone());
    let token = login_as(&app, "ada@acme.test", "Password123!").await;

    // Create a source with format_dialect = subfielded.
    let body = serde_json::json!({
        "kind": "bank", "name": "MT940 Test", "currency": "GBP", "formatDialect": "subfielded"
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sources")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let src: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap();
    let src_id = src["id"].as_str().unwrap().to_string();

    // Upload an MT940 file via multipart.
    let boundary = "----recon-test";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"format\"\r\n\r\nmt940\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"acme.sta\"\r\nContent-Type: application/octet-stream\r\n\r\n{}\r\n--{boundary}--\r\n",
        std::str::from_utf8(MT940_FIXTURE).unwrap()
    );

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sources/{src_id}/ingest"))
                .header("authorization", format!("Bearer {token}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap();
    assert_eq!(body["ingested"], 1);
}

const BAI2_FIXTURE: &[u8] = b"01,SENDR,RCVR,250601,0930,12345,80,2,2/
02,ACME,SENDR,1,250601,0930,USD,2/
03,123456789,USD,010,500000,,,015,500000,,/
16,175,25000,V,BNKREF-A,CUSTREF-1,Deposit from customer/
49,25000,2/
98,25000,1,3/
99,25000,1,5/
";

#[sqlx::test(migrations = "../../migrations")]
async fn bai2_happy_path_ingest(pool: sqlx::PgPool) {
    recon_store::Store::from_pool(pool.clone())
        .seed()
        .await
        .unwrap();
    let (app, _cfg) = recon_api::test_app(pool.clone());
    let token = login_as(&app, "ada@acme.test", "Password123!").await;

    // Create a source — no dialect (BAI2 has no variants).
    let body = serde_json::json!({
        "kind": "bank", "name": "BAI2 Test", "currency": "USD"
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sources")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let src: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap();
    let src_id = src["id"].as_str().unwrap().to_string();

    // Upload a BAI2 file.
    let boundary = "----recon-test";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"format\"\r\n\r\nbai2\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"acme.bai\"\r\nContent-Type: application/octet-stream\r\n\r\n{}\r\n--{boundary}--\r\n",
        std::str::from_utf8(BAI2_FIXTURE).unwrap()
    );

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sources/{src_id}/ingest"))
                .header("authorization", format!("Bearer {token}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap();
    assert_eq!(body["ingested"], 1);
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

const MT942_FIXTURE: &[u8] = b":20:INTRA-DAY-1
:25:DE89370400440532013000
:28C:1/1
:34F:EUR0,00
:13D:2601011200+0100
:61:260101D250,00NTRFCUSTREF-A//BNKREF-1
:86:Intra-day debit one
:61:260101C500,00NTRFCUSTREF-B//BNKREF-2
:86:Intra-day credit one
:90D:1EUR250,00
:90C:1EUR500,00
";

#[sqlx::test(migrations = "../../migrations")]
async fn pdf_ingest_pipeline(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();

    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));

    // pdf-profiles endpoint lists the registry.
    let req = Request::builder().method("GET").uri("/api/pdf-profiles")
        .header("authorization", &auth).body(Body::empty()).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["profiles"].as_array().unwrap().contains(&Value::from("acmebank")));

    // Create a source WITH a pdf profile.
    let req = Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"kind":"bank","name":"PDF Bank","currency":"GBP","pdfProfile":"acmebank"}"#)).unwrap();
    let (st, src) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "create: {src}");
    assert_eq!(src["pdfProfile"], "acmebank");
    let src_id = src["id"].as_str().unwrap().to_string();

    // Upload the committed PDF fixture.
    let pdf = std::fs::read("../recon-ingest/tests/fixtures/pdf-acmebank.pdf").expect("fixture");
    let boundary = "BOUNDARY";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"s.pdf\"\r\n\r\n").as_bytes());
    body.extend_from_slice(&pdf);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"format\"\r\n\r\npdf\r\n").as_bytes());
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::builder().method("POST").uri(format!("/api/sources/{src_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "pdf ingest: {v}");
    assert_eq!(v["ingested"], 3);

    // A source with NO pdf profile rejects pdf upload with 400.
    let req = Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"kind":"ledger","name":"No Profile","currency":"GBP"}"#)).unwrap();
    let (st, np) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "create no-profile: {np}");
    let np_id = np["id"].as_str().unwrap().to_string();
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"s.pdf\"\r\n\r\n").as_bytes());
    body.extend_from_slice(&pdf);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"format\"\r\n\r\npdf\r\n").as_bytes());
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::builder().method("POST").uri(format!("/api/sources/{np_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _v) = json(&app, req).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "pdf upload without profile must 400");
}

#[sqlx::test(migrations = "../../migrations")]
async fn mt942_happy_path_ingest(pool: sqlx::PgPool) {
    recon_store::Store::from_pool(pool.clone())
        .seed()
        .await
        .unwrap();
    let (app, _cfg) = recon_api::test_app(pool.clone());
    let token = login_as(&app, "ada@acme.test", "Password123!").await;

    let body = serde_json::json!({
        "kind": "bank", "name": "MT942 Test", "currency": "EUR", "formatDialect": null
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sources")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let src: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap();
    let src_id = src["id"].as_str().unwrap().to_string();

    let boundary = "----recon-test";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"format\"\r\n\r\nmt942\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"intraday.sta\"\r\nContent-Type: application/octet-stream\r\n\r\n{}\r\n--{boundary}--\r\n",
        std::str::from_utf8(MT942_FIXTURE).unwrap()
    );

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sources/{src_id}/ingest"))
                .header("authorization", format!("Bearer {token}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
    )
    .unwrap();
    assert_eq!(body["ingested"], 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn auto_detect_dispatches_by_content(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();
    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));

    let req = Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"kind":"bank","name":"Auto Bank","currency":"GBP","formatDialect":"generic"}"#)).unwrap();
    let (st, src) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "create: {src}");
    let src_id = src["id"].as_str().unwrap().to_string();

    let mt940 = ":20:STMT001\r\n:25:12345\r\n:28C:1/1\r\n:60F:C260501GBP0,00\r\n:61:2605010501D45,20NTRFREF//BANK\r\n:86:PAYMENT\r\n:62F:C260501GBP45,20\r\n";
    let boundary = "BOUNDARY";
    let body = multipart_body(boundary, &[("file", Some("s.sta"), mt940), ("format", None, "auto")]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{src_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "auto ingest: {v}");
    assert_eq!(v["ingested"], 1);

    let body = multipart_body(boundary, &[("file", Some("x.csv"), "ref,date,amount\nA1,2026-05-01,10.00\n"), ("format", None, "auto")]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{src_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _v) = json(&app, req).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "auto cannot detect CSV");
}
