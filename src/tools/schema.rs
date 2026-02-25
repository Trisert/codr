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

#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub property_type: PropertyType,
    pub description: String,
    pub required: bool,
    pub default: Option<Value>,
}

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
                        let type_strs: Vec<&str> = types.iter().map(|t| match t {
                            PropertyType::String => "string",
                            PropertyType::Number => "number",
                            PropertyType::Integer => "integer",
                            PropertyType::Boolean => "boolean",
                            _ => "any",
                        }).collect();
                        type_strs.join(" or ")
                    }
                };
                let req = if p.required { " (required)" } else { "" };
                format!("- {} ({}): {}{}", p.name, type_str, p.description, req)
            })
            .collect();

        props.join("\n")
    }
}

impl Default for ToolSchema {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// Parameter Extraction Helpers
// ============================================================

pub trait ExtractParams {
    fn get_str(&self, key: &str) -> Result<Option<String>, String>;
    fn get_required_str(&self, key: &str) -> Result<String, String>;
    fn get_number(&self, key: &str) -> Result<Option<f64>, String>;
    fn get_required_number(&self, key: &str) -> Result<f64, String>;
    fn get_bool(&self, key: &str) -> Result<Option<bool>, String>;
}

impl ExtractParams for serde_json::Value {
    fn get_str(&self, key: &str) -> Result<Option<String>, String> {
        match self.get(key) {
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(Value::Null) => Ok(None),
            Some(v) => Ok(Some(v.to_string())),
            None => Ok(None),
        }
    }

    fn get_required_str(&self, key: &str) -> Result<String, String> {
        self.get_str(key)?
            .ok_or_else(|| format!("Missing required parameter: {}", key))
    }

    fn get_number(&self, key: &str) -> Result<Option<f64>, String> {
        match self.get(key) {
            Some(Value::Number(n)) => Ok(Some(n.as_f64().unwrap_or(0.0))),
            Some(Value::Null) => Ok(None),
            None => Ok(None),
            Some(_) => Err(format!("Parameter '{}' must be a number", key)),
        }
    }

    fn get_required_number(&self, key: &str) -> Result<f64, String> {
        self.get_number(key)?
            .ok_or_else(|| format!("Missing required parameter: {}", key))
    }

    fn get_bool(&self, key: &str) -> Result<Option<bool>, String> {
        match self.get(key) {
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(Value::Null) => Ok(None),
            None => Ok(None),
            Some(v) => Ok(Some(v.as_bool().unwrap_or(false))),
        }
    }
}
