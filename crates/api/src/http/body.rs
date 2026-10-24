//! 활성 본문 하나를 검사하고 요청마다(리디렉션 hop마다) 다시 만든다.
use crate::validate::{MAX_PARTS, MAX_TEXT_BODY, too_large};
use crate::{ApiError, Body, GraphqlMethod, Part};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderValue};
use serde_json::value::RawValue;
use std::path::PathBuf;

pub(crate) enum PreparedPart {
    Text {
        name: String,
        value: String,
        content_type: Option<String>,
    },
    File {
        name: String,
        path: PathBuf,
        file_name: String,
        content_type: Option<String>,
        len: u64,
    },
}

pub(crate) enum PreparedBody {
    None,
    Bytes {
        data: bytes::Bytes,
        content_type: Option<HeaderValue>,
    },
    Multipart(Vec<PreparedPart>),
    File {
        path: PathBuf,
        len: u64,
        content_type: HeaderValue,
    },
}

pub(crate) struct Graphql {
    /// GET으로 보낼 때(또는 같은 origin의 GET 리디렉션) 붙일 query.
    pub(crate) params: Vec<(String, String)>,
    pub(crate) post: bool,
}

fn file(index: u32, path: &str) -> Result<(PathBuf, u64), ApiError> {
    let path = PathBuf::from(path);
    let meta = path
        .is_absolute()
        .then(|| std::fs::metadata(&path).ok())
        .flatten()
        .filter(std::fs::Metadata::is_file)
        .ok_or(ApiError::FileMissing { part: index })?;
    Ok((path, meta.len()))
}

fn text(text: &str) -> Result<bytes::Bytes, ApiError> {
    if text.len() > MAX_TEXT_BODY {
        return Err(too_large(MAX_TEXT_BODY));
    }
    Ok(bytes::Bytes::copy_from_slice(text.as_bytes()))
}

fn content_type(value: &Option<String>) -> Result<Option<HeaderValue>, ApiError> {
    value
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            let v = crate::validate::field_value(v).map_err(|_| ApiError::body("content_type"))?;
            HeaderValue::from_str(v).map_err(|_| ApiError::body("content_type"))
        })
        .transpose()
}

fn part_type(value: &Option<String>) -> Result<Option<String>, ApiError> {
    let Some(value) = value.as_deref().filter(|v| !v.trim().is_empty()) else {
        return Ok(None);
    };
    reqwest::multipart::Part::text("")
        .mime_str(value.trim())
        .map_err(|_| ApiError::body("content_type"))?;
    Ok(Some(value.trim().to_owned()))
}

fn graphql_variables(text: &str) -> Result<String, ApiError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok("{}".into());
    }
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text)
        .map_err(|_| ApiError::body("graphql_variables"))?;
    Ok(text.to_owned())
}

#[derive(serde::Serialize)]
struct Payload<'a> {
    query: &'a str,
    variables: &'a RawValue,
    #[serde(rename = "operationName", skip_serializing_if = "Option::is_none")]
    operation_name: Option<&'a str>,
}

pub(crate) fn prepare(body: &Body) -> Result<(PreparedBody, Option<Graphql>), ApiError> {
    let json = HeaderValue::from_static("application/json");
    Ok(match body {
        Body::None => (PreparedBody::None, None),
        Body::Raw {
            text: t,
            content_type: ct,
            encoding,
        } => (
            PreparedBody::Bytes {
                data: if encoding.is_some() {
                    let _ = text(t)?;
                    data_encoding::BASE64
                        .decode(t.as_bytes())
                        .map_err(|_| ApiError::body("invalid_base64"))?
                        .into()
                } else {
                    text(t)?
                },
                content_type: content_type(ct)?,
            },
            None,
        ),
        Body::Json { text: t } => (
            PreparedBody::Bytes {
                data: text(t)?,
                content_type: Some(json),
            },
            None,
        ),
        Body::Form { fields } => {
            let encoded = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|p| (&p.name, &p.value)))
                .finish();
            (
                PreparedBody::Bytes {
                    data: text(&encoded)?,
                    content_type: Some(HeaderValue::from_static(
                        "application/x-www-form-urlencoded",
                    )),
                },
                None,
            )
        }
        Body::Multipart { parts } => {
            if parts.len() > MAX_PARTS {
                return Err(too_large(MAX_PARTS));
            }
            let mut text_bytes = 0;
            let mut out = Vec::with_capacity(parts.len());
            for (index, part) in parts.iter().enumerate() {
                out.push(match part {
                    Part::Text {
                        name,
                        value,
                        content_type,
                    } => {
                        text_bytes += value.len();
                        PreparedPart::Text {
                            name: name.clone(),
                            value: value.clone(),
                            content_type: part_type(content_type)?,
                        }
                    }
                    Part::File {
                        name,
                        path,
                        file_name,
                        content_type,
                    } => {
                        let (path, len) = file(index as u32, path)?;
                        let file_name = file_name
                            .clone()
                            .filter(|n| !n.is_empty())
                            .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
                            .unwrap_or_else(|| "file".into());
                        PreparedPart::File {
                            name: name.clone(),
                            path,
                            file_name,
                            content_type: part_type(content_type)?,
                            len,
                        }
                    }
                });
            }
            if text_bytes > MAX_TEXT_BODY {
                return Err(too_large(MAX_TEXT_BODY));
            }
            (PreparedBody::Multipart(out), None)
        }
        Body::Binary { path } => {
            let (path, len) = file(0, path)?;
            (
                PreparedBody::File {
                    path,
                    len,
                    content_type: HeaderValue::from_static("application/octet-stream"),
                },
                None,
            )
        }
        Body::Graphql {
            query,
            variables,
            operation_name,
            method,
        } => {
            if query.trim().is_empty() {
                return Err(ApiError::body("graphql_query"));
            }
            if query.len() + variables.len() > MAX_TEXT_BODY {
                return Err(too_large(MAX_TEXT_BODY));
            }
            let variables = graphql_variables(variables)?;
            let name = operation_name.as_deref().filter(|n| !n.is_empty());
            let mut params = vec![
                ("query".to_string(), query.clone()),
                ("variables".into(), variables.clone()),
            ];
            if let Some(name) = name {
                params.push(("operationName".into(), name.into()));
            }
            let post = *method == GraphqlMethod::Post;
            let body = if post {
                let raw = RawValue::from_string(variables)
                    .map_err(|_| ApiError::body("graphql_variables"))?;
                let payload = serde_json::to_vec(&Payload {
                    query,
                    variables: &raw,
                    operation_name: name,
                })
                .map_err(|_| ApiError::body("graphql_variables"))?;
                PreparedBody::Bytes {
                    data: payload.into(),
                    content_type: Some(json),
                }
            } else {
                PreparedBody::None
            };
            (body, Some(Graphql { params, post }))
        }
    })
}

