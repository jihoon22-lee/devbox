use serde::{Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MethodKind {
    Query,
    Mutation,
    Subscription,
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Confirm {
    None,
    Always,
    Reversible,
}

/// 도메인 오류 enum. `domain_error!`가 구현한다.
pub trait DomainError: std::error::Error + Serialize + ts_rs::TS + Send + Sync + 'static {
    const CODES: &'static [&'static str];
}

pub trait Method: 'static {
    const NAME: &'static str;
    const KIND: MethodKind;
    const CONFIRM: Confirm;
    type Params: DeserializeOwned + ts_rs::TS + Send + 'static;
    type Output: Serialize + ts_rs::TS + Send + 'static;
    type Error: DomainError;
}

pub trait Topic: 'static {
    const NAME: &'static str;
    type Payload: Serialize + ts_rs::TS + Send + Sync + 'static;
}

/// 모든 mutation이 결과가 따로 없을 때 돌려주는 값.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize, ts_rs::TS)]
pub struct Done {
    pub rev: String,
}

/// 선언된 메서드·주제를 순회하는 쪽(TS 생성기, 라우터 완전성 시험)이 구현한다.
pub trait Exporter {
    fn method<M: Method>(&mut self);
    fn topic<T: Topic>(&mut self);
}

#[derive(Default, Debug)]
pub struct NameCollector {
    pub methods: Vec<&'static str>,
    pub topics: Vec<&'static str>,
}

impl Exporter for NameCollector {
    fn method<M: Method>(&mut self) {
        self.methods.push(M::NAME);
    }
    fn topic<T: Topic>(&mut self) {
        self.topics.push(T::NAME);
    }
}

#[macro_export]
macro_rules! method {
    ($vis:vis $ty:ident = $name:literal, $kind:ident $( ( $confirm:ident ) )?, $params:ty => $output:ty, $error:ty) => {
        $vis struct $ty;
        impl $crate::method::Method for $ty {
            const NAME: &'static str = $name;
            const KIND: $crate::method::MethodKind = $crate::__method_kind!($kind);
            const CONFIRM: $crate::method::Confirm = $crate::__method_confirm!($($confirm)?);
            type Params = $params;
            type Output = $output;
            type Error = $error;
        }
    };
}

#[macro_export]
macro_rules! topic {
    ($vis:vis $ty:ident = $name:literal, $payload:ty) => {
        $vis struct $ty;
        impl $crate::method::Topic for $ty {
            const NAME: &'static str = $name;
            type Payload = $payload;
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __method_kind {
    (query) => {
        $crate::method::MethodKind::Query
    };
    (mutation) => {
        $crate::method::MethodKind::Mutation
    };
    (subscription) => {
        $crate::method::MethodKind::Subscription
    };
    (stream) => {
        $crate::method::MethodKind::Stream
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __method_confirm {
    () => {
        $crate::method::Confirm::None
    };
    (none) => {
        $crate::method::Confirm::None
    };
    (always) => {
        $crate::method::Confirm::Always
    };
    (reversible) => {
        $crate::method::Confirm::Reversible
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{PingParams, Pong, SystemError};
    use ts_rs::TS;

    crate::method!(Undo = "test.undo", mutation(reversible), PingParams => Pong, SystemError);
    crate::method!(Watch = "test.watch", subscription, PingParams => Pong, SystemError);
    crate::method!(Read = "test.read", stream, PingParams => Pong, SystemError);
    crate::topic!(Changed = "test.changed", Done);

    #[test]
    fn declarations_expose_metadata_and_export_types() {
        assert_eq!(Undo::CONFIRM, Confirm::Reversible);
        assert_eq!(Undo::KIND, MethodKind::Mutation);
        assert_eq!(Watch::KIND, MethodKind::Subscription);
        assert_eq!(Read::KIND, MethodKind::Stream);
        assert_eq!(Read::CONFIRM, Confirm::None);
        let mut names = NameCollector::default();
        crate::system::export(&mut names);
        names.method::<Undo>();
        names.topic::<Changed>();
        assert_eq!(
            names.methods,
            [
                "system.ping",
                "system.info",
                "system.doctor",
                "system.refresh_env",
                "test.undo"
            ]
        );
        assert_eq!(names.topics, ["test.changed"]);
        let config = ts_rs::Config::new().with_large_int("number");
        assert!(Done::decl(&config).contains("rev: string"));
        assert!(Pong::decl(&config).contains("uptimeMs: number"));
        assert!(crate::system::DaemonInfo::decl(&config).contains("startedMs: number"));
        assert!(SystemError::decl(&config).contains("system.unavailable"));
        assert_eq!(
            serde_json::to_string(&Done {
                rev: "9007199254740993".into()
            })
            .expect("serialize fixture JSON"),
            r#"{"rev":"9007199254740993"}"#
        );
    }
}
