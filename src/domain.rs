use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::{ConverterError, NameFormatter};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtoFile {
    pub syntax: String,
    pub package: String,
    pub imports: Vec<String>,
    // pub options: HashMap<String, String>,
    pub messages: Vec<Message>,
    pub enums: Vec<Enum>,
    pub services: Vec<Service>,
}

impl NameFormatter for ProtoFile {}

impl ProtoFile {
    pub fn new(package: &str) -> Self {
        Self {
            syntax: "proto3".to_string(),
            package: package.to_string(),
            imports: vec![
                "google/protobuf/empty.proto".to_string(),
                "google/protobuf/timestamp.proto".to_string(),
                "google/protobuf/struct.proto".to_string(),
            ],
            ..Default::default()
        }
    }

    pub fn add_import(&mut self, import_path: &str) {
        if !self.imports.contains(&import_path.to_string()) {
            self.imports.push(import_path.to_string());
        }
    }

    pub fn add_message(&mut self, message: Message) -> Result<(), ConverterError> {
        if self.messages.iter().any(|m| m.name == message.name) {
            return Err(ConverterError::DuplicateMessageName(message.name));
        }
        self.messages.push(message);
        Ok(())
    }

    pub fn add_enum(&mut self, enum_def: Enum) -> Result<(), ConverterError> {
        if self.enums.iter().any(|e| e.name == enum_def.name) {
            return Err(ConverterError::DuplicateMessageName(enum_def.name));
        }
        self.enums.push(enum_def);
        Ok(())
    }

    pub fn add_service(&mut self, service: Service) -> Result<(), ConverterError> {
        if self.services.iter().any(|s| s.name == service.name) {
            return Err(ConverterError::DuplicateMessageName(service.name));
        }
        self.services.push(service);
        Ok(())
    }

    pub fn find_message_mut(&mut self, name: &str) -> Option<&mut Message> {
        self.messages.iter_mut().find(|m| m.name == name)
    }

    pub fn find_message(&self, name: &str) -> Option<&Message> {
        self.messages.iter().find(|m| m.name == name)
    }

    pub fn find_service_mut(&mut self, name: &str) -> Option<&mut Service> {
        self.services.iter_mut().find(|s| s.name == name)
    }

    pub fn find_service(&self, name: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.name == name)
    }

    pub fn to_proto_text(&self) -> String {
        let mut output = String::new();

        output.push_str(&format!("syntax = \"{}\";\n\n", self.syntax));
        output.push_str(&format!("package {};\n\n", self.package));

        for import in &self.imports {
            output.push_str(&format!("import \"{}\";\n", import));
        }
        if !self.imports.is_empty() {
            output.push_str("\n");
        }

        // for (key, value) in &self.options {
        //     output.push_str(&format!("option {} = \"{}\";\n", key, value));
        // }
        // if !self.options.is_empty() {
        //     output.push_str("\n");
        // }

        for message in &self.messages {
            output.push_str(&message.to_proto_text(0));
        }

        for enum_def in &self.enums {
            output.push_str(&enum_def.to_proto_text(0));
        }

        for service in &self.services {
            output.push_str(&service.to_proto_text());
        }

        output
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Message {
    pub name: String,
    pub fields: Vec<Field>,
    pub comments: Vec<String>,
    pub nested_messages: Vec<Message>,
    pub nested_enums: Vec<Enum>,
}

impl Message {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn add_comment(&mut self, comment: &str) {
        self.comments.push(comment.to_string());
    }

    pub fn add_field(&mut self, field: Field) -> Result<(), ConverterError> {
        if self.fields.iter().any(|f| f.name == field.name) {
            return Err(ConverterError::InvalidFieldName(format!(
                "Duplicate field name: {}",
                field.name
            )));
        }
        self.fields.push(field);
        Ok(())
    }

    pub fn add_nested_message(&mut self, message: Message) -> Result<(), ConverterError> {
        if self.nested_messages.iter().any(|m| m.name == message.name) {
            return Err(ConverterError::DuplicateMessageName(message.name));
        }
        self.nested_messages.push(message);
        Ok(())
    }

    pub fn add_nested_enum(&mut self, enum_def: Enum) -> Result<(), ConverterError> {
        if self.nested_enums.iter().any(|e| e.name == enum_def.name) {
            return Err(ConverterError::DuplicateMessageName(enum_def.name));
        }
        self.nested_enums.push(enum_def);
        Ok(())
    }

    pub fn to_proto_text(&self, indent_level: usize) -> String {
        let indent = "  ".repeat(indent_level);
        let mut output = String::new();

        for comment in &self.comments {
            output.push_str(&format!("{}// {}\n", indent, comment));
        }

        output.push_str(&format!("{}message {} {{\n", indent, self.name));

        for field in &self.fields {
            output.push_str(&field.to_proto_text(indent_level + 1));
        }

        for message in &self.nested_messages {
            output.push_str(&message.to_proto_text(indent_level + 1));
        }

        for enum_def in &self.nested_enums {
            output.push_str(&enum_def.to_proto_text(indent_level + 1));
        }

        output.push_str(&format!("{}}}\n\n", indent));

        output
    }
}

/// Represents a protofile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub type_: String,
    pub number: i32,
    pub rule: FieldRule,
    pub comments: Vec<String>,
    pub options: HashMap<String, String>,
}

