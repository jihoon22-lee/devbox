use crate::{
    service::{Inner, now_ms},
    *,
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Semaphore},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
const BODY_LIMIT: usize = 16 * 1024 * 1024;
#[derive(Default)]
struct Rate {
    start: i64,
    count: usize,
}
impl Rate {
    fn admit(&mut self, now: i64) -> bool {
        if now < self.start || now - self.start >= 1000 {
            self.start = now;
            self.count = 0;
        }
        if self.count >= 120 {
            return false;
        }
        self.count += 1;
        true
    }
}
pub(crate) async fn serve(
    socket: TcpListener,
    inner: Arc<Inner>,
    id: String,
    cancel: CancellationToken,
) {
    let mut tasks = JoinSet::new();
    let slots = Arc::new(Semaphore::new(16));
    let rate = Arc::new(Mutex::new(Rate::default()));
    loop {
        tokio::select! {
            _=cancel.cancelled()=>break,
            Some(_)=tasks.join_next(),if !tasks.is_empty()=>{},
            accepted=socket.accept()=> {
                let Ok((stream,_))=accepted else { break; };
                let Ok(permit)=slots.clone().try_acquire_owned() else { drop(stream);continue; };
                let inner=inner.clone();let id=id.clone();let rate=rate.clone();
                tasks.spawn(async move {
                    let _permit=permit;
                    let service=service_fn(move |req| receive(inner.clone(),id.clone(),rate.clone(),req));
                    let mut builder=http1::Builder::new();
                    builder.max_buf_size(64*1024).max_headers(100).timer(hyper_util::rt::TokioTimer::new()).header_read_timeout(Duration::from_secs(10));
                    let _=tokio::time::timeout(Duration::from_secs(90),builder.serve_connection(TokioIo::new(stream),service)).await;
                });
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}
fn response(status: u16, body: impl Into<Bytes>) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(body.into()));
    *r.status_mut() =
        hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    r
}
async fn receive(
    inner: Arc<Inner>,
    id: String,
    rate: Arc<Mutex<Rate>>,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let header_bytes = req
        .headers()
        .iter()
        .map(|(n, v)| n.as_str().len() + v.as_bytes().len() + 4)
        .sum::<usize>();
    if header_bytes > 64 * 1024 {
        return Ok(response(431, ""));
    }
    if !rate.lock().await.admit(now_ms()) {
        return Ok(response(429, ""));
    }
    if req
        .headers()
        .get(hyper::header::EXPECT)
        .is_some_and(|v| v.as_bytes() != b"100-continue")
    {
        return Ok(response(417, ""));
    }
    let method = req.method().to_string();
    let path = req.uri().path().to_owned();
    let query = req.uri().query().unwrap_or("").to_owned();
    let headers = req
        .headers()
        .iter()
        .map(|(n, v)| {
            (
                n.to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let mut incoming = req.into_body();
    let mut body = Vec::new();
    let mut truncated = false;
    let mut observed_size = 0usize;
    let read = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(frame) = incoming.frame().await {
            let frame = frame.map_err(|_| ())?;
            if let Ok(data) = frame.into_data() {
                observed_size = observed_size.saturating_add(data.len());
                if observed_size > BODY_LIMIT {
                    truncated = true;
                    body.clear();
                    break;
                }
                body.extend_from_slice(&data);
            }
        }
        Ok::<_, ()>(())
    })
    .await;
    match read {
        Err(_) => return Ok(response(408, "")),
        Ok(Err(_)) => return Ok(response(400, "")),
        _ => {}
    }
    let detail = ReceivedDetail {
        received: Received {
            listener_id: id.clone(),
            id: uuid::Uuid::new_v4().to_string(),
            method: method.clone(),
            path: path.clone(),
            size_bytes: observed_size.min(u32::MAX as usize) as u32,
            at_ms: now_ms() as f64,
            truncated,
        },
        query,
        headers,
        body_base64: String::new(),
        preview_truncated: false,
    };
    if inner.record(detail, &body).await.is_err() {
        return Ok(response(500, ""));
    }
    if truncated {
        let mut r = response(413, "");
        r.headers_mut().insert(
            hyper::header::CONNECTION,
            hyper::header::HeaderValue::from_static("close"),
        );
        return Ok(r);
    }
    let step = {
        let _guard = inner.rule_lock.lock().await;
        let Ok(rules) = inner.rules(id.clone()).await else {
            return Ok(response(500, ""));
        };
        match crate::rules::select_matching_rule(rules.items.iter(), &method, &path) {
            Some(rule) => Some(
                inner
                    .sequences
                    .lock()
                    .await
                    .entry(id)
                    .or_default()
                    .next_response(rule),
            ),
            None => None,
        }
    };
    let Some(step) = step else {
        return Ok(response(200, ""));
    };
    if step.delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
    }
    let no_body = method == "HEAD" || step.status < 200 || matches!(step.status, 204 | 304);
    let mut result = response(step.status, if no_body { String::new() } else { step.body });
    for (name, value) in step.headers {
        if let (Ok(n), Ok(v)) = (
            hyper::header::HeaderName::from_bytes(name.as_bytes()),
            hyper::header::HeaderValue::from_str(&value),
        ) {
            result.headers_mut().append(n, v);
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_rate_is_bounded_per_fixed_window() {
        let mut r = Rate::default();
        for _ in 0..120 {
            assert!(r.admit(100));
        }
        assert!(!r.admit(999));
        assert!(r.admit(1000));
    }
    #[test]
    fn request_rate_recovers_when_the_wall_clock_moves_backwards() {
        let mut r = Rate::default();
        for _ in 0..120 {
            assert!(r.admit(2000));
        }
        assert!(r.admit(1000));
    }
}
