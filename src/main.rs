use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use thiserror::Error;

const LABEL: &str = "/*\n * That schema was generated automatically by the Anna Karenina swagger-to-proto generator.\n * Please check everything twice before using that or parsing it somehow.\n*/ \n\n";

#[derive(Error, Debug)]
pub enum SwaggerToProtoError {
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
    #[error("Unresolvable schema")]
    UnresolvableSchema,
}

#[derive(Debug, Deserialize, Serialize)]
struct SwaggerDoc {
    paths: HashMap<String, PathItem>,
    definitions: Option<HashMap<String, Schema>>,
    components: Option<Components>,
    info: Option<Info>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Info {
    title: Option<String>,
    description: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Components {
    schemas: Option<HashMap<String, Schema>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PathItem {
    get: Option<Operation>,
    post: Option<Operation>,
    put: Option<Operation>,
    delete: Option<Operation>,
    parameters: Option<Vec<Parameter>>,
    request_body: Option<RequestBody>,
    #[serde(rename = "$ref")]
    ref_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RequestBody {
    description: Option<String>,
    content: HashMap<String, MediaType>,
    required: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Operation {
    tags: Option<Vec<String>>,
    summary: Option<String>,
    description: Option<String>,
    operation_id: Option<String>,
    responses: HashMap<String, Response>,
    parameters: Option<Vec<Parameter>>,
    request_body: Option<RequestBody>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Parameter {
    name: String,
    description: Option<String>,
    schema: Option<SchemaRef>,
    r#in: String,
    required: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Response {
    description: Option<String>,
    schema: Option<SchemaRef>,
    content: Option<HashMap<String, MediaType>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct MediaType {
    schema: Option<SchemaRef>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
enum SchemaRef {
    Ref { r#ref: String },
    Inline(Box<Schema>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Schema {
    r#type: Option<String>,
    format: Option<String>,
    description: Option<String>,
    items: Option<Box<SchemaRef>>,
    properties: Option<HashMap<String, Schema>>,
    additional_properties: Option<Box<SchemaRef>>,
    #[serde(rename = "enum")]
    enum_values: Option<Vec<String>>,
    #[serde(rename = "$ref")]
    ref_path: Option<String>,
}

struct ProtoGenerator {
    messages: HashMap<String, String>,
}

impl ProtoGenerator {
    fn new() -> Self {
        Self {
            messages: HashMap::new(),
        }
    }

    fn generate_proto(&mut self, swagger: &SwaggerDoc) -> Result<String, SwaggerToProtoError> {
        let mut proto_content = String::new();
        self.generate_header(&mut proto_content, swagger);

        if let Some(definitions) = &swagger.definitions {
            self.generate_messages(&mut proto_content, definitions)?;
        }

        if let Some(components) = &swagger.components {
            if let Some(schemas) = &components.schemas {
                self.generate_messages(&mut proto_content, schemas)?;
            }
        }

        self.generate_services(&mut proto_content, &swagger.paths)?;

        Ok(proto_content)
    }

    fn generate_header(&self, proto: &mut String, swagger: &SwaggerDoc) {
        proto.push_str(LABEL);
        proto.push_str("syntax = \"proto3\";\n\n");
        proto.push_str("import \"google/protobuf/empty.proto\";\n");
        proto.push_str("import \"google/protobuf/struct.proto\";\n\n");

        if let Some(info) = &swagger.info {
            proto.push_str("/*\n");
            if let Some(title) = &info.title {
                proto.push_str(&format!(" * Title: {}\n", title));
            }
            if let Some(description) = &info.description {
                for line in description.lines() {
                    proto.push_str(&format!(" * {}\n", line.trim()));
                }
            }
            if let Some(version) = &info.version {
                proto.push_str(&format!(" * Version: {}\n", version));
            }
            proto.push_str(" */\n\n");
        }
    }

    fn generate_messages(
        &mut self,
        proto: &mut String,
        schemas: &HashMap<String, Schema>,
    ) -> Result<(), SwaggerToProtoError> {
        for (name, schema) in schemas {
            if self.messages.contains_key(name) {
                continue;
            }

            let message_content = self.convert_schema_to_message(name, schema)?;
            proto.push_str(&message_content);
            self.messages.insert(name.clone(), message_content);
        }
        Ok(())
    }

    fn convert_schema_to_message(
        &mut self,
        name: &str,
        schema: &Schema,
    ) -> Result<String, SwaggerToProtoError> {
        let mut message = String::new();

        if let Some(description) = &schema.description {
            message.push_str(&format!("/* {} */\n", description.trim()));
        }

        message.push_str(&format!("message {} {{\n", name));

        if let Some(properties) = &schema.properties {
            for (i, (field_name, field_schema)) in properties.iter().enumerate() {
                if let Some(description) = &field_schema.description {
                    message.push_str(&format!("  /* {} */\n", description.trim()));
                }

                let proto_type = self.map_schema_to_proto_type(field_schema)?;
                message.push_str(&format!("  {} {} = {};\n", proto_type, field_name, i + 1));
            }
        }

        message.push_str("}\n\n");
        Ok(message)
    }
    fn generate_services(
        &mut self,
        proto: &mut String,
        paths: &HashMap<String, PathItem>,
    ) -> Result<(), SwaggerToProtoError> {
        let mut services: BTreeMap<String, Vec<(String, String, &Operation)>> = BTreeMap::new();

        // Собираем все операции
        for (path, item) in paths {
            self.collect_operations(&mut services, path, "Get", item.get.as_ref());
            self.collect_operations(&mut services, path, "Post", item.post.as_ref());
            self.collect_operations(&mut services, path, "Put", item.put.as_ref());
            self.collect_operations(&mut services, path, "Delete", item.delete.as_ref());
        }

        for (tag, methods) in services {
            let service_name = to_pascal_case(&tag);

            // Сначала генерируем все message для параметров
            for (path, method, operation) in &methods {
                // Генерируем message для query/path параметров
                if let Some(parameters) = &operation.parameters {
                    let query_params: Vec<_> = parameters
                        .iter()
                        .filter(|p| p.r#in == "query" || p.r#in == "path")
                        .collect();

                    if !query_params.is_empty() {
                        let message_name = format!(
                            "request_query_params_{}_{}",
                            service_name,
                            self.generate_method_name(path, method, operation)
                        );
                        let message =
                            self.generate_parameters_message(&message_name, query_params)?;
                        proto.push_str(&message);
                    }
                }

                // Генерируем message для тела запроса
                if let Some(request_body) = &operation.request_body {
                    let message_name = format!(
                        "request_body_{}_{}",
                        service_name,
                        self.generate_method_name(path, method, operation)
                    );
                    let message = self.generate_body_message(&message_name, request_body)?;
                    proto.push_str(&message);
                }
            }

            // Затем генерируем сам сервис
            proto.push_str(&format!("service {}Service {{\n", service_name));

            for (path, method, operation) in methods {
                if let Some(summary) = &operation.summary {
                    proto.push_str(&format!("  /* {} */\n", summary.trim()));
                }

                let method_name = self.generate_method_name(&path, &method, operation);
                let input_type =
                    self.generate_input_type(&service_name, &method_name, operation)?;
                let output_type = self.generate_output_type(operation)?;

                proto.push_str(&format!(
                    "  rpc {}({}) returns ({});\n",
                    method_name, input_type, output_type
                ));
            }

            proto.push_str("}\n\n");
        }

        Ok(())
    }

    fn generate_input_type(
        &mut self,
        service_name: &str,
        method_name: &str,
        operation: &Operation,
    ) -> Result<String, SwaggerToProtoError> {
        let mut has_query = false;
        let mut has_body = false;

        if let Some(parameters) = &operation.parameters {
            has_query = parameters
                .iter()
                .any(|p| p.r#in == "query" || p.r#in == "path");

            // FOR Swagger 2.0
            if let Some(body_param) = parameters.iter().find(|p| p.r#in == "body") {
                has_body = true;
                let message_name = format!("request_body_{}_{}", service_name, method_name);
                if !self.messages.contains_key(&message_name) {
                    // Создаем временный RequestBody из параметра
                    let mut fake_request_body = RequestBody {
                        description: body_param.description.clone(),
                        content: HashMap::new(),
                        required: body_param.required,
                    };

                    if let Some(schema_ref) = &body_param.schema {
                        let media_type = MediaType {
                            schema: Some(schema_ref.clone()),
                        };
                        fake_request_body
                            .content
                            .insert("application/json".to_string(), media_type);
                    }

                    let message = self.generate_body_message(&message_name, &fake_request_body)?;
                    self.messages.insert(message_name.clone(), message);
                }
            }
        }

        // FOR OpenAPI 3.0
        if let Some(request_body) = &operation.request_body {
            has_body = true;
            let message_name = format!("request_body_{}_{}", service_name, method_name);
            if !self.messages.contains_key(&message_name) {
                let message = self.generate_body_message(&message_name, request_body)?;
                self.messages.insert(message_name.clone(), message);
            }
        }

        Ok(match (has_query, has_body) {
            (true, true) => format!(
                "request_query_params_{}_{}AndBody",
                service_name, method_name
            ),
            (true, false) => format!("request_query_params_{}_{}", service_name, method_name),
            (false, true) => format!("request_body_{}_{}", service_name, method_name),
            (false, false) => "google.protobuf.Empty".to_string(),
        })
    }

    fn generate_parameters_message(
        &mut self,
        message_name: &str,
        parameters: Vec<&Parameter>,
    ) -> Result<String, SwaggerToProtoError> {
        if let Some(existing) = self.messages.get(message_name) {
            return Ok(existing.clone());
        }

        let mut message = format!("message {} {{\n", message_name);
        let mut field_number = 1;

        for param in parameters {
            if let Some(description) = &param.description {
                message.push_str(&format!("  /* {} */\n", description));
            }

            let proto_type = match &param.schema {
                Some(schema_ref) => self.schema_ref_to_proto_type(schema_ref)?,
                None => "string".to_string(),
            };

            let field_rule = if param.required.unwrap_or(false) {
                ""
            } else {
                "optional "
            };

            message.push_str(&format!(
                "  {}{} {} = {};\n",
                field_rule, proto_type, param.name, field_number
            ));
            field_number += 1;
        }

        message.push_str("}\n\n");
        self.messages
            .insert(message_name.to_string(), message.clone());
        Ok(message)
    }

    fn generate_body_message(
        &mut self,
        message_name: &str,
        request_body: &RequestBody,
    ) -> Result<String, SwaggerToProtoError> {
        if let Some(existing) = self.messages.get(message_name) {
            return Ok(existing.clone());
        }

        let mut message = String::new();

        if let Some(description) = &request_body.description {
            message.push_str(&format!("/* {} */\n", description));
        }

        message.push_str(&format!("message {} {{\n", message_name));

        if let Some((content_type, media_type)) = request_body.content.iter().next() {
            if let Some(schema_ref) = &media_type.schema {
                let proto_type = self.schema_ref_to_proto_type(schema_ref)?;
                message.push_str(&format!(
                    "  {} data = 1; // Content-Type: {}\n",
                    proto_type, content_type
                ));
            } else {
                message.push_str("  string data = 1;\n");
            }
        } else {
            message.push_str("  // No content schema defined\n");
        }

        message.push_str("}\n\n");
        self.messages
            .insert(message_name.to_string(), message.clone());
        Ok(message)
    }

    fn generate_output_type(&self, operation: &Operation) -> Result<String, SwaggerToProtoError> {
        if let Some(response) = operation.responses.get("200") {
            if let Some(schema_ref) = &response.schema {
                return self.schema_ref_to_proto_type(schema_ref);
            }
            if let Some(content) = &response.content {
                if let Some((_, media_type)) = content.iter().next() {
                    if let Some(schema_ref) = &media_type.schema {
                        return self.schema_ref_to_proto_type(schema_ref);
                    }
                }
            }
        }
        Ok("google.protobuf.Empty".to_string())
    }

    fn schema_ref_to_proto_type(
        &self,
        schema_ref: &SchemaRef,
    ) -> Result<String, SwaggerToProtoError> {
        match schema_ref {
            SchemaRef::Ref { r#ref } => {
                Ok(r#ref.split('/').last().unwrap_or("Response").to_string())
            }
            SchemaRef::Inline(schema) => self.map_schema_to_proto_type(schema),
        }
    }

    fn map_schema_to_proto_type(&self, schema: &Schema) -> Result<String, SwaggerToProtoError> {
        match schema.r#type.as_deref() {
            Some("integer") => match schema.format.as_deref() {
                Some("int64") => Ok("int64".to_string()),
                Some("int32") => Ok("int32".to_string()),
                _ => Ok("int64".to_string()),
            },
            Some("number") => match schema.format.as_deref() {
                Some("double") => Ok("double".to_string()),
                Some("float") => Ok("float".to_string()),
                _ => Ok("double".to_string()),
            },
            Some("boolean") => Ok("bool".to_string()),
            Some("string") => match schema.format.as_deref() {
                Some("date") | Some("date-time") => Ok("string".to_string()),
                Some("byte") => Ok("bytes".to_string()),
                _ => Ok("string".to_string()),
            },
            Some("array") => {
                let items_schema = schema
                    .items
                    .as_ref()
                    .ok_or(SwaggerToProtoError::InvalidArrayDefinition)?;
                let item_type = self.schema_ref_to_proto_type(items_schema)?;
                Ok(format!("repeated {}", item_type))
            }
            Some("object") => {
                if schema.properties.is_some() {
                    Ok("google.protobuf.Struct".to_string())
                } else if schema.additional_properties.is_some() {
                    Ok("map<string, string>".to_string())
                } else {
                    Ok("google.protobuf.Struct".to_string())
                }
            }
            None if schema.ref_path.is_some() => Ok(schema
                .ref_path
                .as_ref()
                .unwrap()
                .split('/')
                .last()
                .unwrap_or("Response")
                .to_string()),
            _ => Err(SwaggerToProtoError::UnsupportedSchemaType(
                schema.r#type.clone().unwrap_or("unknown".to_string()),
            )),
        }
    }

    fn collect_operations<'a>(
        &self,
        services: &mut BTreeMap<String, Vec<(String, String, &'a Operation)>>,
        path: &str,
        method: &str,
        operation: Option<&'a Operation>,
    ) {
        if let Some(op) = operation {
            let default_tags = vec!["Default".to_string()];
            let tags = op.tags.as_ref().unwrap_or(&default_tags);

            for tag in tags {
                services.entry(tag.clone()).or_default().push((
                    path.to_string(),
                    method.to_string(),
                    op,
                ));
            }
        }
    }

    fn generate_method_name(&self, path: &str, http_method: &str, operation: &Operation) -> String {
        operation.operation_id.as_ref().map_or_else(
            || {
                let clean_path = path.trim_matches('/').replace(['/', '{', '}'], "_");
                format!("{}{}", http_method, to_pascal_case(&clean_path))
            },
            |id| to_pascal_case(id),
        )
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().chain(c).collect(),
            }
        })
        .collect()
}

fn main() -> Result<(), SwaggerToProtoError> {
    let swagger_json = fs::read_to_string("swagger.json")?;
    let swagger: SwaggerDoc = serde_json::from_str(&swagger_json)?;

    let mut generator = ProtoGenerator::new();
    let proto_content = generator.generate_proto(&swagger)?;

    fs::write("api.proto", proto_content)?;
    println!("Successfully generated api.proto");

    Ok(())
}