impl Field {
    /// Creates a new Field
    pub fn new(name: &str, type_: &str, number: i32, rule: FieldRule) -> Self {
        Self {
            name: name.to_string(),
            type_: type_.to_string(),
            number,
            rule,
            comments: Vec::new(),
            options: HashMap::new(),
        }
    }

    /// Adds a comment line to the field
    pub fn add_comment(&mut self, comment: &str) {
        self.comments.push(comment.to_string());
    }

    /// Adds an option to the field
    pub fn add_option(&mut self, key: &str, value: &str) {
        self.options.insert(key.to_string(), value.to_string());
    }

    /// Converts the Field to its textual representation
    pub fn to_proto_text(&self, indent_level: usize) -> String {
        let indent = "  ".repeat(indent_level);
        let mut output = String::new();

        // Comments
        for comment in &self.comments {
            output.push_str(&format!("{}// {}\n", indent, comment));
        }

        // Field definition
        let rule_str = match self.rule {
            FieldRule::Optional => "optional ",
            FieldRule::Required => "",
            FieldRule::Repeated => "repeated ",
        };

        output.push_str(&format!(
            "{}{}{} {} = {}",
            indent, rule_str, self.type_, self.name, self.number
        ));

        // Options
        if !self.options.is_empty() {
            let options: Vec<String> = self
                .options
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect();
            output.push_str(&format!(" [{}]", options.join(", ")));
        }

        output.push_str(";\n");
        output
    }
}

/// Represents field rules in Protocol Buffers
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FieldRule {
    Optional,
    Required,
    Repeated,
}

impl fmt::Display for FieldRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldRule::Optional => write!(f, "optional"),
            FieldRule::Required => write!(f, ""),
            FieldRule::Repeated => write!(f, "repeated"),
        }
    }
}

/// Represents a Protocol Buffers enum
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Enum {
    pub name: String,
    pub values: Vec<EnumValue>,
    pub comments: Vec<String>,
}

impl Enum {
    /// Creates a new Enum with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    /// Adds a comment line to the enum
    pub fn add_comment(&mut self, comment: &str) {
        self.comments.push(comment.to_string());
    }

    /// Adds a value to the enum
    pub fn add_value(&mut self, value: EnumValue) -> Result<(), ConverterError> {
        if self.values.iter().any(|v| v.name == value.name) {
            return Err(ConverterError::InvalidFieldName(format!(
                "Duplicate enum value: {}",
                value.name
            )));
        }
        self.values.push(value);
        Ok(())
    }

    /// Converts the Enum to its textual representation
    pub fn to_proto_text(&self, indent_level: usize) -> String {
        let indent = "  ".repeat(indent_level);
        let mut output = String::new();

        // Comments
        for comment in &self.comments {
            output.push_str(&format!("{}// {}\n", indent, comment));
        }

        // Enum header
        output.push_str(&format!("{}enum {} {{\n", indent, self.name));

        // Values
        for value in &self.values {
            output.push_str(&value.to_proto_text(indent_level + 1));
        }

        // Closing brace
        output.push_str(&format!("{}}}\n\n", indent));

        output
    }
}

