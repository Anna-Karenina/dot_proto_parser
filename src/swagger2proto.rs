use rand::random;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::{
    ConverterError, Enum, EnumValue, Field, FieldRule, Message, Method, ProtoFile, Service,
};

pub struct SwaggerToProtoConverter {
    proto: ProtoFile,
    generated_messages: HashMap<String, usize>,
    current_refs: Vec<String>,
}

impl SwaggerToProtoConverter {
    pub fn new(package_name: &str) -> Self {
        Self {
            proto: ProtoFile::new(package_name),
            generated_messages: HashMap::new(),
            current_refs: Vec::new(),
        }
    }

    pub fn convert_file(
        &mut self,
        input_path: &Path,
        output_path: &Path,
    ) -> Result<(), ConverterError> {
        let content = std::fs::read_to_string(input_path)?;
        let spec: SwaggerDoc = serde_json::from_str(&content)?;

        self.process_swagger_doc(&spec)?;

        let proto_text = self.proto.to_proto_text();
        std::fs::write(output_path, proto_text)?;

        Ok(())
    }

    fn process_swagger_doc(&mut self, spec: &SwaggerDoc) -> Result<(), ConverterError> {
        if let Some(definitions) = &spec.definitions {
            self.process_schemas(definitions, None)?;
        }

        if let Some(components) = &spec.components {
            if let Some(schemas) = &components.schemas {
                self.process_schemas(schemas, Some(components))?;
            }
        }

        self.process_services(&spec.paths, spec)?;

        Ok(())
    }

