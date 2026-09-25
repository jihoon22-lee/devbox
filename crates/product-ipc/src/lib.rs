//! Typed component calls retain the existing header/method/args wire shape.
use product_contract::RouteRequest;
use serde::{de::DeserializeOwned, Deserialize};

pub use ts_rs;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", bound = "C: DeserializeOwned")]
pub struct ComponentRequest<C> {
    pub header: RouteRequest,
    #[serde(flatten)]
    pub call: C,
}

/// Closed framework envelope. Decode the component enum inside native
/// admission so malformed methods/arguments still return a Problem and record
/// `rejected`; Tauri's blanket CommandArg deserialization would bypass both.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IncomingRequest {
    pub header: RouteRequest,
    pub method: String,
    pub args: serde_json::Value,
}

pub const MAX_ARGUMENT_BYTES: usize = 20 * 1024 * 1024;

impl IncomingRequest {
    pub fn decode<C: ComponentCall>(self) -> Result<ComponentRequest<C>, DecodeError> {
        if !self.args.is_object()
            || serde_json::to_vec(&self.args).map_or(true, |bytes| bytes.len() > MAX_ARGUMENT_BYTES)
        {
            return Err(DecodeError);
        }
        let call: C =
            serde_json::from_value(serde_json::json!({"method": self.method, "args": self.args}))
                .map_err(|_| DecodeError)?;
        // Accepted method spellings must be declared by the native enum.
        if call.method() != self.method {
            return Err(DecodeError);
        }
        Ok(ComponentRequest {
            header: self.header,
            call,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionClass {
    Normal,
    Control,
}

pub trait ComponentCall: DeserializeOwned + Send + 'static {
    const COMPONENT: &'static str;
    fn method(&self) -> &'static str;
    fn routes(&self) -> &'static [&'static str];
    fn class(&self) -> ExecutionClass {
        ExecutionClass::Normal
    }
}

pub fn results_map(type_name: &str, entries: &[(&str, String)]) -> String {
    let mut out = format!("export type {type_name} = {{\n");
    for (method, result) in entries {
        out.push_str(&format!("  {method}: {result};\n"));
    }
    out.push_str("};\n");
    out
}

#[macro_export]
macro_rules! issue_codes {
    ($vis:vis enum $name:ident { $($variant:ident = $code:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, $crate::ts_rs::TS)]
        $vis enum $name { $(#[serde(rename = $code)] $variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn code(self) -> &'static str { match self { $(Self::$variant => $code),+ } }
            pub fn from_code(code: &str) -> Option<Self> {
                match code { $($code => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

/// Export reachable DTOs once and reject distinct Rust types that would
/// overwrite the same generated TypeScript path.
pub struct TypeExporter<'a> {
    cfg: &'a ts_rs::Config,
    seen: std::collections::HashSet<std::any::TypeId>,
    files: std::collections::BTreeMap<std::path::PathBuf, (std::any::TypeId, String)>,
    error: Option<String>,
}
impl<'a> TypeExporter<'a> {
    pub fn new(cfg: &'a ts_rs::Config) -> Self {
        Self {
            cfg,
            seen: Default::default(),
            files: Default::default(),
            error: None,
        }
    }
    pub fn register<T: ts_rs::TS + 'static + ?Sized>(&mut self) -> Result<String, String> {
        <Self as ts_rs::TypeVisitor>::visit::<T>(self);
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(T::name(self.cfg)),
        }
    }
    pub fn results(&self, type_name: &str, entries: &[(&str, String)]) -> String {
        let body = results_map(type_name, entries);
        let tokens: std::collections::BTreeSet<_> = entries
            .iter()
            .flat_map(|(_, name)| name.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')))
            .collect();
        let mut out = String::new();
        for (path, (_, name)) in &self.files {
            if tokens.contains(name.as_str()) {
                let module = path.with_extension("").to_string_lossy().replace('\\', "/");
                out.push_str(&format!("import type {{ {name} }} from \"./{module}\";\n"));
            }
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&body);
        out
    }
}
impl ts_rs::TypeVisitor for TypeExporter<'_> {
    fn visit<T: ts_rs::TS + 'static + ?Sized>(&mut self) {
        if self.error.is_some() || !self.seen.insert(std::any::TypeId::of::<T>()) {
            return;
        }
        if let Some(path) = T::output_path() {
            let id = std::any::TypeId::of::<T>();
            let name = T::ident(self.cfg);
            if self
                .files
                .get(&path)
                .is_some_and(|(previous, _)| *previous != id)
            {
                self.error = Some(format!("generated type path collision: {}", path.display()));
                return;
            }
            self.files.insert(path, (id, name));
            if let Err(error) = T::export(self.cfg) {
                self.error = Some(error.to_string());
                return;
            }
        }
        T::visit_dependencies(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug, Deserialize, PartialEq, ts_rs::TS)]
    #[serde(
        tag = "method",
        content = "args",
        rename_all = "snake_case",
        deny_unknown_fields
    )]
    enum DemoCall {
        GetDay { date: String, day_start: i64 },
        StopTracking {},
    }
    impl ComponentCall for DemoCall {
        const COMPONENT: &'static str = "demo.activity";
        fn method(&self) -> &'static str {
            match self {
                Self::GetDay { .. } => "get_day",
                Self::StopTracking {} => "stop_tracking",
            }
        }
        fn routes(&self) -> &'static [&'static str] {
            &["activity"]
        }
    }
    issue_codes! { pub enum DemoIssue { Cancelled = "digest_cancelled", Unavailable = "unavailable" } }
    fn request(method: &str, args: serde_json::Value) -> serde_json::Value {
        let header: serde_json::Value = serde_json::from_str(include_str!(
            "../../../packages/product-shell/fixtures/route-request.json"
        ))
        .unwrap();
        serde_json::json!({"header": header, "method": method, "args": args})
    }
    #[test]
    fn requests_keep_the_existing_wire_shape() {
        let value = request(
            "get_day",
            serde_json::json!({"date":"2026-09-24","day_start":0}),
        );
        let parsed: ComponentRequest<DemoCall> = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            parsed.call,
            DemoCall::GetDay {
                date: "2026-09-24".into(),
                day_start: 0
            }
        );
        let incoming: IncomingRequest = serde_json::from_value(value).unwrap();
        assert_eq!(incoming.decode::<DemoCall>().unwrap().call, parsed.call);
    }
    #[test]
    fn invalid_calls_reach_the_native_rejection_boundary_without_becoming_valid_calls() {
        for value in [
            request("drop_tables", serde_json::json!({})),
            request("get_day", serde_json::json!({"date":1})),
            request("stop_tracking", serde_json::json!([])),
            request("stop_tracking", serde_json::json!({"unexpected":true})),
        ] {
            let incoming: IncomingRequest = serde_json::from_value(value).unwrap();
            assert!(incoming.decode::<DemoCall>().is_err());
        }
        let mut extra = request("stop_tracking", serde_json::json!({}));
        extra["component"] = "another.owner".into();
        assert!(serde_json::from_value::<IncomingRequest>(extra).is_err());
    }
    #[test]
    fn oversized_arguments_are_refused_before_dispatch() {
        let value = request(
            "get_day",
            serde_json::json!({"date":"x".repeat(MAX_ARGUMENT_BYTES),"day_start":0}),
        );
        assert!(serde_json::from_value::<IncomingRequest>(value)
            .unwrap()
            .decode::<DemoCall>()
            .is_err());
    }
    #[test]
    fn issue_codes_round_trip_and_export_a_literal_union() {
        assert_eq!(DemoIssue::Cancelled.code(), "digest_cancelled");
        assert_eq!(
            DemoIssue::from_code("unavailable"),
            Some(DemoIssue::Unavailable)
        );
        assert_eq!(DemoIssue::from_code("other"), None);
        assert_eq!(DemoIssue::ALL.len(), 2);
        let decl = <DemoIssue as ts_rs::TS>::decl(&ts_rs::Config::new());
        assert!(
            decl.contains("\"digest_cancelled\"") && decl.contains("\"unavailable\""),
            "{decl}"
        );
    }
    #[test]
    fn results_map_lists_each_method() {
        assert_eq!(
            results_map(
                "DemoResults",
                &[
                    ("get_day", "DaySummary".into()),
                    ("stop_tracking", "null".into())
                ]
            ),
            "export type DemoResults = {\n  get_day: DaySummary;\n  stop_tracking: null;\n};\n"
        );
    }
}
