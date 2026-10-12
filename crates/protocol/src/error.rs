use crate::message::WireError;
use serde::Serialize;

/// `#[serde(tag = "code", content = "detail")]` enum을 만든다.
/// 세부 필드 이름은 한 단어 소문자로 쓴다(TS와 그대로 맞추기 위해).
#[macro_export]
macro_rules! domain_error {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $( $variant:ident $( { $($field:ident : $fty:ty),* $(,)? } )? = $code:literal ),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::ts_rs::TS, ::thiserror::Error)]
        #[serde(tag = "code", content = "detail")]
        $vis enum $name {
            $( #[serde(rename = $code)] #[error($code)] $variant $( { $($field: $fty),* } )? ),+
        }
        impl $crate::method::DomainError for $name {
            const CODES: &'static [&'static str] = &[$($code),+];
        }
    };
}

domain_error! {
    pub enum CommonError {
        Internal = "internal",
        InvalidParams { reason: String } = "invalid_params",
        MethodNotFound { method: String } = "method_not_found",
        HelloRequired = "hello_required",
        Cancelled = "cancelled",
        DaemonRestarting = "daemon_restarting",
        NotFound = "not_found",
        SecretsLocked = "secrets_locked",
    }
}

/// 핸들러 실패: 도메인 오류이거나, 로그로만 남길 내부 오류.
#[derive(Debug)]
pub enum Fail<E> {
    Domain(E),
    Internal(String),
}

impl<E: crate::method::DomainError> From<E> for Fail<E> {
    fn from(e: E) -> Self {
        Fail::Domain(e)
    }
}

/// `io·sqlite` 같은 하부 오류를 내부 오류로 바꾼다: `x.internal("read projects")?`
pub trait Internal<T> {
    fn internal<E>(self, what: &'static str) -> Result<T, Fail<E>>;
}

impl<T, X: std::fmt::Display> Internal<T> for Result<T, X> {
    fn internal<E>(self, what: &'static str) -> Result<T, Fail<E>> {
        self.map_err(|e| Fail::Internal(format!("{what}: {e}")))
    }
}

pub fn to_wire<E: Serialize>(e: &E) -> WireError {
    let raw = serde_json::to_string(e).expect("domain errors always serialize");
    serde_json::from_str(&raw).expect("domain errors use the wire error schema")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DomainError, Internal};

    #[test]
    fn wire_conversion_preserves_raw_detail_and_null() {
        #[derive(Serialize)]
        struct Error<'a> {
            code: &'a str,
            detail: &'a serde_json::value::RawValue,
        }
        for json in ["null", r#"{"n":9007199254740993}"#] {
            let raw =
                serde_json::value::RawValue::from_string(json.into()).expect("encode fixture JSON");
            let error = to_wire(&Error {
                code: "test.raw",
                detail: &raw,
            });
            assert_eq!(
                error.detail.expect("fixture error includes details").get(),
                json
            );
        }
        assert_eq!(
            CommonError::CODES,
            &[
                "internal",
                "invalid_params",
                "method_not_found",
                "hello_required",
                "cancelled",
                "daemon_restarting",
                "not_found",
                "secrets_locked"
            ]
        );
        assert!(to_wire(&CommonError::Cancelled).detail.is_none());
    }

    #[test]
    fn lower_level_errors_are_contextual_internal_failures() {
        let error: Result<(), std::io::Error> = Err(std::io::Error::other("private cause"));
        let converted: Result<(), Fail<CommonError>> = error.internal("read fixture");
        match converted.expect_err("fixture operation must fail") {
            Fail::Internal(message) => assert_eq!(message, "read fixture: private cause"),
            Fail::Domain(_) => panic!("lower-level failures must be internal"),
        }
    }
}
