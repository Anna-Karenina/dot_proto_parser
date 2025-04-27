use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Proto parse error: {0}")]
    ProtoParse(#[from] ProtoParseError),

    #[error("Converter error: {0}")]
    Converter(#[from] ConverterError),
    // Другие ошибки...
}

#[derive(Error, Debug)]
pub enum ConverterError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Unsupported schema type: {0}")]
    UnsupportedSchemaType(String),

    #[error("Missing reference: {0}")]
    MissingReference(String),

    #[error("Invalid array definition")]
    InvalidArrayDefinition,

    #[error("Circular reference detected: {0}")]
    CircularReference(String),

    #[error("Duplicate message name: {0}")]
    DuplicateMessageName(String),

    #[error("Invalid parameter location: {0}")]
    InvalidParameterLocation(String),

    #[error("Unsupported HTTP method: {0}")]
    UnsupportedHttpMethod(String),

    #[error("Invalid field name: {0}")]
    InvalidFieldName(String),

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("Message not found: {0}")]
    MessageNotFound(String),
}

#[derive(Error, Debug)]
pub enum ProtoParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error at line {line}: {message}")]
    ParseError { line: usize, message: String },

    #[error("Unexpected token: {0}")]
    UnexpectedToken(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Duplicate definition: {0}")]
    DuplicateDefinition(String),
}
