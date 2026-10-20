pub mod crypto;
pub mod keystore;
mod model;
mod service;
mod store;
use devbox_protocol::{Exporter, method};
pub use model::*;
pub use service::SecretsService;
pub use store::MIGRATIONS;
method!(pub List = "secrets.list", query, Empty => Vec<SecretInfo>, SecretsError);
method!(pub Set = "secrets.set", mutation, SetSecret => devbox_protocol::Done, SecretsError);
method!(pub Delete = "secrets.delete", mutation(always), Name => devbox_protocol::Done, SecretsError);
method!(pub ExportKey = "secrets.export_key", query, Empty => Recovery, SecretsError);
method!(pub ImportKey = "secrets.import_key", mutation, Recovery => devbox_protocol::Done, SecretsError);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<List>();
    x.method::<Set>();
    x.method::<Delete>();
    x.method::<ExportKey>();
    x.method::<ImportKey>();
}
pub type R<T> = Result<T, devbox_protocol::Fail<SecretsError>>;
#[cfg(test)]
mod crypto_tests;
#[cfg(test)]
mod store_tests;

#[cfg(test)]
mod keystore_tests;
