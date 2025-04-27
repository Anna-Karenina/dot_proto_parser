use std::path::Path;

use dot_proto_parser::{ProtoParser, SwaggerToProtoConverter};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Конвертация Swagger → Proto
    // let mut converter = SwaggerToProtoConverter::new("api");
    // converter.convert_file(Path::new("swagger.json"), Path::new("api.proto"))?;

    // Обратная конвертация Proto → Model
    let mut parser = ProtoParser::new();
    let proto_file = parser.parse_file(Path::new("api.proto"))?;

    println!("Parsed proto file: {:?}", proto_file);

    Ok(())
}
