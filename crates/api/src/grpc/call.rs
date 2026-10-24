//! 호출: 서술자에서 메서드를 찾고 요청 JSON을 동적 메시지로 만든 뒤(연결 전에 모두 검사),
//! unary·server streaming을 같은 server-streaming 경로로 부른다(전송 형식이 같다). 화면이 스트림을 닫으면
//! 호출 future를 버려 HTTP/2 스트림을 취소한다.
use super::codec::DynamicCodec;
use super::{GrpcEvent, MAX_MESSAGE_BYTES, MAX_STREAM_MESSAGES, Prepared, describe, metadata};
use crate::ApiError;
use crate::redact::Redactor;
use devbox_protocol::stream::StreamTx;
use prost::Message;
use prost_reflect::{DeserializeOptions, DynamicMessage, MessageDescriptor};
use std::time::Duration;

const MAX_METHOD_BYTES: usize = 1024;
const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct Job {
    channel: tonic::transport::Channel,
    metadata: tonic::metadata::MetadataMap,
    path: tonic::codegen::http::uri::PathAndQuery,
    input: DynamicMessage,
    output: MessageDescriptor,
    timeout: Duration,
    redactor: Redactor,
}

/// 요청 JSON → 동적 메시지. 모르는 필드는 거절하고, 오류 문장(값이 들어 있을 수 있음)은 버린다.
pub(crate) fn message(desc: MessageDescriptor, json: &str) -> Result<DynamicMessage, ApiError> {
    if json.len() > MAX_JSON_BYTES {
        return Err(crate::validate::too_large(MAX_JSON_BYTES));
    }
    let mut de = serde_json::Deserializer::from_str(json);
    let options = DeserializeOptions::new().deny_unknown_fields(true);
    let message = DynamicMessage::deserialize_with_options(desc, &mut de, &options)
        .map_err(|_| ApiError::grpc("message_json"))?;
    de.end().map_err(|_| ApiError::grpc("message_json"))?;
    if message.encoded_len() > MAX_MESSAGE_BYTES {
        return Err(crate::validate::too_large(MAX_MESSAGE_BYTES));
    }
    Ok(message)
}

pub(crate) async fn prepare(
    prepared: Prepared,
    method: &str,
    json: &str,
    timeout: Duration,
) -> Result<Job, ApiError> {
    if method.len() > MAX_METHOD_BYTES {
        return Err(ApiError::grpc("method_not_found"));
    }
    let (service_name, method_name) = method
        .split_once('/')
        .ok_or_else(|| ApiError::grpc("method_not_found"))?;
    let (pool, _) = tokio::time::timeout(super::DESCRIBE_TIMEOUT, describe::pool(&prepared))
        .await
        .map_err(|_| ApiError::network("timeout"))??;
    let found = pool
        .get_service_by_name(service_name)
        .and_then(|s| s.methods().find(|m| m.name() == method_name))
        .ok_or_else(|| ApiError::grpc("method_not_found"))?;
    if found.is_client_streaming() {
        return Err(ApiError::grpc("client_streaming"));
    }
    let input = message(found.input(), json)?;
    let path = format!("/{service_name}/{method_name}")
        .parse()
        .map_err(|_| ApiError::grpc("method_not_found"))?;
    Ok(Job {
        channel: super::connect::channel(&prepared)?,
        metadata: prepared.metadata,
        path,
        input,
        output: found.output(),
        timeout,
        redactor: Redactor::new(&prepared.values),
    })
}

async fn write(tx: &StreamTx, event: &GrpcEvent) -> Result<(), crate::events::Closed> {
    let mut line = serde_json::to_vec(event).map_err(|_| crate::events::Closed)?;
    line.push(b'\n');
    tx.write(&line).await.map_err(|_| crate::events::Closed)
}

fn json_of(message: &DynamicMessage, redactor: &Redactor) -> String {
    let text = serde_json::to_string(message).unwrap_or_else(|_| "{}".into());
    redactor.text(&text)
}

/// 서버 상태 → `Trailers`, 연결 계층 실패 → `Failed`.
fn ended(status: &tonic::Status, redactor: &Redactor) -> GrpcEvent {
    match describe::transport(status) {
        Some(error) => GrpcEvent::Failed {
            error: devbox_protocol::to_wire(&error),
        },
        None => GrpcEvent::Trailers {
            code: status.code() as i32,
            message: redactor.text(status.message()),
            metadata: metadata::pairs(status.metadata(), redactor),
        },
    }
}

async fn exchange(job: Job, tx: &StreamTx) -> Result<&'static str, crate::events::Closed> {
    let redactor = job.redactor.clone();
    let mut grpc = tonic::client::Grpc::new(job.channel)
        .max_decoding_message_size(MAX_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_MESSAGE_BYTES);
    if let Err(error) = grpc.ready().await {
        let status = tonic::Status::from_error(Box::new(error));
        write(tx, &ended(&status, &redactor)).await?;
        return Ok("failed");
    }
    let mut request = tonic::Request::new(job.input);
    *request.metadata_mut() = job.metadata;
    request.set_timeout(job.timeout);
    let codec = DynamicCodec { output: job.output };
    let response = match grpc.server_streaming(request, job.path, codec).await {
        Ok(response) => response,
        Err(status) => {
            write(tx, &ended(&status, &redactor)).await?;
            return Ok("failed");
        }
    };
    write(
        tx,
        &GrpcEvent::Headers {
            metadata: metadata::pairs(response.metadata(), &redactor),
        },
    )
    .await?;
    let mut stream = response.into_inner();
    let mut count = 0;
    loop {
        match stream.message().await {
            Ok(Some(message)) => {
                count += 1;
                if count > MAX_STREAM_MESSAGES {
                    let error = crate::validate::too_large(MAX_STREAM_MESSAGES);
                    write(
                        tx,
                        &GrpcEvent::Failed {
                            error: devbox_protocol::to_wire(&error),
                        },
                    )
                    .await?;
                    return Ok("failed");
                }
                write(
                    tx,
                    &GrpcEvent::Message {
                        json: json_of(&message, &redactor),
                    },
                )
                .await?;
            }
            Ok(None) => {
                let trailers = stream.trailers().await.ok().flatten().unwrap_or_default();
                write(
                    tx,
                    &GrpcEvent::Trailers {
                        code: 0,
                        message: String::new(),
                        metadata: metadata::pairs(&trailers, &redactor),
                    },
                )
                .await?;
                return Ok("ok");
            }
            Err(status) => {
                write(tx, &ended(&status, &redactor)).await?;
                return Ok("failed");
            }
        }
    }
}

pub(crate) async fn run(job: Job, tx: StreamTx, permit: tokio::sync::OwnedSemaphorePermit) {
    let started = std::time::Instant::now();
    let closed = tx.closed();
    let timeout = job.timeout;
    let outcome = tokio::select! {
        biased;
        _ = closed.cancelled() => "cancelled",
        r = tokio::time::timeout(timeout, exchange(job, &tx)) => match r {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => "cancelled",
            Err(_) => {
                let error = ApiError::network("timeout");
                let _ = write(&tx, &GrpcEvent::Failed { error: devbox_protocol::to_wire(&error) }).await;
                "failed"
            }
        },
    };
    drop(permit);
    tx.finish().await;
    tracing::debug!(
        method = "api.grpc_call",
        elapsed_ms = started.elapsed().as_millis() as u64,
        outcome
    );
}
