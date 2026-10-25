//! Purpose: Integration tests for student import and retrieval API behavior.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use thot_api::router;
use thot_engine::{SqliteStudentRepository, StudentService};
use tower::ServiceExt;

#[tokio::test]
async fn imports_valid_students_reports_discards_and_lists_saved_students() {
    let database_path = std::env::temp_dir().join(format!(
        "thot-student-import-test-{}-{}.sqlite3",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let database_url = format!("sqlite:{}", database_path.display());
    let repository = SqliteStudentRepository::connect(&database_url)
        .await
        .expect("test database should initialize");
    let app = router(StudentService::new(repository));

    let import_request = Request::builder()
        .method("POST")
        .uri("/import/students")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{
                "students": [
                    {
                        "source_row": 2,
                        "name": "Anna Garcia",
                        "email": "anna@example.org",
                        "surname": "Garcia",
                        "group": "ASIX2",
                        "ignored_column": "ignored"
                    },
                    {
                        "source_row": 3,
                        "name": " ",
                        "email": "invalid@example.org"
                    },
                    {
                        "source_row": 4,
                        "name": "Duplicate Anna",
                        "email": "ANNA@example.org"
                    }
                ]
            }"#,
        ))
        .unwrap();

    let import_response = app.clone().oneshot(import_request).await.unwrap();
    assert_eq!(import_response.status(), StatusCode::OK);
    let import_body = to_bytes(import_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let import_json: serde_json::Value = serde_json::from_slice(&import_body).unwrap();

    assert_eq!(import_json["imported"], 1);
    assert_eq!(import_json["discarded"], 2);
    assert_eq!(import_json["rejected"][0]["row"], 3);
    assert_eq!(
        import_json["rejected"][0]["reason"],
        "El nombre es obligatorio"
    );
    assert_eq!(import_json["rejected"][1]["row"], 4);
    assert_eq!(
        import_json["rejected"][1]["reason"],
        "Ya existe un estudiante con este correo electrónico"
    );

    let list_request = Request::builder()
        .method("GET")
        .uri("/get/students")
        .body(Body::empty())
        .unwrap();
    let list_response = app.clone().oneshot(list_request).await.unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = to_bytes(list_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_json: serde_json::Value = serde_json::from_slice(&list_body).unwrap();

    assert_eq!(list_json["total"], 1);
    assert_eq!(list_json["students"].as_array().unwrap().len(), 1);
    assert_eq!(list_json["students"][0]["name"], "Anna Garcia");
    assert_eq!(list_json["students"][0]["email"], "anna@example.org");
    assert_eq!(list_json["students"][0]["group"], "ASIX2");

    drop(app);
    let _ = std::fs::remove_file(&database_path);
}
