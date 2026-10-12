use devbox_protocol::{CommonError, Confirm, DomainError, Exporter, Method, MethodKind, Topic};
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use ts_rs::{Config, TS, TypeVisitor};

pub struct TsExporter {
    cfg: Config,
    seen: HashSet<TypeId>,
    decls: BTreeMap<String, String>,
    methods: BTreeMap<&'static str, String>,
    info: BTreeMap<&'static str, (MethodKind, Confirm)>,
    topics: BTreeMap<&'static str, String>,
    codes: BTreeSet<&'static str>,
    error: Option<String>,
}

fn kind_str(k: MethodKind) -> &'static str {
    match k {
        MethodKind::Query => "query",
        MethodKind::Mutation => "mutation",
        MethodKind::Subscription => "subscription",
        MethodKind::Stream => "stream",
    }
}
fn confirm_str(c: Confirm) -> &'static str {
    match c {
        Confirm::None => "none",
        Confirm::Always => "always",
        Confirm::Reversible => "reversible",
    }
}

impl TsExporter {
    pub fn new() -> Self {
        let mut x = Self {
            cfg: Config::new().with_large_int("number"),
            seen: HashSet::new(),
            decls: BTreeMap::new(),
            methods: BTreeMap::new(),
            info: BTreeMap::new(),
            topics: BTreeMap::new(),
            codes: BTreeSet::new(),
            error: None,
        };
        x.error_type::<CommonError>();
        x
    }

    fn error_type<E: DomainError>(&mut self) {
        self.codes.extend(E::CODES.iter().copied());
        self.visit::<E>();
    }

    fn name<T: TS + 'static + ?Sized>(&self) -> String {
        T::name(&self.cfg)
    }

    pub fn render(&self) -> anyhow::Result<String> {
        if let Some(e) = &self.error {
            anyhow::bail!(e.clone());
        }
        let mut out = String::from(
            "// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.\n/* eslint-disable */\n\n",
        );
        for decl in self.decls.values() {
            out.push_str("export ");
            out.push_str(decl.trim_end());
            out.push('\n');
        }
        out.push_str("\nexport interface Methods {\n");
        for (name, line) in &self.methods {
            out.push_str(&format!("  \"{name}\": {line};\n"));
        }
        out.push_str("}\n\nexport interface Topics {\n");
        for (name, payload) in &self.topics {
            out.push_str(&format!("  \"{name}\": {payload};\n"));
        }
        out.push_str("}\n\nexport const METHOD_INFO = {\n");
        for (name, (k, c)) in &self.info {
            out.push_str(&format!(
                "  \"{name}\": {{ kind: \"{}\", confirm: \"{}\" }},\n",
                kind_str(*k),
                confirm_str(*c)
            ));
        }
        out.push_str("} as const;\n\nexport const RPC_ERROR_CODES = [\n");
        for code in &self.codes {
            out.push_str(&format!("  \"{code}\",\n"));
        }
        out.push_str("] as const;\nexport type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];\n");
        Ok(out)
    }
}

impl Exporter for TsExporter {
    fn method<M: Method>(&mut self) {
        self.visit::<M::Params>();
        self.visit::<M::Output>();
        self.error_type::<M::Error>();
        let line = format!(
            "{{ kind: \"{}\"; confirm: \"{}\"; params: {}; result: {}; error: {} }}",
            kind_str(M::KIND),
            confirm_str(M::CONFIRM),
            self.name::<M::Params>(),
            self.name::<M::Output>(),
            self.name::<M::Error>()
        );
        if self.methods.insert(M::NAME, line).is_some() {
            self.error = Some(format!("method {} declared twice", M::NAME));
        }
        self.info.insert(M::NAME, (M::KIND, M::CONFIRM));
    }

    fn topic<T: Topic>(&mut self) {
        self.visit::<T::Payload>();
        if self
            .topics
            .insert(T::NAME, self.name::<T::Payload>())
            .is_some()
        {
            self.error = Some(format!("topic {} declared twice", T::NAME));
        }
    }
}

