use serde_json::Value;
use std::sync::Arc;

// ============================================================
// JSON Schema Types
// ============================================================

#[derive(Debug, Clone)]
pub struct ToolSchema {
    #[allow(dead_code)]
    pub schema_type: String,
    pub properties: Vec<Property>,
    pub required: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub property_type: PropertyType,
    pub description: String,
    pub required: bool,
    pub default: Option<Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PropertyType {
    String,
    Number,
    Integer,
    Boolean,
    Array(Box<PropertyType>),
    Object,
    OneOf(Vec<PropertyType>),
}

// ============================================================
// Schema Builders
// ============================================================

impl ToolSchema {
    pub fn new() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: Vec::new(),
            required: Vec::new(),
        }
    }

    pub fn string(mut self, name: &str, description: &str, required: bool) -> Self {
        self.properties.push(Property {
            name: name.to_string(),
            property_type: PropertyType::String,
            description: description.to_string(),
            required,
            default: None,
        });
        if required {
            self.required.push(name.to_string());
        }
        self
    }

    #[allow(dead_code)]
    pub fn number(mut self, name: &str, description: &str, required: bool) -> Self {
        self.properties.push(Property {
            name: name.to_string(),
            property_type: PropertyType::Number,
            description: description.to_string(),
            required,
            default: None,
        });
        if required {
            self.required.push(name.to_string());
        }
        self
    }

    pub fn integer(mut self, name: &str, description: &str, required: bool) -> Self {
        self.properties.push(Property {
            name: name.to_string(),
            property_type: PropertyType::Integer,
            description: description.to_string(),
            required,
            default: None,
        });
        if required {
            self.required.push(name.to_string());
        }
        self
    }

    pub fn boolean(mut self, name: &str, description: &str, required: bool) -> Self {
        self.properties.push(Property {
            name: name.to_string(),
            property_type: PropertyType::Boolean,
            description: description.to_string(),
            required,
            default: None,
        });
        if required {
            self.required.push(name.to_string());
        }
        self
    }

    #[allow(dead_code)]
    pub fn build(&self) -> String {
        let props: Vec<String> = self
            .properties
            .iter()
            .map(|p| {
                let type_str: String = match &p.property_type {
                    PropertyType::String => "string".to_string(),
                    PropertyType::Number => "number".to_string(),
                    PropertyType::Integer => "integer".to_string(),
                    PropertyType::Boolean => "boolean".to_string(),
                    PropertyType::Array(inner) => {
                        let inner_str = match inner.as_ref() {
                            PropertyType::String => "string",
                            _ => "any",
                        };
                        format!("array of {}", inner_str)
                    }
                    PropertyType::Object => "object".to_string(),
                    PropertyType::OneOf(types) => {
                        let type_strs: Vec<&str> = types
                            .iter()
                            .map(|t| match t {
                                PropertyType::String => "string",
                                PropertyType::Number => "number",
                                PropertyType::Integer => "integer",
                                PropertyType::Boolean => "boolean",
                                _ => "any",
                            })
                            .collect();
                        type_strs.join(" or ")
                    }
                };
                let req = if p.required { " (required)" } else { "" };
                format!("- {} ({}): {}{}", p.name, type_str, p.description, req)
            })
            .collect();

        props.join("\n")
    }

    pub fn get_property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }
}

impl Default for ToolSchema {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// JSON Schema Conversion for Native Tool Calling
// ============================================================

impl ToolSchema {
    /// Convert to JSON Schema format for native tool calling
    /// Returns the input_schema JSON value for tool definitions
    pub fn to_json_schema(&self) -> serde_json::Value {
        let mut properties_map = serde_json::Map::new();

        for prop in &self.properties {
            let schema = match &prop.property_type {
                PropertyType::String => {
                    json_schema_type("string", &prop.description, prop.required, &prop.default)
                }
                PropertyType::Number => {
                    json_schema_type("number", &prop.description, prop.required, &prop.default)
                }
                PropertyType::Integer => {
                    json_schema_type("integer", &prop.description, prop.required, &prop.default)
                }
                PropertyType::Boolean => {
                    json_schema_type("boolean", &prop.description, prop.required, &prop.default)
                }
                PropertyType::Array(inner) => {
                    let item_type = match inner.as_ref() {
                        PropertyType::String => "string",
                        _ => "any",
                    };
                    serde_json::json!({
                        "type": "array",
                        "description": prop.description,
                        "items": {"type": item_type}
                    })
                }
                PropertyType::Object => {
                    serde_json::json!({
                        "type": "object",
                        "description": prop.description
                    })
                }
                PropertyType::OneOf(types) => {
                    let type_list: Vec<&str> = types.iter().map(|t| match t {
                        PropertyType::String => "string",
                        PropertyType::Number => "number",
                        PropertyType::Integer => "integer",
                        PropertyType::Boolean => "boolean",
                        _ => "string",
                    }).collect();
                    serde_json::json!({
                        "description": prop.description,
                        "oneOf": type_list.iter().map(|t| serde_json::json!({"type": t})).collect::<Vec<_>>()
                    })
                }
            };
            properties_map.insert(prop.name.clone(), schema);
        }

        serde_json::json!({
            "type": "object",
            "properties": properties_map,
            "required": self.required
        })
    }
}

fn json_schema_type(
    type_name: &str,
    description: &str,
    _required: bool,
    default: &Option<Value>,
) -> Value {
    let mut result = serde_json::json!({
        "type": type_name,
        "description": description
    });

    if let Some(default_val) = default {
        result["default"] = default_val.clone();
    }

    result
}

// ============================================================
// Validation Error
// ============================================================

#[derive(Clone)]
pub struct ValidationError {
    pub error: Arc<str>,
    pub message: Arc<str>,
    pub tool: Arc<str>,
    pub received: Value,
    pub expected: Value,
}

impl serde::Serialize for ValidationError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("ValidationError", 5)?;
        s.serialize_field("error", &*self.error)?;
        s.serialize_field("message", &*self.message)?;
        s.serialize_field("tool", &*self.tool)?;
        s.serialize_field("received", &self.received)?;
        s.serialize_field("expected", &self.expected)?;
        s.end()
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.error, self.message)
    }
}

