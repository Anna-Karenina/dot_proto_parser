use std::path::Path;

use crate::{
    Enum, EnumValue, Error, Field, FieldRule, Message, Method, ProtoFile, ProtoParseError, Service,
};

pub struct ProtoParser {
    current_line: usize,
}

impl ProtoParser {
    pub fn new() -> Self {
        Self { current_line: 0 }
    }

    pub fn parse_file(&mut self, path: &Path) -> Result<ProtoFile, Error> {
        let content = std::fs::read_to_string(path)?;
        self.parse(&content)
    }

    pub fn parse(&mut self, content: &str) -> Result<ProtoFile, Error> {
        let mut proto_file = ProtoFile::default();
        let mut stack: Vec<ProtoItem> = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            self.current_line = line_num + 1;
            let line = line.trim();

            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            match self.parse_line(line, &mut stack)? {
                LineType::Syntax(s) => proto_file.syntax = s,
                LineType::Package(p) => proto_file.package = p,
                LineType::Import(i) => proto_file.imports.push(i),
                // LineType::Option(k, v) => {
                //     proto_file.options.insert(k, v);
                // }
                LineType::Message(m) => stack.push(ProtoItem::Message(m)),
                LineType::Enum(e) => stack.push(ProtoItem::Enum(e)),
                LineType::Service(s) => stack.push(ProtoItem::Service(s)),
                LineType::Field(f) => {
                    if let Some(ProtoItem::Message(msg)) = stack.last_mut() {
                        msg.add_field(f)?;
                    }
                }
                LineType::EnumValue(v) => {
                    if let Some(ProtoItem::Enum(en)) = stack.last_mut() {
                        en.add_value(v)?;
                    }
                }
                LineType::Method(m) => {
                    if let Some(ProtoItem::Service(svc)) = stack.last_mut() {
                        svc.add_method(m)?;
                    }
                }
                LineType::End => {
                    if let Some(item) = stack.pop() {
                        match item {
                            ProtoItem::Message(m) => proto_file.add_message(m)?,
                            ProtoItem::Enum(e) => proto_file.add_enum(e)?,
                            ProtoItem::Service(s) => proto_file.add_service(s)?,
                        }
                    }
                }
            }
        }

        Ok(proto_file)
    }

    fn parse_line(&self, line: &str, stack: &[ProtoItem]) -> Result<LineType, ProtoParseError> {
        if line == "}" {
            return Ok(LineType::End);
        }

        if line.starts_with("syntax") {
            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() != 2 {
                return Err(self.parse_error("Invalid syntax declaration"));
            }
            return Ok(LineType::Syntax(
                parts[1].trim_matches(|c| c == '"' || c == ';').to_string(),
            ));
        }

        if line.starts_with("package") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 2 || !parts[1].ends_with(';') {
                return Err(self.parse_error("Invalid package declaration"));
            }
            return Ok(LineType::Package(
                parts[1].trim_end_matches(';').to_string(),
            ));
        }

        if line.starts_with("import") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 2 || !parts[1].ends_with(';') {
                return Err(self.parse_error("Invalid import declaration"));
            }
            return Ok(LineType::Import(
                parts[1].trim_matches(|c| c == '"' || c == ';').to_string(),
            ));
        }

        // if line.starts_with("option") {
        //     let option_part = line.trim_start_matches("option").trim();
        //     let parts: Vec<&str> = option_part.splitn(2, '=').collect();
        //     if parts.len() != 2 {
        //         return Err(self.parse_error("Invalid option format"));
        //     }
        //     return Ok(LineType::Option(
        //         parts[0].trim().to_string(),
        //         parts[1]
        //             .trim()
        //             .trim_matches(|c| c == '"' || c == ';')
        //             .to_string(),
        //     ));
        // }

        if line.starts_with("message") {
            let name = line["message".len()..].split('{').next().unwrap().trim();
            if name.is_empty() {
                return Err(self.parse_error("Message name cannot be empty"));
            }
            return Ok(LineType::Message(Message::new(name)));
        }

        if line.starts_with("enum") {
            let name = line["enum".len()..].split('{').next().unwrap().trim();
            if name.is_empty() {
                return Err(self.parse_error("Enum name cannot be empty"));
            }
            return Ok(LineType::Enum(Enum::new(name)));
        }

        if line.starts_with("service") {
            let name = line["service".len()..].split('{').next().unwrap().trim();
            if name.is_empty() {
                return Err(self.parse_error("Service name cannot be empty"));
            }
            return Ok(LineType::Service(Service::new(name)));
        }

        if line.starts_with("rpc") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            dbg!(&parts);
            if parts.len() < 5 {
                return Err(self.parse_error("Invalid method declaration"));
            }
            return Ok(LineType::Method(Method::new(
                parts[1],
                parts[3].trim_matches('('),
                parts[4].trim_matches(')'),
            )));
        }

        if let Some(ProtoItem::Message(_)) = stack.last() {
            return self.parse_field(line);
        }

        if let Some(ProtoItem::Enum(_)) = stack.last() {
            return self.parse_enum_value(line);
        }

        Err(self.parse_error("Unknown line type"))
    }

    fn parse_field(&self, line: &str) -> Result<LineType, ProtoParseError> {
        let line = line.trim_end_matches(';');
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() < 4 {
            return Err(self.parse_error("Invalid field declaration"));
        }

        let mut idx = 0;
        let rule = match parts[idx] {
            "repeated" => {
                idx += 1;
                FieldRule::Repeated
            }
            "optional" => {
                idx += 1;
                FieldRule::Optional
            }
            "required" => {
                idx += 1;
                FieldRule::Required
            }
            _ => FieldRule::Required,
        };

        let type_ = parts[idx].to_string();
        idx += 1;
        let name = parts[idx].to_string();
        idx += 1;

        if parts[idx] != "=" {
            return Err(self.parse_error("Expected '=' in field declaration"));
        }
        idx += 1;

        let number = parts[idx]
            .parse()
            .map_err(|_| self.parse_error("Invalid field number"))?;

        let mut field = Field::new(&name, &type_, number, rule);

        if let Some(options_start) = line.find('[') {
            let options_str = &line[options_start..].trim_matches(|c| c == '[' || c == ']');
            for option in options_str.split(',') {
                let option = option.trim();
                if let Some((key, value)) = option.split_once('=') {
                    field.add_option(key.trim(), value.trim().trim_matches('"'));
                }
            }
        }

        Ok(LineType::Field(field))
    }

    fn parse_enum_value(&self, line: &str) -> Result<LineType, ProtoParseError> {
        let line = line.trim_end_matches(';');
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() != 3 || parts[1] != "=" {
            return Err(self.parse_error("Invalid enum value declaration"));
        }

        Ok(LineType::EnumValue(EnumValue::new(
            parts[0],
            parts[2]
                .parse()
                .map_err(|_| self.parse_error("Invalid enum value number"))?,
        )))
    }

    fn parse_error(&self, msg: &str) -> ProtoParseError {
        ProtoParseError::ParseError {
            line: self.current_line,
            message: msg.to_string(),
        }
    }
}

enum ProtoItem {
    Message(Message),
    Enum(Enum),
    Service(Service),
}

enum LineType {
    Syntax(String),
    Package(String),
    Import(String),
    // Option(String, String),
    Message(Message),
    Enum(Enum),
    Service(Service),
    Field(Field),
    EnumValue(EnumValue),
    Method(Method),
    End,
}