impl TypeVisitor for TsExporter {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if self.error.is_some() || !self.seen.insert(TypeId::of::<T>()) {
            return;
        }
        if T::output_path().is_some() {
            let ident = T::ident(&self.cfg);
            let decl = T::decl(&self.cfg);
            if let Some(prev) = self.decls.get(&ident) {
                if *prev != decl {
                    self.error = Some(format!("two different types are both named {ident}"));
                    return;
                }
            }
            self.decls.insert(ident, decl);
        }
        T::visit_dependencies(self);
        T::visit_generics(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_system_methods_types_and_error_codes() {
        let mut x = TsExporter::new();
        devbox_protocol::system::export(&mut x);
        let out = x.render().unwrap();
        assert!(out.starts_with("// 이 파일은 `pnpm gen`이 만든다. 직접 고치지 않는다.\n"));
        assert!(out.contains(
            "export type Pong = { daemonId: string, version: string, uptimeMs: number, };"
        ));
        assert!(out.contains("\"system.ping\": { kind: \"query\"; confirm: \"none\"; params: PingParams; result: Pong; error: SystemError };"));
        assert!(out.contains("\"internal\","));
        assert!(out.contains("\"system.unavailable\","));
        assert!(out.contains("export type RpcErrorCode = (typeof RPC_ERROR_CODES)[number];"));
    }

    #[derive(serde::Serialize, serde::Deserialize, ts_rs::TS)]
    #[serde(rename_all = "camelCase")]
    struct Leaf {
        elapsed_ms: i64,
    }
    #[derive(serde::Serialize, serde::Deserialize, ts_rs::TS)]
    struct Nested {
        values: Vec<Option<Leaf>>,
    }
    devbox_protocol::method!(NestedMethod = "test.nested", mutation(always), Nested => Nested, devbox_protocol::system::SystemError);
    devbox_protocol::topic!(Changed = "test.changed", Nested);

    #[test]
    fn traverses_nested_dependencies_and_renders_topics_and_confirmation() {
        let mut x = TsExporter::new();
        x.method::<NestedMethod>();
        x.topic::<Changed>();
        let out = x.render().unwrap();
        assert!(out.contains("export type Leaf = { elapsedMs: number, };"));
        assert!(out.contains("export type Nested = { values: Array<Leaf | null>, };"));
        assert!(out.contains("\"test.changed\": Nested;"));
        assert!(out.contains("kind: \"mutation\"; confirm: \"always\""));
        assert!(out.contains("\"test.nested\": { kind: \"mutation\", confirm: \"always\" }"));
    }

    #[test]
    fn rejects_duplicate_methods_and_topics() {
        let mut x = TsExporter::new();
        x.method::<NestedMethod>();
        x.method::<NestedMethod>();
        assert!(
            x.render()
                .unwrap_err()
                .to_string()
                .contains("method test.nested declared twice")
        );
        let mut x = TsExporter::new();
        x.topic::<Changed>();
        x.topic::<Changed>();
        assert!(
            x.render()
                .unwrap_err()
                .to_string()
                .contains("topic test.changed declared twice")
        );
    }

    #[derive(serde::Serialize, ts_rs::TS)]
    #[ts(rename = "Leaf")]
    struct ConflictingLeaf {
        text: String,
    }

    #[test]
    fn rejects_conflicting_type_identifiers() {
        let mut x = TsExporter::new();
        x.visit::<Nested>();
        x.visit::<ConflictingLeaf>();
        assert!(
            x.render()
                .unwrap_err()
                .to_string()
                .contains("both named Leaf")
        );
    }

    #[test]
    fn output_is_deterministic() {
        let mut first = TsExporter::new();
        first.method::<devbox_protocol::system::Ping>();
        first.method::<devbox_protocol::system::Info>();
        first.method::<NestedMethod>();
        first.topic::<Changed>();
        let mut reversed = TsExporter::new();
        reversed.topic::<Changed>();
        reversed.method::<NestedMethod>();
        reversed.method::<devbox_protocol::system::Info>();
        reversed.method::<devbox_protocol::system::Ping>();
        assert_eq!(first.render().unwrap(), reversed.render().unwrap());
    }
}
