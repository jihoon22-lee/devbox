//! 파일 순회와 디렉터리 제외 규칙을 제공하는 공용 크레이트.
//!
//! - [`is_ignored_dir`]: gitignore 스타일로 흔히 제외되는 디렉터리 이름 판정
//! - [`collect`] / [`IndexedFile`]: 루트 디렉터리를 순회하며 제외 규칙을 적용한
//!   파일 목록 수집
//!
//! 이 크레이트가 담지 않는 것:
//! - **watcher (파일 변경 감시)**: 아직 실제 소비자가 없다. `notify` 등으로
//!   구현할 필요가 생기면 그때 추가한다 (`CONVENTIONS.md`의 "두 번 이상
//!   실제로 필요해진 코드만 추출" 원칙).
//! - **"내용을 인덱싱할 파일인가" 판단** (텍스트 확장자, 최대 크기 등): 이는
//!   순회된 파일을 어떻게 쓸지 결정하는 소비자 고유의 관심사다. 예를 들어
//!   everything-plus는 내용 검색을 위해 텍스트 확장자만 골라 읽지만,
//!   code-pad 같은 다른 소비자는 다른 기준을 쓸 수 있다. 각 소비자가 직접
//!   구현한다.

pub mod ignore;
pub mod walk;

pub use ignore::is_ignored_dir;
pub use walk::{collect, IndexedFile};
