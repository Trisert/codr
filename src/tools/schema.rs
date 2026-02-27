use serde_json::Value;

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
// Validation Error
// ============================================================

#[derive(Clone, serde::Serialize)]
pub struct ValidationError {
    pub error: String,
    pub message: String,
    pub tool: String,
    pub received: Value,
    pub expected: Value,
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
            error: "INVALID_PARAMS".to_string(),
            message: message.to_string(),
            tool: tool.to_string(),
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
