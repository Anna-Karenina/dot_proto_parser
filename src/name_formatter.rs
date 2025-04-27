pub trait NameFormatter {
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

        // Удаляем завершающий подчеркивание если есть
        if sanitized.ends_with('_') {
            sanitized.pop();
        }

        if sanitized
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            sanitized = format!("_{}", sanitized);
        }

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
}