    fn process_schemas(
        &mut self,
        schemas: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<(), ConverterError> {
        for (name, schema) in schemas {
            if self.generated_messages.contains_key(name) {
                continue;
            }

            let message = self.convert_schema_to_message(name, schema, schemas, components)?;
            self.proto.add_message(message)?;
            self.generated_messages.insert(name.clone(), 1);
        }

        Ok(())
    }

    fn convert_schema_to_message(
        &mut self,
        name: &str,
        schema: &Schema,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<Message, ConverterError> {
        if self.current_refs.contains(&name.to_string()) {
            return Err(ConverterError::CircularReference(name.to_string()));
        }
        self.current_refs.push(name.to_string());

        let mut message = Message::new(name);
        let mut field_number = 1;

        if let Some(description) = &schema.description {
            description.lines().for_each(|line| {
                message.add_comment(line.trim());
            });
        }

        if let Some(one_of) = &schema.one_of {
            self.handle_one_of_any_of(
                &mut message,
                name,
                "OneOf",
                one_of,
                definitions,
                components,
            )?;
        } else if let Some(all_of) = &schema.all_of {
            self.handle_all_of(&mut message, all_of, definitions, components)?;
        } else if let Some(any_of) = &schema.any_of {
            self.handle_one_of_any_of(
                &mut message,
                name,
                "AnyOf",
                any_of,
                definitions,
                components,
            )?;
        } else if let Some(properties) = &schema.properties {
            self.handle_properties(
                &mut message,
                name,
                properties,
                &schema.required,
                definitions,
                components,
            )?;
        } else if let Some(additional_props) = &schema.additional_properties {
            self.handle_additional_properties(
                &mut message,
                additional_props,
                definitions,
                components,
            )?;
        } else if let Some(enum_values) = &schema.enum_values {
            self.handle_root_enum(&mut message, name, enum_values)?;
        }

        self.current_refs.pop();
        Ok(message)
    }
    fn handle_one_of_any_of(
        &mut self,
        message: &mut Message,
        name: &str,
        suffix: &str,
        items: &[SchemaRef],
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<(), ConverterError> {
        let mut fields = Vec::new();
        let type_name = format!("{}{}", name, suffix);

        for (i, item) in items.iter().enumerate() {
            let field_type = self.schema_ref_to_type(item, definitions, components)?;
            fields.push(Field::new(
                &format!("variant_{}", i + 1),
                &field_type,
                (i + 1) as i32,
                FieldRule::Optional,
            ));
        }

        let mut nested_msg = Message::new(&type_name);
        for field in fields {
            nested_msg.add_field(field)?;
        }

        message.add_nested_message(nested_msg)?;
        message.add_field(Field::new(
            &suffix.to_lowercase(),
            &type_name,
            1,
            FieldRule::Optional,
        ))
    }

    fn handle_all_of(
        &mut self,
        message: &mut Message,
        items: &[SchemaRef],
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<(), ConverterError> {
        let mut field_number = 1;
        for item in items {
            let resolved = self.resolve_schema_ref(item, definitions, components)?;
            if let Some(properties) = &resolved.properties {
                for (prop_name, prop_schema) in properties {
                    let type_name = self.schema_to_type(prop_schema, definitions, components)?;
                    message.add_field(Field::new(
                        &self.sanitize_field_name(prop_name),
                        &type_name,
                        field_number,
                        FieldRule::Optional,
                    ))?;
                    field_number += 1;
                }
            }
        }
        Ok(())
    }

    fn handle_properties(
        &mut self,
        message: &mut Message,
        message_name: &str,
        properties: &HashMap<String, Schema>,
        required_fields: &Option<Vec<String>>,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<(), ConverterError> {
        let mut field_number = 1;

        for (prop_name, prop_schema) in properties {
            if prop_name.starts_with("//") {
                continue;
            }

            // Добавляем описание свойства как комментарий
            if let Some(description) = &prop_schema.description {
                description.lines().for_each(|line| {
                    message.add_comment(&format!("// {}", line.trim()));
                });
            }

            // Обрабатываем enum поля
            let type_name = if let Some(enum_values) = &prop_schema.enum_values {
                let enum_name = format!("{}{}", message_name, self.to_pascal_case(prop_name));
                let mut enum_def = Enum::new(&enum_name);

                for (i, value) in enum_values.iter().enumerate() {
                    let variant_name = match value {
                        serde_json::Value::String(s) => s
                            .to_uppercase()
                            .replace(|c: char| !c.is_alphanumeric(), "_"),
                        serde_json::Value::Number(n) => format!("VALUE_{}", n),
                        _ => format!("VALUE_{}", i + 1),
                    };
                    enum_def.add_value(EnumValue::new(&variant_name, (i) as i32))?;
                }

                self.proto.add_enum(enum_def)?;
                enum_name
            } else {
                self.schema_to_type(prop_schema, definitions, components)?
            };

            let (final_type, field_rule) = if type_name.starts_with("repeated ") {
                let item_type = type_name.trim_start_matches("repeated ");
                let list_type = format!("{}List", item_type);

                if !self.generated_messages.contains_key(&list_type) {
                    let mut list_message = Message::new(&list_type);
                    list_message.add_field(Field::new(
                        "items",
                        &format!("repeated {}", item_type),
                        1,
                        FieldRule::Optional,
                    ))?;
                    self.proto.add_message(list_message)?;
                    self.generated_messages.insert(list_type.clone(), 1);
                }

                (list_type, FieldRule::Optional)
            } else {
                let rule = if required_fields
                    .as_ref()
                    .map(|r| r.contains(prop_name))
                    .unwrap_or(false)
                {
                    FieldRule::Required
                } else {
                    FieldRule::Optional
                };
                (type_name, rule)
            };

            message.add_field(Field::new(
                &self.sanitize_field_name(prop_name),
                &final_type,
                field_number,
                field_rule,
            ))?;

            field_number += 1;
        }
        Ok(())
    }

    fn handle_additional_properties(
        &mut self,
        message: &mut Message,
        additional_props: &SchemaRef,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<(), ConverterError> {
        let value_type = self.schema_ref_to_type(additional_props, definitions, components)?;
        message.add_field(Field::new(
            "properties",
            &format!("map<string, {}>", value_type),
            1,
            FieldRule::Optional,
        ))
    }

    fn handle_root_enum(
        &mut self,
        message: &mut Message,
        message_name: &str,
        enum_values: &[serde_json::Value],
    ) -> Result<(), ConverterError> {
        let enum_name = format!("{}Status", message_name);
        let mut enum_def = Enum::new(&enum_name);

        for (i, value) in enum_values.iter().enumerate() {
            let variant_name = match value {
                serde_json::Value::String(s) => s
                    .to_uppercase()
                    .replace(|c: char| !c.is_alphanumeric(), "_"),
                serde_json::Value::Number(n) => format!("VALUE_{}", n),
                _ => format!("VALUE_{}", i + 1),
            };
            enum_def.add_value(EnumValue::new(&variant_name, (i + 1) as i32))?;
        }

        self.proto.add_enum(enum_def)?;
        message.add_field(Field::new("status", &enum_name, 1, FieldRule::Optional))
    }

    fn schema_to_type(
        &mut self,
        schema: &Schema,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<String, ConverterError> {
        if let Some(ref_path) = &schema.ref_path {
            return Ok(self.resolve_ref_name(ref_path));
        }

        if let Some(enum_values) = &schema.enum_values {
            let enum_name = format!("Enum_{}", random::<u32>());
            let mut enum_def = Enum::new(&enum_name);

            for (i, value) in enum_values.iter().enumerate() {
                let variant_name = match value {
                    serde_json::Value::String(s) => s
                        .to_uppercase()
                        .replace(|c: char| !c.is_alphanumeric(), "_"),
                    serde_json::Value::Number(n) => format!("VALUE_{}", n),
                    _ => format!("VALUE_{}", i + 1),
                };
                enum_def.add_value(EnumValue::new(&variant_name, (i + 1) as i32))?;
            }

            self.proto.add_enum(enum_def)?;
            return Ok(enum_name);
        }

        match schema.type_.as_deref() {
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
                Some("date") => Ok("google.protobuf.Timestamp".to_string()),
                Some("date-time") => Ok("google.protobuf.Timestamp".to_string()),
                Some("byte") => Ok("bytes".to_string()),
                Some("binary") => Ok("bytes".to_string()),
                _ => Ok("string".to_string()),
            },
            Some("array") => {
                let items = schema
                    .items
                    .as_ref()
                    .ok_or(ConverterError::InvalidArrayDefinition)?;
                let item_type = self.schema_ref_to_type(items, definitions, components)?;
                Ok(format!("repeated {}", item_type))
            }
            Some("object") => {
                if schema.properties.is_some() || schema.all_of.is_some() {
                    // Generate nested message for complex objects
                    let temp_name = format!("NestedObject_{}", random::<u32>());
                    let message = self.convert_schema_to_message(
                        &temp_name,
                        schema,
                        definitions,
                        components,
                    )?;
                    self.proto.add_message(message)?;
                    Ok(temp_name)
                } else if let Some(additional_props) = &schema.additional_properties {
                    let value_type =
                        self.schema_ref_to_type(additional_props, definitions, components)?;
                    Ok(format!("map<string, {}>", value_type))
                } else {
                    Ok("google.protobuf.Struct".to_string())
                }
            }
            None if schema.enum_values.is_some() => {
                let temp_name = format!("Enum_{}", random::<u32>());
                let mut enum_def = Enum::new(&temp_name);
                for (i, value) in schema.enum_values.as_ref().unwrap().iter().enumerate() {
                    let variant_name = match value {
                        serde_json::Value::String(s) => s
                            .to_uppercase()
                            .replace(|c: char| !c.is_alphanumeric(), "_"),
                        _ => format!("VALUE_{}", i + 1),
                    };
                    enum_def.add_value(EnumValue::new(&variant_name, (i + 1) as i32))?;
                }
                self.proto.add_enum(enum_def)?;
                Ok(temp_name)
            }
            None => Err(ConverterError::UnsupportedSchemaType("unknown".to_string())),
            Some(t) => Err(ConverterError::UnsupportedSchemaType(t.to_string())),
        }
    }

    fn schema_ref_to_type(
        &mut self,
        schema_ref: &SchemaRef,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<String, ConverterError> {
        match schema_ref {
            SchemaRef::Ref { ref_path } => Ok(self.resolve_ref_name(ref_path)),
            SchemaRef::Inline(schema) => self.schema_to_type(schema, definitions, components),
        }
    }

    fn process_services(
        &mut self,
        paths: &HashMap<String, PathItem>,
        spec: &SwaggerDoc,
    ) -> Result<(), ConverterError> {
        let mut services: BTreeMap<String, Vec<(String, String, &Operation)>> = BTreeMap::new();

        // Get definitions and components
        let definitions = spec.definitions.as_ref().unwrap_or_else(|| {
            static EMPTY: once_cell::sync::Lazy<HashMap<String, Schema>> =
                once_cell::sync::Lazy::new(|| HashMap::new());
            &EMPTY
        });

        let components = spec.components.as_ref();

        for (path, item) in paths {
            self.collect_operations(&mut services, path, "GET", item.get.as_ref());
            self.collect_operations(&mut services, path, "POST", item.post.as_ref());
            self.collect_operations(&mut services, path, "PUT", item.put.as_ref());
            self.collect_operations(&mut services, path, "DELETE", item.delete.as_ref());
            self.collect_operations(&mut services, path, "PATCH", item.patch.as_ref());
        }

        if let Some(default_ops) = services.remove("Default") {
            if !default_ops.is_empty() {
                self.generate_service("Default", &default_ops, definitions, components)?;
            }
        }

        for (tag, methods) in services {
            if methods.is_empty() {
                continue;
            }

            let service_name = self.to_pascal_case(&tag);
            self.generate_service(&service_name, &methods, definitions, components)?;
        }

        Ok(())
    }

    fn generate_service(
        &mut self,
        service_name: &str,
        methods: &[(String, String, &Operation)],
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<(), ConverterError> {
        let mut service = Service::new(&format!("{}Service", service_name));

        for (path, http_method, operation) in methods {
            let method_name = self.generate_method_name(path, http_method, operation);

            let (request_type, request_messages) = self.generate_request_message(
                service_name,
                &method_name,
                operation,
                definitions,
                components,
            )?;

            for message in request_messages {
                self.proto.add_message(message)?;
            }

            let response_type = self.generate_response_type(operation, definitions, components)?;

            let mut method = Method::new(&method_name, &request_type, &response_type);

            if let Some(summary) = &operation.summary {
                method.add_comment(summary);
            }
            if let Some(description) = &operation.description {
                for line in description.lines() {
                    method.add_comment(line.trim());
                }
            }
            if operation.deprecated.unwrap_or(false) {
                method.add_comment("Deprecated");
            }

            method.add_option("http_method", http_method);
            method.add_option("http_path", path);

            service.add_method(method)?;
        }

        self.proto.add_service(service)?;
        Ok(())
    }

    fn generate_request_message(
        &mut self,
        service_name: &str,
        method_name: &str,
        operation: &Operation,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<(String, Vec<Message>), ConverterError> {
        let mut messages = Vec::new();
        let mut has_query = false;
        let mut has_body = false;
        let mut query_message_name = String::new();

        if let Some(parameters) = &operation.parameters {
            let query_params: Vec<_> = parameters
                .iter()
                .filter(|p| p.in_ == "query" || p.in_ == "path")
                .collect();

            if !query_params.is_empty() {
                has_query = true;
                query_message_name = format!("{}{}QueryParams", service_name, method_name);
                let message = self.generate_parameters_message(
                    &query_message_name,
                    query_params,
                    definitions,
                    components,
                )?;
                messages.push(message);
            }

            // Process body parameters (Swagger 2.0)
            if let Some(body_param) = parameters.iter().find(|p| p.in_ == "body") {
                has_body = true;
                let body_message_name = format!("{}{}RequestBody", service_name, method_name);
                let mut fake_request_body = RequestBody {
                    description: body_param.description.clone(),
                    content: HashMap::new(),
                    required: body_param.required,
                };

                if let Some(schema_ref) = &body_param.schema {
                    let media_type = MediaType {
                        schema: Some(schema_ref.clone()),
                        example: None,
                        examples: None,
                    };
                    fake_request_body
                        .content
                        .insert("application/json".to_string(), media_type);
                }

                let message = self.generate_body_message(
                    &body_message_name,
                    &fake_request_body,
                    definitions,
                    components,
                )?;
                messages.push(message);
            }
        }

        // Process request body (OpenAPI 3.0)
        if let Some(request_body) = &operation.request_body {
            has_body = true;
            let body_message_name = format!("{}{}RequestBody", service_name, method_name);
            let message = self.generate_body_message(
                &body_message_name,
                request_body,
                definitions,
                components,
            )?;
            messages.push(message);
        }

        let request_type = match (has_query, has_body) {
            (true, true) => {
                let combined_name = format!("{}{}Request", service_name, method_name);
                let mut combined_message = Message::new(&combined_name);
                combined_message.add_field(Field::new(
                    "params",
                    &query_message_name,
                    1,
                    FieldRule::Optional,
                ))?;
                combined_message.add_field(Field::new(
                    "body",
                    &format!("{}{}RequestBody", service_name, method_name),
                    2,
                    FieldRule::Optional,
                ))?;
                messages.push(combined_message);
                combined_name
            }
            (true, false) => query_message_name,
            (false, true) => format!("{}{}RequestBody", service_name, method_name),
            (false, false) => "google.protobuf.Empty".to_string(),
        };

        Ok((request_type, messages))
    }

    fn generate_response_type(
        &mut self,
        operation: &Operation,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<String, ConverterError> {
        // Find first successful response (2xx)
        let success_response = operation
            .responses
            .iter()
            .find(|(code, _)| code.starts_with('2'))
            .map(|(_, r)| r);

        if let Some(response) = success_response {
            // OpenAPI 3.0 style - check content first
            if let Some(content) = &response.content {
                if let Some((_, media_type)) = content.iter().next() {
                    if let Some(schema_ref) = &media_type.schema {
                        let type_name =
                            self.schema_ref_to_type(schema_ref, definitions, components)?;

                        // НОВЫЙ КОД: Обработка массивов
                        if type_name.starts_with("repeated ") {
                            let item_type = type_name.trim_start_matches("repeated ");
                            let list_type = format!("{}List", item_type);

                            if !self.generated_messages.contains_key(&list_type) {
                                let mut list_message = Message::new(&list_type);
                                list_message.add_field(Field::new(
                                    "items",
                                    &type_name,
                                    1,
                                    FieldRule::Optional,
                                ))?;
                                self.proto.add_message(list_message)?;
                                self.generated_messages.insert(list_type.clone(), 1);
                            }

                            return Ok(list_type);
                        }

                        return Ok(type_name);
                    }
                }
            }

            // Swagger 2.0 compatibility - check schema directly
            if let Some(schema_ref) = &response.schema {
                return self.schema_ref_to_type(schema_ref, definitions, components);
            }

            if let Some(ref_path) = &response.ref_path {
                return Ok(self.resolve_ref_name(ref_path));
            }
        }

        Ok("google.protobuf.Empty".to_string())
    }

    fn generate_parameters_message(
        &mut self,
        message_name: &str,
        parameters: Vec<&Parameter>,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<Message, ConverterError> {
        if let Some(message) = self.proto.find_message(message_name) {
            return Ok(message.clone());
        }

        let mut message = Message::new(message_name);
        let mut field_number = 1;

        for param in parameters {
            if let Some(desc) = &param.description {
                message.add_comment(desc);
            }

            let proto_type = if let Some(schema_ref) = &param.schema {
                self.schema_ref_to_type(schema_ref, definitions, components)?
            } else {
                match param.type_.as_deref() {
                    Some("integer") => "int64".to_string(),
                    Some("number") => "double".to_string(),
                    Some("boolean") => "bool".to_string(),
                    _ => "string".to_string(),
                }
            };

            let rule = if param.required.unwrap_or(false) {
                FieldRule::Required
            } else {
                FieldRule::Optional
            };
            let field_name = self.sanitize_field_name(&param.name);

            message.add_field(Field::new(&field_name, &proto_type, field_number, rule))?;
            field_number += 1;
        }

        Ok(message)
    }

    fn generate_body_message(
        &mut self,
        message_name: &str,
        request_body: &RequestBody,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<Message, ConverterError> {
        if let Some(message) = self.proto.find_message(message_name) {
            return Ok(message.clone());
        }

        let mut message = Message::new(message_name);

        if let Some(description) = &request_body.description {
            message.add_comment(description);
        }

        if let Some((content_type, media_type)) = request_body.content.iter().next() {
            if let Some(schema_ref) = &media_type.schema {
                let proto_type = self.schema_ref_to_type(schema_ref, definitions, components)?;

                if proto_type.contains("map<") || proto_type == "google.protobuf.Struct" {
                    let mut field = Field::new("data", &proto_type, 1, FieldRule::Optional);
                    field.add_option("json_name", content_type);
                    message.add_field(field)?;
                } else {
                    let mut field = Field::new("data", &proto_type, 1, FieldRule::Optional);
                    field.add_comment(&format!("Content-Type: {}", content_type));
                    message.add_field(field)?;
                }
            } else {
                message.add_field(Field::new("data", "string", 1, FieldRule::Optional))?;
            }
        } else {
            message.add_comment("No content schema defined");
        }

        Ok(message)
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
                let clean_path = path
                    .trim_matches('/')
                    .replace(['/', '{', '}'], "_")
                    .replace(|c: char| !c.is_alphanumeric(), "");
                format!("{}{}", http_method, self.to_pascal_case(&clean_path))
            },
            |id| self.to_pascal_case(id),
        )
    }

    fn resolve_schema_ref(
        &self,
        schema_ref: &SchemaRef,
        definitions: &HashMap<String, Schema>,
        components: Option<&Components>,
    ) -> Result<Schema, ConverterError> {
        match schema_ref {
            SchemaRef::Ref { ref_path } => {
                let ref_name = ref_path
                    .split('/')
                    .last()
                    .ok_or_else(|| ConverterError::MissingReference(ref_path.clone()))?;

                // Check definitions (Swagger 2.0)
                if let Some(schema) = definitions.get(ref_name) {
                    return Ok(schema.clone());
                }

                // Check components (OpenAPI 3.0)
                if let Some(components) = components {
                    if let Some(schemas) = &components.schemas {
                        if let Some(schema) = schemas.get(ref_name) {
                            return Ok(schema.clone());
                        }
                    }
                }

                Err(ConverterError::MissingReference(ref_path.clone()))
            }
            SchemaRef::Inline(schema) => Ok(*schema.clone()),
        }
    }

    fn sanitize_field_name(&self, name: &str) -> String {
        let mut sanitized = String::with_capacity(name.len());
        let mut prev_was_underscore = false;

        for c in name.chars() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' => {
                    sanitized.push(c);
                    prev_was_underscore = false;
                }
                _ => {
                    if !prev_was_underscore && !sanitized.is_empty() {
                        sanitized.push('_');
                        prev_was_underscore = true;
                    }
                }
            }
        }

        // Remove trailing underscore if present
        if sanitized.ends_with('_') {
            sanitized.pop();
        }

        // Names can't start with digit
        if sanitized
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            sanitized = format!("_{}", sanitized);
        }

        // Name can't be empty
        if sanitized.is_empty() {
            sanitized = "field".to_string();
        }

        sanitized
    }

    fn to_pascal_case(&self, s: &str) -> String {
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

    fn resolve_ref_name(&self, ref_path: &str) -> String {
        ref_path
            .split('/')
            .last()
            .unwrap_or("UnknownRef")
            .to_string()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
enum SchemaRef {
    Ref {
        #[serde(rename = "$ref")]
        ref_path: String,
    },
    Inline(Box<Schema>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Schema {
    #[serde(rename = "type")]
    type_: Option<String>,
    format: Option<String>,
    description: Option<String>,
    items: Option<Box<SchemaRef>>,
    properties: Option<HashMap<String, Schema>>,
    additional_properties: Option<Box<SchemaRef>>,
    required: Option<Vec<String>>,
    #[serde(rename = "enum")]
    enum_values: Option<Vec<serde_json::Value>>,
    #[serde(rename = "$ref")]
    ref_path: Option<String>,
    one_of: Option<Vec<SchemaRef>>,
    all_of: Option<Vec<SchemaRef>>,
    any_of: Option<Vec<SchemaRef>>,
    nullable: Option<bool>,
    default: Option<serde_json::Value>,
    example: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SwaggerDoc {
    swagger: Option<String>,
    openapi: Option<String>,
    info: Info,
    paths: HashMap<String, PathItem>,
    definitions: Option<HashMap<String, Schema>>,
    components: Option<Components>,
    tags: Option<Vec<Tag>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Info {
    title: String,
    description: Option<String>,
    version: String,
    contact: Option<Contact>,
    license: Option<License>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Contact {
    name: Option<String>,
    url: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct License {
    name: String,
    url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Tag {
    name: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Components {
    schemas: Option<HashMap<String, Schema>>,
    responses: Option<HashMap<String, Response>>,
    parameters: Option<HashMap<String, Parameter>>,
    examples: Option<HashMap<String, Example>>,
    request_bodies: Option<HashMap<String, RequestBody>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PathItem {
    get: Option<Operation>,
    post: Option<Operation>,
    put: Option<Operation>,
    delete: Option<Operation>,
    patch: Option<Operation>,
    head: Option<Operation>,
    options: Option<Operation>,
    trace: Option<Operation>,
    parameters: Option<Vec<Parameter>>,
    #[serde(rename = "$ref")]
    ref_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Operation {
    tags: Option<Vec<String>>,
    summary: Option<String>,
    description: Option<String>,
    operation_id: Option<String>,
    parameters: Option<Vec<Parameter>>,
    request_body: Option<RequestBody>,
    responses: HashMap<String, Response>,
    deprecated: Option<bool>,
    security: Option<Vec<HashMap<String, Vec<String>>>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Parameter {
    name: String,
    #[serde(rename = "in")]
    in_: String,
    description: Option<String>,
    required: Option<bool>,
    schema: Option<SchemaRef>,
    #[serde(rename = "type")]
    type_: Option<String>,
    format: Option<String>,
    default: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RequestBody {
    description: Option<String>,
    content: HashMap<String, MediaType>,
    required: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MediaType {
    schema: Option<SchemaRef>,
    example: Option<serde_json::Value>,
    examples: Option<HashMap<String, Example>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Example {
    summary: Option<String>,
    description: Option<String>,
    value: Option<serde_json::Value>,
    external_value: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Response {
    description: String,
    content: Option<HashMap<String, MediaType>>,
    #[serde(rename = "$ref")]
    ref_path: Option<String>,
    headers: Option<HashMap<String, Header>>,
    // For Swagger 2.0 compatibility:
    schema: Option<SchemaRef>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Header {
    description: Option<String>,
    #[serde(rename = "type")]
    type_: String,
    format: Option<String>,
}
