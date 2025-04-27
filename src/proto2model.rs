use std::path::Path;

use crate::{
    Enum, EnumValue, Error, Field, FieldRule, Message, Method, ProtoFile, ProtoParseError, Service,
};

pub struct ProtoParser {
    current_line: usize,
    pending_comments: Vec<String>,
}

impl ProtoParser {
    pub fn new() -> Self {
        Self {
            current_line: 0,
            pending_comments: Vec::new(),
        }
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

            if line.is_empty() {
                continue;
            }

            match self.parse_line(line, &mut stack)? {
                LineType::Syntax(s) => {
                    proto_file.syntax = s;
                    self.pending_comments.clear();
                }
                LineType::Package(p) => {
                    proto_file.package = p;
                    self.pending_comments.clear();
                }
                LineType::Import(i) => {
                    proto_file.imports.push(i);
                    self.pending_comments.clear();
                }
                LineType::Message(mut m) => {
                    m.comments = std::mem::take(&mut self.pending_comments);
                    stack.push(ProtoItem::Message(m));
                }
                LineType::Enum(mut e) => {
                    e.comments = std::mem::take(&mut self.pending_comments);
                    stack.push(ProtoItem::Enum(e));
                }
                LineType::Service(mut s) => {
                    s.comments = std::mem::take(&mut self.pending_comments);
                    stack.push(ProtoItem::Service(s));
                }
                LineType::Field(mut f) => {
                    f.comments = std::mem::take(&mut self.pending_comments);
                    if let Some(ProtoItem::Message(msg)) = stack.last_mut() {
                        msg.add_field(f)?;
                    }
                }
                LineType::EnumValue(mut v) => {
                    v.comments = std::mem::take(&mut self.pending_comments);
                    if let Some(ProtoItem::Enum(en)) = stack.last_mut() {
                        en.add_value(v)?;
                    }
                }
                LineType::Method(mut m) => {
                    m.comments = std::mem::take(&mut self.pending_comments);
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
                    self.pending_comments.clear();
                }
                LineType::Comment => {}
            }
        }

        Ok(proto_file)
    }

    fn parse_line(&mut self, line: &str, stack: &[ProtoItem]) -> Result<LineType, ProtoParseError> {
        if line.is_empty() {
            return Ok(LineType::Comment);
        }

        if line.starts_with("//") {
            let comment = line[2..].trim().to_string();
            self.pending_comments.push(comment);
            return Ok(LineType::Comment);
        }

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
            if parts.len() < 5 {
                return Err(self.parse_error("Invalid method declaration"));
            }

            let mut method = Method::new(
                parts[1],
                parts[3].trim_matches('('),
                parts[4].trim_matches(')'),
            );

            if let Some(options_start) = line.find('[') {
                let options_str = &line[options_start..].trim_matches(|c| c == '[' || c == ']');
                for option in options_str.split(',') {
                    let option = option.trim();
                    if let Some((key, value)) = option.split_once('=') {
                        method.add_option(key.trim(), value.trim().trim_matches('"'));
                    }
                }
            }

            return Ok(LineType::Method(method));
        }

        if let Some(ProtoItem::Message(_)) = stack.last() {
            return self.parse_field(line);
        }

        if let Some(ProtoItem::Enum(_)) = stack.last() {
            return self.parse_enum_value(line);
        }

        Err(self.parse_error("Unknown line type"))
    }

    fn parse_field(&mut self, line: &str) -> Result<LineType, ProtoParseError> {
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
        field.comments = std::mem::take(&mut self.pending_comments);

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

    fn parse_enum_value(&mut self, line: &str) -> Result<LineType, ProtoParseError> {
        let line = line.trim_end_matches(';');
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() != 3 || parts[1] != "=" {
            return Err(self.parse_error("Invalid enum value declaration"));
        }

        let mut value = EnumValue::new(
            parts[0],
            parts[2]
                .parse()
                .map_err(|_| self.parse_error("Invalid enum value number"))?,
        );

        value.comments = std::mem::take(&mut self.pending_comments);
        Ok(LineType::EnumValue(value))
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
    Message(Message),
    Enum(Enum),
    Service(Service),
    Field(Field),
    EnumValue(EnumValue),
    Method(Method),
    End,
    Comment,
}