/// 사용자 행 중 본문이 정하는 헤더(multipart·GraphQL은 Content-Type까지)는 보내지 않는다.
pub(crate) fn derived(body: &PreparedBody, graphql: bool, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(name.as_str(), "content-length" | "transfer-encoding")
        || (name == "content-type" && (graphql || matches!(body, PreparedBody::Multipart(_))))
}

async fn stream(path: &std::path::Path, index: u32) -> Result<reqwest::Body, ApiError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| ApiError::FileMissing { part: index })?;
    Ok(reqwest::Body::wrap_stream(
        tokio_util::io::ReaderStream::new(file),
    ))
}

pub(crate) async fn apply(
    mut builder: reqwest::RequestBuilder,
    body: &PreparedBody,
    user_content_type: bool,
) -> Result<reqwest::RequestBuilder, ApiError> {
    Ok(match body {
        PreparedBody::None => builder,
        PreparedBody::Bytes { data, content_type } => {
            if let Some(ct) = content_type.as_ref().filter(|_| !user_content_type) {
                builder = builder.header(CONTENT_TYPE, ct.clone());
            }
            builder.body(data.clone())
        }
        PreparedBody::File {
            path,
            len,
            content_type,
        } => {
            if !user_content_type {
                builder = builder.header(CONTENT_TYPE, content_type.clone());
            }
            builder
                .header(CONTENT_LENGTH, *len)
                .body(stream(path, 0).await?)
        }
        PreparedBody::Multipart(parts) => {
            let mut form = reqwest::multipart::Form::new();
            for (index, part) in parts.iter().enumerate() {
                let (name, mut built, ct) = match part {
                    PreparedPart::Text {
                        name,
                        value,
                        content_type,
                    } => (
                        name,
                        reqwest::multipart::Part::text(value.clone()),
                        content_type,
                    ),
                    PreparedPart::File {
                        name,
                        path,
                        file_name,
                        content_type,
                        len,
                    } => (
                        name,
                        reqwest::multipart::Part::stream_with_length(
                            stream(path, index as u32).await?,
                            *len,
                        )
                        .file_name(file_name.clone()),
                        content_type,
                    ),
                };
                if let Some(ct) = ct {
                    built = built
                        .mime_str(ct)
                        .map_err(|_| ApiError::body("content_type"))?;
                }
                form = form.part(name.clone(), built);
            }
            builder.multipart(form)
        }
    })
}

#[cfg(test)]
mod raw_encoding_tests {
    use super::*;
    #[test]
    fn raw_base64_preserves_bytes_and_content_type() {
        let body: Body = serde_json::from_str(r#"{"kind":"raw","text":"AP+A","encoding":"base64","contentType":"application/octet-stream"}"#).unwrap();
        let (PreparedBody::Bytes { data, content_type }, _) = prepare(&body).unwrap() else {
            panic!("bytes");
        };
        assert_eq!(data.as_ref(), &[0, 255, 128]);
        assert_eq!(content_type.unwrap(), "application/octet-stream");
    }
    #[test]
    fn invalid_base64_is_rejected_and_raw_default_is_unchanged() {
        let invalid: Body = serde_json::from_str(
            r#"{"kind":"raw","text":"!","encoding":"base64","contentType":null}"#,
        )
        .unwrap();
        assert!(prepare(&invalid).is_err());
        let plain: Body =
            serde_json::from_str(r#"{"kind":"raw","text":"한글","contentType":null}"#).unwrap();
        let (PreparedBody::Bytes { data, .. }, _) = prepare(&plain).unwrap() else {
            panic!("bytes");
        };
        assert_eq!(data.as_ref(), "한글".as_bytes());
    }
}