impl std::fmt::Debug for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ValidationError({}, {})", self.error, self.message)
    }
}

impl std::error::Error for ValidationError {}

impl ValidationError {
    pub fn new(tool: &str, message: &str, received: Value, expected: Value) -> Self {
        Self {
            error: "INVALID_PARAMS".into(),
            message: message.into(),
            tool: tool.into(),
            received,
            expected,
        }
    }

    #[allow(dead_code)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

// ============================================================
// Parameter Extraction Helpers with Type Coercion
// ============================================================

#[allow(dead_code)]
#[allow(clippy::result_large_err)]
pub trait ExtractParams {
    fn get_str(&self, key: &str) -> Result<Option<String>, ValidationError>;
    fn get_required_str(&self, key: &str) -> Result<String, ValidationError>;
    fn get_number(&self, key: &str) -> Result<Option<f64>, ValidationError>;
    fn get_required_number(&self, key: &str) -> Result<f64, ValidationError>;
    fn get_integer(&self, key: &str) -> Result<Option<i64>, ValidationError>;
    fn get_required_integer(&self, key: &str) -> Result<i64, ValidationError>;
    fn get_bool(&self, key: &str) -> Result<Option<bool>, ValidationError>;
    fn get_required_bool(&self, key: &str) -> Result<bool, ValidationError>;
}

impl ExtractParams for Value {
    fn get_str(&self, key: &str) -> Result<Option<String>, ValidationError> {
        match self.get(key) {
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(Value::Null) => Ok(None),
            Some(v) => Ok(Some(v.to_string())),
            None => Ok(None),
        }
    }

    fn get_required_str(&self, key: &str) -> Result<String, ValidationError> {
        self.get_str(key)?.ok_or_else(|| {
            ValidationError::new(
                "",
                &format!("Missing required parameter: {}", key),
                Value::Null,
                Value::String("string".to_string()),
            )
        })
    }

    fn get_number(&self, key: &str) -> Result<Option<f64>, ValidationError> {
        match self.get(key) {
            Some(Value::Number(n)) => Ok(Some(n.as_f64().unwrap_or(0.0))),
            Some(Value::String(s)) => {
                // Try to coerce string to number
                match s.parse::<f64>() {
                    Ok(n) => Ok(Some(n)),
                    Err(_) => Err(ValidationError::new(
                        "",
                        &format!("Parameter '{}' must be a number, got: '{}'", key, s),
                        Value::String(s.clone()),
                        Value::String("number".to_string()),
                    )),
                }
            }
            Some(Value::Null) => Ok(None),
            Some(v) => Err(ValidationError::new(
                "",
                &format!("Parameter '{}' must be a number", key),
                v.clone(),
                Value::String("number".to_string()),
            )),
            None => Ok(None),
        }
    }

    fn get_required_number(&self, key: &str) -> Result<f64, ValidationError> {
        self.get_number(key)?.ok_or_else(|| {
            ValidationError::new(
                "",
                &format!("Missing required parameter: {}", key),
                Value::Null,
                Value::String("number".to_string()),
            )
        })
    }

    fn get_integer(&self, key: &str) -> Result<Option<i64>, ValidationError> {
        match self.get(key) {
            Some(Value::Number(n)) => Ok(n.as_i64()),
            Some(Value::String(s)) => match s.parse::<i64>() {
                Ok(n) => Ok(Some(n)),
                Err(_) => Err(ValidationError::new(
                    "",
                    &format!("Parameter '{}' must be an integer, got: '{}'", key, s),
                    Value::String(s.clone()),
                    Value::String("integer".to_string()),
                )),
            },
            Some(Value::Bool(b)) => Ok(Some(if *b { 1 } else { 0 })),
            Some(Value::Null) => Ok(None),
            Some(v) => Err(ValidationError::new(
                "",
                &format!("Parameter '{}' must be an integer", key),
                v.clone(),
                Value::String("integer".to_string()),
            )),
            None => Ok(None),
        }
    }

