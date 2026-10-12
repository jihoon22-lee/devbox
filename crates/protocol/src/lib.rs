//! devbox 데몬과 클라이언트 사이의 프레임·메시지·메서드 계약.

/// 클라이언트와 데몬이 같은 빌드에서 나왔는지 확인하는 프로토콜 번호.
pub const PROTOCOL_VERSION: u32 = 1;

pub mod frame;

pub mod message;