/// Represents a Protocol Buffers enum value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumValue {
    pub name: String,
    pub number: i32,
    pub comments: Vec<String>,
}

impl EnumValue {
    /// Creates a new EnumValue
    pub fn new(name: &str, number: i32) -> Self {
        Self {
            name: name.to_string(),
            number,
            comments: Vec::new(),
        }
    }

    /// Adds a comment line to the enum value
    pub fn add_comment(&mut self, comment: &str) {
        self.comments.push(comment.to_string());
    }

    /// Converts the EnumValue to its textual representation
    pub fn to_proto_text(&self, indent_level: usize) -> String {
        let indent = "  ".repeat(indent_level);
        let mut output = String::new();

        // Comments
        for comment in &self.comments {
            output.push_str(&format!("{}// {}\n", indent, comment));
        }

        // Value definition
        output.push_str(&format!("{} {} = {};\n", indent, self.name, self.number));

        output
    }
}

/// Represents a Protocol Buffers service
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub methods: Vec<Method>,
    pub comments: Vec<String>,
}

impl Service {
    /// Creates a new Service with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    /// Adds a comment line to the service
    pub fn add_comment(&mut self, comment: &str) {
        self.comments.push(comment.to_string());
    }

    /// Adds a method to the service
    pub fn add_method(&mut self, method: Method) -> Result<(), ConverterError> {
        if self.methods.iter().any(|m| m.name == method.name) {
            return Err(ConverterError::InvalidFieldName(format!(
                "Duplicate method name: {}",
                method.name
            )));
        }
        self.methods.push(method);
        Ok(())
    }

    /// Converts the Service to its textual representation
    pub fn to_proto_text(&self) -> String {
        let mut output = String::new();

        // Service header
        output.push_str(&format!("service {} {{\n", self.name));

        // Methods with their own comments
        for method in &self.methods {
            output.push_str(&method.to_proto_text());
        }

        // Closing brace
        output.push_str("}\n\n");

        output
    }
}

/// Represents a Protocol Buffers service method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Method {
    pub name: String,
    pub input_type: String,
    pub output_type: String,
    pub comments: Vec<String>,
    pub options: HashMap<String, String>,
}

impl Method {
    /// Creates a new Method
    pub fn new(name: &str, input_type: &str, output_type: &str) -> Self {
        Self {
            name: name.to_string(),
            input_type: input_type.to_string(),
            output_type: output_type.to_string(),
            comments: Vec::new(),
            options: HashMap::new(),
        }
    }

    /// Adds a comment line to the method
    pub fn add_comment(&mut self, comment: &str) {
        self.comments.push(comment.to_string());
    }

    /// Adds an option to the method
    pub fn add_option(&mut self, key: &str, value: &str) {
        self.options.insert(key.to_string(), value.to_string());
    }

    /// Converts the Method to its textual representation
    pub fn to_proto_text(&self) -> String {
        let mut output = String::new();

        // Method comments
        for comment in &self.comments {
            output.push_str(&format!("  // {}\n", comment));
        }

        // Add HTTP options as comments
        if let Some(http_method) = self.options.get("http_method") {
            if let Some(http_path) = self.options.get("http_path") {
                output.push_str(&format!("  // HTTP: {} {}\n", http_method, http_path));
            }
        }

        // Method definition
        output.push_str(&format!(
            "  rpc {} ({}) returns ({})",
            self.name, self.input_type, self.output_type
        ));

        // Other options (excluding HTTP options)
        let other_options: Vec<String> = self
            .options
            .iter()
            .filter(|&(k, _)| k != "http_method" && k != "http_path")
            .map(|(k, v)| format!("{}=\"{}\"", k, v))
            .collect();

        if !other_options.is_empty() {
            output.push_str(&format!(" [{}]", other_options.join(", ")));
        }

        output.push_str(";\n\n");
        output
    }
}
