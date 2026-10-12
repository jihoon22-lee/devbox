// crates/protocol/src/lib.rs (최종)
//! devbox 데몬과 클라이언트 사이의 프레임·메시지·메서드 계약.
pub mod error;
pub mod frame;
pub mod message;
pub mod method;
pub mod router;
pub mod system;

pub use error::{CommonError, Fail, Internal, to_wire};
pub use method::{Confirm, DomainError, Done, Exporter, Method, MethodKind, NameCollector, Topic};
pub use router::{Ctx, DispatchError, Router};
pub use ts_rs;

pub const PROTOCOL_VERSION: u32 = 1;