    fn get_required_integer(&self, key: &str) -> Result<i64, ValidationError> {
        self.get_integer(key)?.ok_or_else(|| {
            ValidationError::new(
                "",
                &format!("Missing required parameter: {}", key),
                Value::Null,
                Value::String("integer".to_string()),
            )
        })
    }

    fn get_bool(&self, key: &str) -> Result<Option<bool>, ValidationError> {
        match self.get(key) {
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(Value::String(s)) => {
                let s_lower = s.to_lowercase();
                match s_lower.as_str() {
                    "true" | "1" | "yes" | "on" => Ok(Some(true)),
                    "false" | "0" | "no" | "off" => Ok(Some(false)),
                    _ => Err(ValidationError::new(
                        "",
                        &format!("Parameter '{}' must be a boolean, got: '{}'", key, s),
                        Value::String(s.clone()),
                        Value::String("boolean".to_string()),
                    )),
                }
            }
            Some(Value::Number(n)) => Ok(Some(n.as_f64().unwrap_or(0.0) != 0.0)),
            Some(Value::Null) => Ok(None),
            Some(v) => Ok(Some(v.as_bool().unwrap_or(false))),
            None => Ok(None),
        }
    }

    fn get_required_bool(&self, key: &str) -> Result<bool, ValidationError> {
        self.get_bool(key)?.ok_or_else(|| {
            ValidationError::new(
                "",
                &format!("Missing required parameter: {}", key),
                Value::Null,
                Value::String("boolean".to_string()),
            )
        })
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_schema_new() {
        let schema = ToolSchema::new();
        assert!(schema.properties.is_empty());
    }

    #[test]
    fn test_tool_schema_string() {
        let schema = ToolSchema::new().string("name", "The name", true);
        assert_eq!(schema.properties.len(), 1);
        assert!(schema.required.contains(&"name".to_string()));
    }

    #[test]
    fn test_tool_schema_integer() {
        let schema = ToolSchema::new().integer("count", "A count", false);
        assert_eq!(schema.properties.len(), 1);
    }

    #[test]
    fn test_tool_schema_boolean() {
        let schema = ToolSchema::new().boolean("enabled", "Is enabled", true);
        assert_eq!(schema.properties.len(), 1);
    }

    #[test]
    fn test_property_creation() {
        let prop = Property {
            name: "test".to_string(),
            property_type: PropertyType::String,
            description: "A name".to_string(),
            required: true,
            default: None,
        };
        assert_eq!(prop.name, "test");
    }

    #[test]
    fn test_validation_error_new() {
        let err = ValidationError::new(
            "test_tool",
            "Invalid parameter",
            Value::Null,
            Value::String("string".to_string()),
        );
        assert_eq!(&*err.error, "INVALID_PARAMS");
    }

    #[test]
    fn test_extract_params_get_str() {
        let value = serde_json::json!({ "name": "test" });
        let result = value.get_str("name");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("test".to_string()));
    }

    #[test]
    fn test_extract_params_get_str_missing() {
        let value = serde_json::json!({});
        let result = value.get_str("name");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_extract_params_get_required_str() {
        let value = serde_json::json!({ "name": "test" });
        let result = value.get_required_str("name");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test");
    }

    #[test]
    fn test_extract_params_get_required_str_missing() {
        let value = serde_json::json!({});
        let result = value.get_required_str("name");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_params_get_bool_true() {
        let value = serde_json::json!({ "enabled": true });
        let result = value.get_bool("enabled");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(true));
    }

    #[test]
    fn test_extract_params_get_bool_false() {
        let value = serde_json::json!({ "enabled": false });
        let result = value.get_bool("enabled");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(false));
    }

    #[test]
    fn test_extract_params_get_bool_from_string_true() {
        let value = serde_json::json!({ "enabled": "true" });
        let result = value.get_bool("enabled");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(true));
    }

    #[test]
    fn test_extract_params_get_bool_from_string_invalid() {
        let value = serde_json::json!({ "enabled": "invalid" });
        let result = value.get_bool("enabled");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_params_get_bool_missing() {
        let value = serde_json::json!({});
        let result = value.get_bool("enabled");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_extract_params_get_required_bool() {
        let value = serde_json::json!({ "enabled": true });
        let result = value.get_required_bool("enabled");
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_extract_params_get_required_bool_missing() {
        let value = serde_json::json!({});
        let result = value.get_required_bool("enabled");
        assert!(result.is_err());
    }
}
