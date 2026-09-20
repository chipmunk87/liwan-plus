mod common;

use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::{Value, json};

#[tokio::test]
async fn test_project_sessions() -> Result<()> {
    let app = common::app();
    let (tx, _rx) = common::events();
    let client = common::TestClient::new(app.clone(), tx);

    app.seed_database(100)?;

    let project_id = "public-project";
    let sessions_path = format!("/api/dashboard/project/{project_id}/sessions");
    let start_date = (Utc::now() - Duration::days(365)).to_rfc3339();
    let end_date = Utc::now().to_rfc3339();
    let range = json!({"start": start_date, "end": end_date});

    // List succeeds with newest-first rows and consistent counters.
    let res = client.post(&sessions_path, json!({"range": range, "limit": 50})).await;
    res.assert_status_success();
    let body: Value = res.json();
    let data = body.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    assert!(!data.is_empty(), "seeded project should have visitor sessions");

    // "~" sorts after any RFC3339 timestamp, so the first row always passes.
    let mut prev_last_seen = String::from("~");
    for row in &data {
        for key in ["visitorGroupId", "firstSeen", "lastSeen", "visits", "views", "events"] {
            assert!(row.get(key).is_some(), "session row missing {key}");
        }
        let last_seen = row["lastSeen"].as_str().expect("lastSeen is a string");
        assert!(last_seen <= prev_last_seen.as_str(), "sessions should be newest-first");
        prev_last_seen = last_seen.to_string();
        assert!(row["visits"].as_i64().unwrap_or(0) >= 1, "every group has at least one visit");
        assert!(row["views"].as_i64().unwrap_or(-1) >= 0);
        assert!(row["events"].as_i64().unwrap_or(-1) >= 0);
    }

    // Limit is honored and clamped server-side (1..=200).
    let res = client.post(&sessions_path, json!({"range": range, "limit": 1})).await;
    res.assert_status_success();
    let body: Value = res.json();
    assert!(body["data"].as_array().unwrap().len() <= 1);
    let res = client.post(&sessions_path, json!({"range": range, "limit": 10000})).await;
    res.assert_status_success();
    let body: Value = res.json();
    assert!(body["data"].as_array().unwrap().len() <= 200);

    // Timeline for a listed visitor is oldest-first (same-format RFC3339 strings compare chronologically).
    let visitor_group_id = data[0]["visitorGroupId"].as_str().expect("visitorGroupId is a string");
    let timeline_path = format!("{sessions_path}/{visitor_group_id}/timeline");
    let res = client.post(&timeline_path, json!({"range": range, "limit": 200})).await;
    res.assert_status_success();
    let body: Value = res.json();
    let events = body["data"].as_array().cloned().unwrap_or_default();
    assert!(!events.is_empty(), "listed visitor should have timeline events");
    let mut prev = String::new();
    for event in &events {
        let ts = event["createdAt"].as_str().expect("createdAt is a string");
        assert!(ts >= prev.as_str(), "timeline should be oldest-first");
        prev = ts.to_string();
    }

    // Unknown projects 404; private projects 404 when logged out (no cross-project leak).
    client
        .post("/api/dashboard/project/does-not-exist/sessions", json!({"range": range}))
        .await
        .assert_status_not_found();
    client
        .post("/api/dashboard/project/private-project/sessions", json!({"range": range}))
        .await
        .assert_status_not_found();

    Ok(())
}
