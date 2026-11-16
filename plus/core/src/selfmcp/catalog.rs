use serde_json::{json, Map, Value};
use toolport_plus_api::Host;

use super::{backend, ToolError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    None,
    Always,
    UnlessDryRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Str,
    Bool,
    Obj,
    StrList,
    Int,
}

pub struct Param {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    pub desc: &'static str,
    pub default: Option<bool>,
}

pub type Runner = fn(&dyn Host, &Value) -> Result<Value, ToolError>;

pub struct ToolDef {
    pub name: &'static str,
    pub tier: u8,
    pub gate: Gate,
    pub description: &'static str,
    pub params: &'static [Param],
    pub run: Runner,
}

pub struct ResourceDef {
    pub uri: &'static str,
    pub name: &'static str,
    pub mime: &'static str,
    pub description: &'static str,
}

#[allow(dead_code)]
pub const fn p(name: &'static str, ty: Ty, required: bool, desc: &'static str) -> Param {
    Param {
        name,
        ty,
        required,
        desc,
        default: None,
    }
}

#[allow(dead_code)]
pub const fn on_by_default(param: Param) -> Param {
    Param {
        default: Some(true),
        ..param
    }
}

macro_rules! tool {
    ($name:literal, $tier:literal, $gate:ident, $desc:literal, [$($param:expr),* $(,)?], $run:path) => {
        ToolDef {
            name: $name,
            tier: $tier,
            gate: Gate::$gate,
            description: $desc,
            params: &[$($param),*],
            run: $run,
        }
    };
}

pub const TOOLS: &[ToolDef] = &[
    tool!(
        "where_am_i",
        1,
        None,
        "Absolute paths and state of this installation",
        [],
        backend::where_am_i
    ),
    tool!(
        "doctor",
        1,
        None,
        "Read-only health checks",
        [],
        backend::doctor
    ),
    tool!(
        "flow_diagram",
        1,
        None,
        "Data-flow diagram of skills and servers",
        [],
        backend::flow_diagram
    ),
];

pub const RESOURCES: &[ResourceDef] = &[
    ResourceDef {
        uri: "mcpm://paths",
        name: "paths",
        mime: "application/json",
        description: "Canonical, sync and output roots",
    },
    ResourceDef {
        uri: "mcpm://status",
        name: "status",
        mime: "application/json",
        description: "Registry summary and drift counts",
    },
    ResourceDef {
        uri: "mcpm://flow",
        name: "flow",
        mime: "text/markdown",
        description: "Data-flow diagram",
    },
    ResourceDef {
        uri: "mcpm://inventory/servers",
        name: "inventory-servers",
        mime: "text/plain",
        description: "Servers, transport and source",
    },
];

pub fn find_tool(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
}

pub fn find_resource(uri: &str) -> Option<&'static ResourceDef> {
    RESOURCES.iter().find(|r| r.uri == uri)
}

fn ty_schema(ty: Ty) -> Value {
    match ty {
        Ty::Str => json!({"type": "string"}),
        Ty::Bool => json!({"type": "boolean"}),
        Ty::Obj => json!({"type": "object"}),
        Ty::StrList => json!({"type": "array", "items": {"type": "string"}}),
        Ty::Int => json!({"type": "integer"}),
    }
}

pub fn input_schema(tool: &ToolDef) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for param in tool.params {
        let mut schema = ty_schema(param.ty);
        schema["description"] = json!(param.desc);
        if let Some(default) = param.default {
            schema["default"] = json!(default);
        }
        props.insert(param.name.to_string(), schema);
        if param.required {
            required.push(json!(param.name));
        }
    }
    if tool.gate != Gate::None {
        props.insert(
            "confirm".to_string(),
            json!({"type": "boolean", "default": false, "description": "Must be true to run this tool"}),
        );
    }
    let mut schema = json!({
        "type": "object",
        "properties": props,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

pub fn tier_label(tier: u8) -> &'static str {
    match tier {
        1 => "read-only",
        2 => "write",
        3 => "confirm",
        _ => "destructive",
    }
}

pub fn tool_descriptor(tool: &ToolDef) -> Value {
    json!({
        "name": tool.name,
        "description": format!("[tier {} {}] {}", tool.tier, tier_label(tool.tier), tool.description),
        "inputSchema": input_schema(tool),
        "annotations": {
            "readOnlyHint": tool.tier == 1,
            "destructiveHint": tool.tier >= 4,
        },
    })
}
