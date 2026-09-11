use crate::ids::{EnvClassId, EnvironmentId};
use crate::model::provenance::ToolInfo;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvironmentFingerprint {
    pub id: EnvironmentId,
    pub class: EnvClassId,
    /// Short human label, e.g. `macos-aarch64-16cpu · fjall · release`.
    pub label: String,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub cpus: Option<u32>,
    pub cpu_model: Option<String>,
    pub runner: Option<String>,
    pub profile: Option<String>,
    pub compiler: Option<String>,
    pub tool: Option<ToolInfo>,
    pub workload: serde_json::Value,
    pub comparability_keys: Vec<String>,
    pub extra: serde_json::Value,
}

impl EnvironmentFingerprint {
    /// Build ids from the fingerprint: the class hashes only the comparability subset, the
    /// environment id hashes everything.
    pub fn build(
        os: Option<String>,
        arch: Option<String>,
        cpus: Option<u32>,
        cpu_model: Option<String>,
        runner: Option<String>,
        profile: Option<String>,
        compiler: Option<String>,
        tool: Option<ToolInfo>,
        workload: serde_json::Value,
        comparability_keys: Vec<String>,
        extra: serde_json::Value,
    ) -> Self {
        let class_json = serde_json::json!({
            "os": os, "arch": arch, "cpus": cpus, "profile": profile,
            "tool": tool.as_ref().map(|t| t.name.clone()), "workload": workload,
        });
        let full_json = serde_json::json!({
            "os": os, "arch": arch, "cpus": cpus, "cpu_model": cpu_model, "runner": runner,
            "profile": profile, "compiler": compiler, "tool": tool, "workload": workload, "extra": extra,
        });
        let class = EnvClassId::derive(&canonical_json(&class_json));
        let id = EnvironmentId::derive(&canonical_json(&full_json));
        let mut label_parts: Vec<String> = vec![];
        if let (Some(o), Some(a)) = (&os, &arch) {
            label_parts.push(match cpus {
                Some(c) => format!("{o}-{a}-{c}cpu"),
                None => format!("{o}-{a}"),
            });
        } else if let Some(r) = &runner {
            label_parts.push(r.clone());
        }
        if let Some(b) = workload.get("backend").and_then(|v| v.as_str()) {
            label_parts.push(b.to_string());
        }
        if let Some(p) = &profile {
            label_parts.push(p.clone());
        }
        let label = if label_parts.is_empty() {
            "unknown environment".into()
        } else {
            label_parts.join(" · ")
        };
        EnvironmentFingerprint {
            id,
            class,
            label,
            os,
            arch,
            cpus,
            cpu_model,
            runner,
            profile,
            compiler,
            tool,
            workload,
            comparability_keys,
            extra,
        }
    }
}

/// Canonical JSON: sorted object keys, no whitespace. serde_json's `Value` map is a BTreeMap
/// only with the `preserve_order` feature disabled, which is our configuration.
pub fn canonical_json(v: &serde_json::Value) -> String {
    fn write(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap());
                    out.push(':');
                    write(&m[*k], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(x, out);
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other).unwrap()),
        }
    }
    let mut s = String::new();
    write(v, &mut s);
    s
}
