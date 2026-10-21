//! Gateway-only operations published in the SDK schema. These are served on the gateway
//! listener, not registered as Control API handlers.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

#[derive(Deserialize, ToSchema)]
struct DocumentInput {
    /// Local library alias. VLMs need a configured multimodal projector.
    model: String,
    /// PNG/JPEG/WebP/GIF or PDF base64 data URL. PDF requires a text layer.
    /// Up to 20 MiB decoded across all pages. Supply this or `pages`, exclusively.
    document: Option<String>,
    /// 1–16 image data URLs, already rasterized, in reading order.
    pages: Option<Vec<String>>,
    /// Required for QA (1–8192 bytes). Ignored for OCR.
    question: Option<String>,
}
#[derive(Serialize, ToSchema)]
struct DocumentPage {
    page: usize,
    markdown: String,
    lines: Vec<String>,
    blocks: Vec<String>,
    /// `vlm`, `pipeline` or `pdf_text`. No bounding boxes are inferred.
    source: String,
}
#[derive(Serialize, ToSchema)]
struct DocumentText {
    model: String,
    markdown: String,
    pages: Vec<DocumentPage>,
}
#[derive(Serialize, ToSchema)]
struct DocumentAnswer {
    model: String,
    answer: String,
    /// Present only when supplied by the DQA pipeline.
    score: Option<f64>,
}

/// Read document text in reading order as Markdown. Gateway listener; named tokens need
/// inference scope and the pipelines endpoint policy. Scanned PDFs must be sent as images.
#[utoipa::path(post, path = "/v1/ocr", operation_id = "read_document", tag = "gateway",
    request_body = DocumentInput, responses((status = 200, body = DocumentText),
    (status = 400, description = "Invalid input, missing PDF text layer or incompatible model"),
    (status = 413, description = "Upload exceeds the decoded or gateway body limit"),
    (status = 429, description = "Token rate limit"), (status = 502, description = "Model failed")))]
fn read_document() {}

/// Ask about document images with a VLM or Python document-question-answering pipeline.
/// Text-layer PDF questions require a VLM. The DQA pipeline accepts one image per request.
/// Gateway listener; named tokens need inference scope and the pipelines endpoint policy.
#[utoipa::path(post, path = "/v1/document/qa", operation_id = "ask_document", tag = "gateway",
    request_body = DocumentInput, responses((status = 200, body = DocumentAnswer),
    (status = 400, description = "Invalid input or incompatible model"),
    (status = 413, description = "Upload exceeds the decoded or gateway body limit"),
    (status = 429, description = "Token rate limit"), (status = 502, description = "Model failed")))]
fn ask_document() {}

#[derive(OpenApi)]
#[openapi(
    paths(read_document, ask_document),
    components(schemas(DocumentInput, DocumentText, DocumentPage, DocumentAnswer))
)]
struct GatewayDocs;

pub fn merge(doc: &mut utoipa::openapi::OpenApi) {
    doc.merge(GatewayDocs::openapi());
}

#[cfg(test)]
mod tests {
    #[test]
    fn document_routes_are_in_sdk_schema() {
        let doc = serde_json::to_value(super::super::openapi_document()).unwrap();
        for path in ["/v1/ocr", "/v1/document/qa"] {
            assert!(doc["paths"][path]["post"]["requestBody"].is_object());
            assert!(doc["paths"][path]["post"]["responses"]["403"].is_object());
        }
    }
}
