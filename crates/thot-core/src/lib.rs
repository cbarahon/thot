//! Contratos de datos estables compartidos entre API, motor y futuros clientes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudentInput {
    /// Fila original del fichero de origen, incluyendo la cabecera como fila 1.
    #[serde(default)]
    pub source_row: Option<u64>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub surname: Option<String>,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default, rename = "group")]
    pub group_name: Option<String>,
    #[serde(default)]
    pub center: Option<String>,
    #[serde(default)]
    pub birth_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Student {
    pub id: i64,
    pub name: String,
    pub surname: Option<String>,
    pub email: String,
    pub external_id: Option<String>,
    #[serde(rename = "group")]
    pub group_name: Option<String>,
    pub center: Option<String>,
    pub birth_date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportStudentsRequest {
    pub students: Vec<StudentInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedStudent {
    pub row: Option<u64>,
    pub record: StudentInput,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportStudentsResponse {
    pub imported: usize,
    pub discarded: usize,
    pub rejected: Vec<RejectedStudent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetStudentsResponse {
    pub students: Vec<Student>,
    pub total: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_is_serialized_as_group() {
        let input = StudentInput {
            source_row: None,
            name: "Anna".into(),
            surname: None,
            email: "anna@example.org".into(),
            external_id: None,
            group_name: Some("ASIX2".into()),
            center: None,
            birth_date: None,
        };
        let value = serde_json::to_value(input).unwrap();
        assert_eq!(value["group"], "ASIX2");
    }
}
